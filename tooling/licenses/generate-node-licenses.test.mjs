import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import test from "node:test";

const execFileAsync = promisify(execFile);
const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const generator = join(repositoryRoot, "tooling/licenses/generate-node-licenses.mjs");

test("normalizes host-selected TypeScript compiler packages", async () => {
  const fixtureRoot = await mkdtemp(join(tmpdir(), "simshredder-node-licenses-"));
  try {
    const outputs = [];
    for (const platformName of ["@typescript/typescript-darwin-arm64", "@typescript/typescript-linux-x64"]) {
      const packageRoot = join(fixtureRoot, platformName.replaceAll("/", "-"));
      await mkdir(packageRoot);
      await writeFile(
        join(packageRoot, "package.json"),
        JSON.stringify({ name: platformName, version: "7.0.2", license: "Apache-2.0" }),
      );
      await writeFile(join(packageRoot, "LICENSE"), "same native compiler license\n");

      const input = join(packageRoot, "licenses.json");
      const output = join(packageRoot, "report.md");
      await writeFile(input, JSON.stringify({ "Apache-2.0": [{ license: "Apache-2.0", paths: [packageRoot] }] }));
      await execFileAsync(process.execPath, [generator, input, output], { cwd: repositoryRoot });
      outputs.push(await readFile(output, "utf8"));
    }

    assert.equal(outputs[0], outputs[1]);
    assert.match(outputs[0], /## @typescript\/typescript-platform@7\.0\.2/);
    assert.doesNotMatch(outputs[0], /darwin|linux/);
  } finally {
    await rm(fixtureRoot, { recursive: true, force: true });
  }
});
