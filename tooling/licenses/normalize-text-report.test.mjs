import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const normalizer = path.join(path.dirname(fileURLToPath(import.meta.url)), "normalize-text-report.mjs");

test("normalizes mixed report line endings to LF", async (context) => {
  const root = await mkdtemp(path.join(os.tmpdir(), "simshredder-license-report-"));
  context.after(() => rm(root, { recursive: true }));
  const report = path.join(root, "report.md");
  await writeFile(report, "first\r\nsecond\rthird\n", "utf8");

  execFileSync(process.execPath, [normalizer, report]);

  assert.equal(await readFile(report, "utf8"), "first\nsecond\nthird\n");
});
