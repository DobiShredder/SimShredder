import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { basename } from "node:path";
import { fileURLToPath } from "node:url";
import path from "node:path";

function fail(message) { throw new Error(message); }

function argumentsFrom(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index];
    const value = argv[index + 1];
    if (!name?.startsWith("--") || value === undefined) fail(`invalid argument near ${String(name)}`);
    options[name.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = value;
  }
  if (!options.catalog || !options.repository || !options.tag) fail("--catalog, --repository and --tag are required");
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(options.repository)) fail("invalid GitHub repository");
  return options;
}

async function github(pathname, options = {}) {
  const token = process.env.GH_TOKEN;
  if (!token) fail("GH_TOKEN is required");
  const response = await fetch(`https://api.github.com${pathname}`, {
    ...options,
    headers: {
      Accept: "application/vnd.github+json",
      Authorization: `Bearer ${token}`,
      "X-GitHub-Api-Version": "2022-11-28",
      "User-Agent": "SimShredder-runtime-catalog-publisher",
      ...options.headers,
    },
  });
  if (!response.ok) fail(`GitHub API ${options.method ?? "GET"} ${pathname} returned HTTP ${response.status}`);
  if (response.status === 204) return undefined;
  return response.json();
}

export function promotionNames(catalogBytes) {
  const digest = createHash("sha256").update(catalogBytes).digest("hex");
  return { canonical: "runtime-catalog.json", staged: `runtime-catalog.${digest}.staged.json`, backup: `runtime-catalog.${digest}.backup.json`, digest };
}

async function ensureRelease(repository, tag) {
  try {
    return await github(`/repos/${repository}/releases/tags/${encodeURIComponent(tag)}`);
  } catch (error) {
    if (!String(error.message).endsWith("HTTP 404")) throw error;
    return github(`/repos/${repository}/releases`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ tag_name: tag, name: "SimulationCraft runtime catalog", body: "Machine-readable Ed25519-signed SimulationCraft runtime metadata. This release does not contain or redistribute SimulationCraft.", draft: false, prerelease: false, make_latest: "false" }),
    });
  }
}

async function renameAsset(repository, assetId, name) {
  return github(`/repos/${repository}/releases/assets/${assetId}`, { method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ name }) });
}

export async function promoteStagedAsset({ canonical, staged, names, rename, remove }) {
  let oldRenamed = false;
  try {
    if (canonical) {
      await rename(canonical.id, names.backup);
      oldRenamed = true;
    }
    await rename(staged.id, names.canonical);
  } catch (error) {
    if (oldRenamed) {
      try {
        await rename(canonical.id, names.canonical);
      } catch (rollbackError) {
        throw new AggregateError([error, rollbackError], "catalog promotion and rollback both failed");
      }
    }
    throw error;
  }
  if (canonical) {
    try {
      await remove(canonical.id);
    } catch (error) {
      process.stderr.write(`warning: promoted catalog is active but backup cleanup failed: ${error instanceof Error ? error.message : String(error)}\n`);
    }
  }
}

export async function publishCatalog({ catalog, repository, tag }) {
  const bytes = await readFile(catalog);
  if (bytes.length === 0 || bytes.length > 1024 * 1024) fail("catalog is outside the 1 MiB publish boundary");
  const names = promotionNames(bytes);
  const release = await ensureRelease(repository, tag);
  const assets = await github(`/repos/${repository}/releases/${release.id}/assets?per_page=100`);
  if (assets.some((asset) => asset.name === names.staged || asset.name === names.backup)) fail("staging or backup asset already exists");
  const uploadUrl = new URL(release.upload_url.replace("{?name,label}", ""));
  uploadUrl.searchParams.set("name", names.staged);
  const uploadResponse = await fetch(uploadUrl, {
    method: "POST",
    headers: { Accept: "application/vnd.github+json", Authorization: `Bearer ${process.env.GH_TOKEN}`, "Content-Type": "application/json", "Content-Length": String(bytes.length), "User-Agent": "SimShredder-runtime-catalog-publisher" },
    body: bytes,
  });
  if (!uploadResponse.ok) fail(`GitHub asset upload returned HTTP ${uploadResponse.status}`);
  const staged = await uploadResponse.json();
  const downloadResponse = await fetch(staged.browser_download_url, { headers: { Authorization: `Bearer ${process.env.GH_TOKEN}` }, redirect: "follow" });
  if (!downloadResponse.ok) fail(`staged catalog download returned HTTP ${downloadResponse.status}`);
  const downloaded = Buffer.from(await downloadResponse.arrayBuffer());
  if (!downloaded.equals(bytes)) fail("staged catalog bytes differ after upload");
  const canonical = assets.find((asset) => asset.name === names.canonical);
  await promoteStagedAsset({
    canonical,
    staged,
    names,
    rename: (assetId, name) => renameAsset(repository, assetId, name),
    remove: (assetId) => github(`/repos/${repository}/releases/assets/${assetId}`, { method: "DELETE" }),
  });
  return names;
}

export async function main(argv = process.argv.slice(2)) {
  const options = argumentsFrom(argv);
  const names = await publishCatalog(options);
  process.stdout.write(`published byte-exact ${basename(options.catalog)} as ${names.canonical} (${names.digest})\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
