import { createHash } from "node:crypto";
import { createWriteStream } from "node:fs";
import { mkdir, open, rename, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import https from "node:https";
import path from "node:path";
import { fileURLToPath } from "node:url";

const MAX_CATALOG_BYTES = 1024 * 1024;
const MAX_ARTIFACT_BYTES = 1024 * 1024 * 1024;
const DEFAULT_TIMEOUT_MS = 15 * 60 * 1000;
const OFFICIAL_HOST = "downloads.simulationcraft.org";

function fail(message) {
  throw new Error(message);
}

function requireString(value, label) {
  if (typeof value !== "string" || value.length === 0) fail(`${label} must be a non-empty string`);
  return value;
}

export function validateManifest(manifest) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) {
    fail("runtime manifest must be an object");
  }
  const platform = requireString(manifest.platform, "platform");
  const architecture = requireString(manifest.architecture, "architecture");
  const filename = requireString(manifest.filename, "filename");
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(filename)) fail(`unsafe artifact filename: ${filename}`);
  if (!Number.isSafeInteger(manifest.size) || manifest.size <= 0 || manifest.size > MAX_ARTIFACT_BYTES) {
    fail(`artifact size is outside the supported range: ${String(manifest.size)}`);
  }
  if (typeof manifest.sha256 !== "string" || !/^[0-9a-f]{64}$/.test(manifest.sha256)) {
    fail(`invalid SHA-256 for ${filename}`);
  }

  const url = new URL(requireString(manifest.url, "url"));
  if (!['http:', 'https:'].includes(url.protocol)
    || url.hostname !== OFFICIAL_HOST
    || url.port !== ""
    || url.username !== ""
    || url.password !== ""
    || url.search !== ""
    || url.hash !== ""
    || url.pathname !== `/nightly/${filename}`) {
    fail(`artifact URL is outside the official exact-file boundary: ${url.href}`);
  }
  return { ...manifest, platform, architecture, filename, url: url.href };
}

export async function loadCatalog(catalogPath) {
  const handle = await open(catalogPath, "r");
  try {
    const metadata = await handle.stat();
    if (!metadata.isFile() || metadata.size <= 0 || metadata.size > MAX_CATALOG_BYTES) {
      fail("catalog must be a non-empty regular file no larger than 1 MiB");
    }
    const catalog = JSON.parse(await handle.readFile("utf8"));
    if (!catalog?.payload || !Array.isArray(catalog.payload.manifests)) {
      fail("signed catalog has no manifest array");
    }
    const manifests = catalog.payload.manifests.map(validateManifest);
    const targets = new Set();
    for (const manifest of manifests) {
      const target = `${manifest.platform}/${manifest.architecture}`;
      if (targets.has(target)) fail(`catalog repeats target ${target}`);
      targets.add(target);
    }
    return manifests;
  } finally {
    await handle.close();
  }
}

function request(url, { method, timeoutMs, headers = {} }) {
  return new Promise((resolve, reject) => {
    const transport = url.protocol === "https:" ? https : http;
    const requestHandle = transport.request(url, { method, headers }, resolve);
    requestHandle.setTimeout(timeoutMs, () => requestHandle.destroy(new Error(`request timed out after ${timeoutMs} ms`)));
    requestHandle.once("error", reject);
    requestHandle.end();
  });
}

function expectedContentLength(response, manifest) {
  const raw = response.headers["content-length"];
  if (raw === undefined) fail(`server omitted Content-Length for ${manifest.filename}`);
  const actual = Number(raw);
  if (!Number.isSafeInteger(actual) || actual !== manifest.size) {
    fail(`size mismatch for ${manifest.filename}: expected ${manifest.size}, got ${String(raw)}`);
  }
}

export async function verifyAvailability(manifest, options = {}) {
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const response = await request(new URL(manifest.url), { method: "HEAD", timeoutMs });
  response.resume();
  if (response.statusCode !== 200) fail(`HEAD ${manifest.url} returned HTTP ${response.statusCode}`);
  if (response.headers.location !== undefined) fail(`redirects are not accepted for ${manifest.url}`);
  expectedContentLength(response, manifest);
  return { ...manifest, verified: "availability" };
}

export async function verifyAndDownload(manifest, outputDirectory, options = {}) {
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  await mkdir(outputDirectory, { recursive: true });
  const destination = path.join(outputDirectory, manifest.filename);
  const partial = `${destination}.partial`;
  await rm(partial, { force: true });
  const response = await request(new URL(manifest.url), { method: "GET", timeoutMs });
  if (response.statusCode !== 200) {
    response.resume();
    fail(`GET ${manifest.url} returned HTTP ${response.statusCode}`);
  }
  if (response.headers.location !== undefined) {
    response.resume();
    fail(`redirects are not accepted for ${manifest.url}`);
  }
  expectedContentLength(response, manifest);

  const hash = createHash("sha256");
  let received = 0;
  const output = createWriteStream(partial, { flags: "wx", mode: 0o600 });
  try {
    await new Promise((resolve, reject) => {
      const abort = (error) => {
        response.destroy();
        output.destroy();
        reject(error);
      };
      response.on("data", (chunk) => {
        received += chunk.length;
        if (received > manifest.size) {
          abort(new Error(`download exceeded signed size for ${manifest.filename}`));
          return;
        }
        hash.update(chunk);
      });
      response.once("error", reject);
      output.once("error", reject);
      output.once("finish", resolve);
      response.pipe(output);
    });
    const digest = hash.digest("hex");
    if (received !== manifest.size) fail(`download size mismatch for ${manifest.filename}: ${received}`);
    if (digest !== manifest.sha256) fail(`download SHA-256 mismatch for ${manifest.filename}: ${digest}`);
    await rename(partial, destination);
    return { ...manifest, verified: "sha256", path: destination };
  } catch (error) {
    await rm(partial, { force: true });
    throw error;
  }
}

function parseArguments(argv) {
  const options = { mode: "availability" };
  const seen = new Set();
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value === undefined) fail(`invalid argument near ${String(flag)}`);
    const key = flag.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    if (seen.has(key)) fail(`duplicate argument ${flag}`);
    seen.add(key);
    options[key] = value;
  }
  if (!options.catalog) fail("--catalog is required");
  if (!['availability', 'full'].includes(options.mode)) fail("--mode must be availability or full");
  if (options.mode === "full" && !options.outputDirectory) fail("--output-directory is required in full mode");
  return options;
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseArguments(argv);
  let manifests = await loadCatalog(options.catalog);
  if (options.platform) manifests = manifests.filter((entry) => entry.platform === options.platform);
  if (options.architecture) manifests = manifests.filter((entry) => entry.architecture === options.architecture);
  if (manifests.length === 0) fail("catalog has no manifest matching the requested target");
  if (options.manifestFile) {
    if (manifests.length !== 1) fail("--manifest-file requires exactly one matching target");
    await writeFile(options.manifestFile, `${JSON.stringify(manifests[0], null, 2)}\n`, { flag: "wx" });
  }

  const results = [];
  for (const manifest of manifests) {
    results.push(options.mode === "full"
      ? await verifyAndDownload(manifest, options.outputDirectory)
      : await verifyAvailability(manifest));
  }
  if (options.resultFile) await writeFile(options.resultFile, `${JSON.stringify(results, null, 2)}\n`, { flag: "wx" });
  for (const result of results) process.stdout.write(`verified ${result.platform}/${result.architecture} ${result.filename} (${result.verified})\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
