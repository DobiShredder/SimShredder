import { readFile, writeFile } from "node:fs/promises";

const paths = process.argv.slice(2);
if (paths.length === 0) {
  throw new Error("usage: node normalize-text-report.mjs <report> [...report]");
}

for (const path of paths) {
  const source = await readFile(path, "utf8");
  await writeFile(path, source.replace(/\r\n?/g, "\n"), "utf8");
}
