import { createHash, createPublicKey, verify as verifySignature } from "node:crypto";
import { createWriteStream } from "node:fs";
import { mkdir, open, readFile, rename, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import https from "node:https";
import path from "node:path";
import { fileURLToPath } from "node:url";

const LISTING_URL = new URL("http://downloads.simulationcraft.org/nightly/?C=M;O=D");
const OFFICIAL_HOST = "downloads.simulationcraft.org";
const MAX_LISTING_BYTES = 2 * 1024 * 1024;
const MAX_CATALOG_BYTES = 1024 * 1024;
const MAX_ARTIFACT_BYTES = 512 * 1024 * 1024;
const DEFAULT_TIMEOUT_MS = 15 * 60 * 1000;
const MAC_PATTERN = /^simc-(\d{4})-(\d{2})-macos-([0-9a-f]{7,40})\.dmg$/;
const WINDOWS_PATTERN = /^simc-(\d{4})\.(\d{2})\.([0-9a-f]{7,40})-win64\.7z$/;

function fail(message) {
  throw new Error(message);
}

function request(url, { method = "GET", timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
  return new Promise((resolve, reject) => {
    const transport = url.protocol === "https:" ? https : http;
    const handle = transport.request(url, { method }, resolve);
    handle.setTimeout(timeoutMs, () => handle.destroy(new Error(`request timed out after ${timeoutMs} ms`)));
    handle.once("error", reject);
    handle.end();
  });
}

function assertOfficialUrl(url, filename, allowListing = false) {
  if (!['http:', 'https:'].includes(url.protocol)
    || url.hostname !== OFFICIAL_HOST
    || url.port !== ""
    || url.username !== ""
    || url.password !== ""
    || url.hash !== "") {
    fail(`URL is outside the official SimulationCraft boundary: ${url.href}`);
  }
  if (allowListing) {
    if (url.pathname !== "/nightly/" || url.search !== "?C=M;O=D") fail(`invalid nightly listing URL: ${url.href}`);
  } else if (url.pathname !== `/nightly/${filename}` || url.search !== "") {
    fail(`artifact URL is not an exact official nightly file: ${url.href}`);
  }
}

function supportedArtifact(filename, platform, architecture, version, revision) {
  return {
    schema_version: 1,
    simc_version: version,
    game_channel: "live",
    platform,
    architecture,
    build: revision,
    filename,
    url: `http://${OFFICIAL_HOST}/nightly/${filename}`,
  };
}

export function parseNightlyListing(html) {
  if (typeof html !== "string" || html.length === 0 || Buffer.byteLength(html) > MAX_LISTING_BYTES) {
    fail("nightly listing must be non-empty and no larger than 2 MiB");
  }
  const hrefs = [...html.matchAll(/href\s*=\s*["']([^"']+)["']/gi)].map((match) => match[1]);
  let macos;
  let windows;
  for (const href of hrefs) {
    if (href.includes("/") || href.includes("?") || href.includes("#")) continue;
    const macMatch = href.match(MAC_PATTERN);
    if (!macos && macMatch) {
      macos = supportedArtifact(href, "macos", "aarch64", `${macMatch[1]}-${macMatch[2]}`, macMatch[3]);
    }
    const windowsMatch = href.match(WINDOWS_PATTERN);
    if (!windows && windowsMatch) {
      windows = supportedArtifact(href, "windows", "x86_64", `${windowsMatch[1]}-${windowsMatch[2]}`, windowsMatch[3]);
    }
  }
  if (!macos || !windows) fail("nightly listing has no complete macOS ARM64 and Windows x64 pair");
  if (macos.simc_version !== windows.simc_version || macos.build !== windows.build) {
    fail(`latest supported artifacts disagree: ${macos.simc_version}/${macos.build} vs ${windows.simc_version}/${windows.build}`);
  }
  return [macos, windows];
}

function validateCatalogShape(catalog, label) {
  if (!catalog?.payload || !Number.isSafeInteger(catalog.payload.sequence) || !Array.isArray(catalog.payload.manifests)) {
    fail(`${label} is not a signed runtime catalog`);
  }
  return catalog;
}

function canonicalPayloadBytes(payload) {
  return Buffer.from(JSON.stringify(payload));
}

export function verifyCatalogSignatures(catalog, trustedKeys, nowUnixSeconds) {
  validateCatalogShape(catalog, "catalog");
  validateTimestamp(nowUnixSeconds, "nowUnixSeconds");
  if (!Array.isArray(trustedKeys) || trustedKeys.length === 0) fail("at least one trusted catalog key is required");
  if (!Array.isArray(catalog.signatures) || catalog.signatures.length === 0) fail("catalog has no signatures");
  if (catalog.payload.generated_at_unix_seconds > nowUnixSeconds + 300) fail("catalog was generated too far in the future");
  if (catalog.payload.expires_at_unix_seconds <= nowUnixSeconds) fail("catalog has expired");
  const message = Buffer.concat([Buffer.from("SimShredder runtime catalog v1\0"), canonicalPayloadBytes(catalog.payload)]);
  const verified = catalog.signatures.some((signature) => {
    if (signature?.algorithm !== "ed25519" || typeof signature.key_id !== "string" || typeof signature.signature_base64 !== "string") return false;
    const trusted = trustedKeys.find((key) => key.key_id === signature.key_id && key.algorithm === "ed25519");
    if (!trusted || !/^[A-Za-z0-9+/]+={0,2}$/.test(trusted.public_key_base64)) return false;
    try {
      const raw = Buffer.from(trusted.public_key_base64, "base64");
      if (raw.length !== 32) return false;
      const spki = Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), raw]);
      const key = createPublicKey({ key: spki, format: "der", type: "spki" });
      return verifySignature(null, message, key, Buffer.from(signature.signature_base64, "base64"));
    } catch {
      return false;
    }
  });
  if (!verified) fail("catalog has no valid signature from a trusted key");
  return catalog;
}

