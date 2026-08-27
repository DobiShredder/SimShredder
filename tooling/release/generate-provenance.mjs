import { createHash } from "node:crypto";
import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";

const [platform, commit, assetDirectory, output] = process.argv.slice(2);
if (!platform || !/^[a-z0-9_-]+$/.test(platform)) {
  throw new Error("a safe platform name is required");
}
if (!commit || !/^[0-9a-f]{40}$/.test(commit)) {
  throw new Error("a full lowercase source commit is required");
}
if (!assetDirectory || !output) {
  throw new Error("usage: generate-provenance <platform> <commit> <asset-dir> <output>");
}

const entries = [];
for (const name of (await readdir(assetDirectory)).sort()) {
  const file = path.join(assetDirectory, name);
  const metadata = await stat(file);
  if (!metadata.isFile()) {
    throw new Error(`release asset staging contains a non-file entry: ${name}`);
  }
  const bytes = await readFile(file);
  entries.push({
    name,
    size: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  });
}
if (entries.length !== 1) {
  throw new Error(`expected exactly one ${platform} installer, found ${entries.length}`);
}

await writeFile(
  output,
  `${JSON.stringify({ schema: 1, platform, commit, artifacts: entries }, null, 2)}\n`,
  { flag: "wx" },
);
