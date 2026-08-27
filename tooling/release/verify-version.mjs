import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";

const tag = process.argv[2];
if (!tag || !/^v0\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/.test(tag)) {
  throw new Error("unsigned prerelease tag must be a valid v0.x semantic version");
}

const expected = tag.slice(1);
const packageJson = JSON.parse(
  await readFile("apps/desktop/package.json", "utf8"),
);
const tauriConfig = JSON.parse(
  await readFile("apps/desktop/src-tauri/tauri.conf.json", "utf8"),
);
const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
    encoding: "utf8",
  }),
);
const desktopPackage = metadata.packages.find(
  (entry) => entry.name === "simshredder-desktop",
);

const versions = new Map([
  ["tag", expected],
  ["apps/desktop/package.json", packageJson.version],
  ["apps/desktop/src-tauri/tauri.conf.json", tauriConfig.version],
  ["apps/desktop/src-tauri/Cargo.toml", desktopPackage?.version],
]);

for (const [source, version] of versions) {
  if (version !== expected) {
    throw new Error(`${source} version ${String(version)} does not match ${expected}`);
  }
}

process.stdout.write(`verified SimShredder ${expected}\n`);