export function highestCatalog(catalogs) {
  if (!Array.isArray(catalogs) || catalogs.length === 0) fail("at least one verified catalog is required");
  return catalogs.reduce((highest, catalog, index) => {
    const validated = validateCatalogShape(catalog, `catalog ${index + 1}`);
    if (!highest || validated.payload.sequence > highest.payload.sequence) return validated;
    if (validated.payload.sequence === highest.payload.sequence
      && JSON.stringify(validated.payload) !== JSON.stringify(highest.payload)) {
      fail(`catalog sequence ${validated.payload.sequence} has conflicting payloads`);
    }
    return highest;
  }, undefined);
}

export function catalogMatchesDiscovery(catalog, discovered) {
  if (catalog.payload.manifests.length !== discovered.length) return false;
  return discovered.every((candidate) => catalog.payload.manifests.some((manifest) =>
    manifest.platform === candidate.platform
    && manifest.architecture === candidate.architecture
    && manifest.game_channel === "live"
    && manifest.simc_version === candidate.simc_version
    && manifest.build === candidate.build
    && manifest.filename === candidate.filename
    && manifest.url === candidate.url));
}

function validateTimestamp(value, label) {
  if (!Number.isSafeInteger(value) || value <= 0) fail(`${label} must be a positive integer`);
  return value;
}

export function buildPayload({ catalogs, discovered, artifacts, generatedAt, expiresAt }) {
  const baseline = highestCatalog(catalogs);
  validateTimestamp(generatedAt, "generatedAt");
  validateTimestamp(expiresAt, "expiresAt");
  if (expiresAt <= generatedAt || expiresAt - generatedAt > 14 * 24 * 60 * 60) {
    fail("catalog validity must be positive and no longer than 14 days");
  }
  const records = new Map(artifacts.map((artifact) => [`${artifact.platform}/${artifact.architecture}`, artifact]));
  const manifests = discovered.map((candidate) => {
    const artifact = records.get(`${candidate.platform}/${candidate.architecture}`);
    if (!artifact || artifact.filename !== candidate.filename) fail(`missing verified artifact for ${candidate.platform}/${candidate.architecture}`);
    if (!Number.isSafeInteger(artifact.size) || artifact.size <= 0 || artifact.size > MAX_ARTIFACT_BYTES) fail(`invalid size for ${artifact.filename}`);
    if (!/^[0-9a-f]{64}$/.test(artifact.sha256)) fail(`invalid SHA-256 for ${artifact.filename}`);
    return { ...candidate, size: artifact.size, sha256: artifact.sha256 };
  });
  return {
    schema_version: 1,
    sequence: baseline.payload.sequence + 1,
    generated_at_unix_seconds: generatedAt,
    expires_at_unix_seconds: expiresAt,
    manifests,
    next_keys: [],
    revoked_key_ids: [],
  };
}

async function readBoundedJson(filename, label) {
  const handle = await open(filename, "r");
  try {
    const stat = await handle.stat();
    if (!stat.isFile() || stat.size <= 0 || stat.size > MAX_CATALOG_BYTES) fail(`${label} is outside the 1 MiB limit`);
    return validateCatalogShape(JSON.parse(await handle.readFile("utf8")), label);
  } finally {
    await handle.close();
  }
}

