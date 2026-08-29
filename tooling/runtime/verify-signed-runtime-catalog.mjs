import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { verifyCatalogSignatures } from "./refresh-runtime-catalog.mjs";
import { validateManifest } from "./verify-catalog-artifacts.mjs";

function fail(message) { throw new Error(message); }

function argumentsFrom(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index];
    const value = argv[index + 1];
    if (!name?.startsWith("--") || value === undefined) fail(`invalid argument near ${String(name)}`);
    options[name.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = value;
  }
  if (!options.catalog || !options.trustRoots || !options.now) fail("--catalog, --trust-roots and --now are required");
  return options;
}

async function hashFile(filename, maximum) {
  const metadata = await stat(filename);
  if (!metadata.isFile() || metadata.size <= 0 || metadata.size > maximum) fail(`artifact is outside its signed bound: ${path.basename(filename)}`);
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(filename)) hash.update(chunk);
  return { size: metadata.size, sha256: hash.digest("hex") };
}

export async function verifySignedCatalogFile(options) {
  const catalog = JSON.parse(await readFile(options.catalog, "utf8"));
  const roots = JSON.parse(await readFile(options.trustRoots, "utf8"));
  verifyCatalogSignatures(catalog, roots, Number(options.now));
  const targets = new Set();
  for (const rawManifest of catalog.payload.manifests) {
    const manifest = validateManifest(rawManifest);
    const target = `${manifest.platform}/${manifest.architecture}`;
    if (targets.has(target)) fail(`catalog repeats target ${target}`);
    targets.add(target);
    if (options.artifactDirectory) {
      const actual = await hashFile(path.join(options.artifactDirectory, manifest.filename), manifest.size);
      if (actual.size !== manifest.size || actual.sha256 !== manifest.sha256) fail(`local artifact differs from signed manifest: ${manifest.filename}`);
    }
  }
  for (const target of ["macos/aarch64", "windows/x86_64"]) {
    if (!targets.has(target)) fail(`catalog is missing required target ${target}`);
  }
  return catalog;
}

export async function main(argv = process.argv.slice(2)) {
  const options = argumentsFrom(argv);
  const catalog = await verifySignedCatalogFile(options);
  process.stdout.write(`verified signed runtime catalog sequence ${catalog.payload.sequence}\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
