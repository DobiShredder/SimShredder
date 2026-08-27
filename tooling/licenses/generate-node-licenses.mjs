import { readFile, readdir, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";

const [inputPath, outputPath] = process.argv.slice(2);
if (!inputPath || !outputPath) {
  throw new Error("usage: node tooling/licenses/generate-node-licenses.mjs <pnpm-json> <output>");
}

const report = JSON.parse(await readFile(inputPath, "utf8"));
const packages = new Map();
for (const entries of Object.values(report)) {
  for (const entry of entries) {
    for (const packagePath of entry.paths) {
      const manifest = JSON.parse(await readFile(join(packagePath, "package.json"), "utf8"));
      packages.set(`${manifest.name}@${manifest.version}`, {
        name: manifest.name,
        version: manifest.version,
        license: manifest.license ?? entry.license,
        path: packagePath,
      });
    }
  }
}

const sections = [
  "# Node.js production third-party licenses",
  "",
  "Generated from the frozen pnpm production dependency graph. Every listed package must ship at least one license, copying, or notice file.",
  "",
];
for (const dependency of [...packages.values()].sort((left, right) =>
  `${left.name}@${left.version}`.localeCompare(`${right.name}@${right.version}`),
)) {
  const candidates = (await readdir(dependency.path, { withFileTypes: true }))
    .filter((entry) => entry.isFile() && /^(licen[cs]e|copying|notice)(\.|-|$)/i.test(entry.name))
    .map((entry) => entry.name)
    .sort();
  if (candidates.length === 0 && dependency.license.includes("Apache-2.0")) {
    candidates.push("@simshredder-apache-2.0");
  }
  if (candidates.length === 0) {
    throw new Error(`${dependency.name}@${dependency.version} has no packaged or approved canonical license text`);
  }
  sections.push(`## ${dependency.name}@${dependency.version}`, "", `Declared license: ${dependency.license}`, "");
  for (const candidate of candidates) {
    const bytes = await readFile(
      candidate === "@simshredder-apache-2.0" ? join(process.cwd(), "LICENSE") : join(dependency.path, candidate),
    );
    if (bytes.length === 0 || bytes.length > 1024 * 1024) {
      throw new Error(`${dependency.name}@${dependency.version}/${candidate} has an unsafe size`);
    }
    const text = bytes.toString("utf8").replaceAll("\0", "").trimEnd();
    const label = candidate === "@simshredder-apache-2.0" ? "Canonical Apache-2.0 text" : basename(candidate);
    sections.push(`### ${label}`, "", ...text.split("\n").map((line) => `    ${line}`), "");
  }
}

await writeFile(outputPath, `${sections.join("\n")}\n`, { flag: "wx" });