async function readResponse(response, limit, label) {
  if (response.statusCode !== 200) {
    response.resume();
    fail(`${label} returned HTTP ${response.statusCode}`);
  }
  if (response.headers.location !== undefined) {
    response.resume();
    fail(`${label} attempted a redirect`);
  }
  const chunks = [];
  let received = 0;
  for await (const chunk of response) {
    received += chunk.length;
    if (received > limit) {
      response.destroy();
      fail(`${label} exceeded ${limit} bytes`);
    }
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

export async function fetchListing(options = {}) {
  const url = new URL(options.url ?? LISTING_URL);
  assertOfficialUrl(url, "", true);
  return (await readResponse(await request(url, { timeoutMs: options.timeoutMs }), MAX_LISTING_BYTES, "nightly listing")).toString("utf8");
}

export async function verifyUnchangedAvailability(catalog, options = {}) {
  for (const manifest of catalog.payload.manifests) {
    const url = new URL(manifest.url);
    assertOfficialUrl(url, manifest.filename);
    const response = await request(url, { method: "HEAD", timeoutMs: options.timeoutMs });
    response.resume();
    if (response.statusCode !== 200 || response.headers.location !== undefined) return false;
    if (Number(response.headers["content-length"]) !== manifest.size) return false;
  }
  return true;
}

export async function decideRefresh(catalogs, discovered, availabilityCheck = verifyUnchangedAvailability) {
  const baseline = highestCatalog(catalogs);
  if (!catalogMatchesDiscovery(baseline, discovered)) {
    return { changed: true, baseline };
  }
  return { changed: !(await availabilityCheck(baseline)), baseline };
}

export async function downloadDiscovered(discovered, outputDirectory, options = {}) {
  await mkdir(outputDirectory, { recursive: true });
  const results = [];
  for (const candidate of discovered) {
    const url = new URL(candidate.url);
    assertOfficialUrl(url, candidate.filename);
    const response = await request(url, { timeoutMs: options.timeoutMs });
    if (response.statusCode !== 200 || response.headers.location !== undefined) {
      response.resume();
      fail(`GET ${candidate.url} returned HTTP ${response.statusCode}`);
    }
    const declared = Number(response.headers["content-length"]);
    if (!Number.isSafeInteger(declared) || declared <= 0 || declared > MAX_ARTIFACT_BYTES) {
      response.resume();
      fail(`artifact Content-Length is outside the supported range: ${candidate.filename}`);
    }
    const destination = path.join(outputDirectory, candidate.filename);
    const partial = `${destination}.partial`;
    await rm(partial, { force: true });
    const hash = createHash("sha256");
    let received = 0;
    const output = createWriteStream(partial, { flags: "wx", mode: 0o600 });
    try {
      await new Promise((resolve, reject) => {
        const abort = (error) => { response.destroy(); output.destroy(); reject(error); };
        response.on("data", (chunk) => {
          received += chunk.length;
          if (received > declared || received > MAX_ARTIFACT_BYTES) return abort(new Error(`artifact exceeded its bound: ${candidate.filename}`));
          hash.update(chunk);
        });
        response.once("error", reject);
        output.once("error", reject);
        output.once("finish", resolve);
        response.pipe(output);
      });
      if (received !== declared) fail(`artifact size mismatch: ${candidate.filename}`);
      await rename(partial, destination);
      results.push({ platform: candidate.platform, architecture: candidate.architecture, filename: candidate.filename, size: received, sha256: hash.digest("hex"), path: destination });
    } catch (error) {
      await rm(partial, { force: true });
      throw error;
    }
  }
  return results;
}

function parseArguments(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value === undefined) fail(`invalid argument near ${String(flag)}`);
    const key = flag.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    if (options[key] !== undefined) fail(`duplicate argument ${flag}`);
    options[key] = value;
  }
  if (!options.bundledCatalog || !options.decisionFile || !options.trustRoots || !options.now) {
    fail("--bundled-catalog, --decision-file, --trust-roots and --now are required");
  }
  return options;
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseArguments(argv);
  const trustedKeys = JSON.parse(await readFile(options.trustRoots, "utf8"));
  const now = Number(options.now);
  const catalogs = [verifyCatalogSignatures(await readBoundedJson(options.bundledCatalog, "bundled catalog"), trustedKeys, now)];
  if (options.productionCatalog) catalogs.push(verifyCatalogSignatures(await readBoundedJson(options.productionCatalog, "production catalog"), trustedKeys, now));
  const listing = options.listingFile ? await readFile(options.listingFile, "utf8") : await fetchListing();
  const discovered = parseNightlyListing(listing);
  const refresh = await decideRefresh(catalogs, discovered);
  const baseline = refresh.baseline;
  const unchanged = !refresh.changed;
  const decision = { changed: refresh.changed, baseline_sequence: baseline.payload.sequence, simc_version: discovered[0].simc_version, revision: discovered[0].build, discovered };
  await writeFile(options.decisionFile, `${JSON.stringify(decision, null, 2)}\n`, { flag: "wx" });
  if (!unchanged && options.payloadFile) {
    if (!options.outputDirectory || !options.generatedAt || !options.expiresAt) fail("changed generation requires --output-directory, --generated-at and --expires-at");
    const artifacts = await downloadDiscovered(discovered, options.outputDirectory);
    const payload = buildPayload({ catalogs, discovered, artifacts, generatedAt: Number(options.generatedAt), expiresAt: Number(options.expiresAt) });
    await writeFile(options.payloadFile, `${JSON.stringify(payload, null, 2)}\n`, { flag: "wx" });
  }
  process.stdout.write(`${unchanged ? "unchanged" : "changed"} ${decision.simc_version}/${decision.revision} after sequence ${decision.baseline_sequence}\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
