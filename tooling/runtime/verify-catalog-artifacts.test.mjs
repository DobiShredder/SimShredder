import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { loadCatalog, validateManifest, verifyAndDownload, verifyAvailability } from "./verify-catalog-artifacts.mjs";

const bytes = Buffer.from("verified SimulationCraft fixture");

function manifest(url, overrides = {}) {
  return {
    schema_version: 1,
    simc_version: "1210-01",
    game_channel: "live",
    platform: "windows",
    architecture: "x86_64",
    build: "fixture",
    filename: "simc-fixture.7z",
    url,
    size: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    ...overrides,
  };
}

async function server(handler) {
  const instance = http.createServer(handler);
  await new Promise((resolve) => instance.listen(0, "127.0.0.1", resolve));
  return instance;
}

test("rejects non-official URLs and unsafe filenames before network access", () => {
  assert.throws(() => validateManifest(manifest("https://example.com/nightly/simc-fixture.7z")), /official exact-file boundary/);
  assert.throws(() => validateManifest(manifest("http://downloads.simulationcraft.org/nightly/..%2Fsecret", { filename: "../secret" })), /unsafe artifact filename/);
});

test("rejects duplicate platform targets in the signed catalog shape", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "simshredder-catalog-test-"));
  const catalog = path.join(directory, "catalog.json");
  const entry = manifest("http://downloads.simulationcraft.org/nightly/simc-fixture.7z");
  await writeFile(catalog, JSON.stringify({ payload: { manifests: [entry, entry] } }));
  await assert.rejects(loadCatalog(catalog), /repeats target/);
  await rm(directory, { recursive: true });
});

test("availability requires HTTP 200 and the signed content length", async (context) => {
  const instance = await server((request, response) => {
    response.writeHead(200, { "Content-Length": bytes.length });
    response.end(request.method === "HEAD" ? undefined : bytes);
  });
  context.after(() => instance.close());
  const port = instance.address().port;
  const entry = manifest(`http://127.0.0.1:${port}/simc-fixture.7z`);
  await verifyAvailability({ ...entry, url: entry.url });
  await assert.rejects(verifyAvailability({ ...entry, size: bytes.length + 1 }), /size mismatch/);
});

test("full verification writes only a size-and-hash verified artifact", async (context) => {
  const instance = await server((_request, response) => {
    response.writeHead(200, { "Content-Length": bytes.length });
    response.end(bytes);
  });
  context.after(() => instance.close());
  const directory = await mkdtemp(path.join(os.tmpdir(), "simshredder-download-test-"));
  context.after(() => rm(directory, { recursive: true }));
  const port = instance.address().port;
  const entry = manifest(`http://127.0.0.1:${port}/simc-fixture.7z`);
  const result = await verifyAndDownload(entry, directory);
  assert.deepEqual(await readFile(result.path), bytes);
  await assert.rejects(verifyAndDownload({ ...entry, sha256: "0".repeat(64) }, directory), /SHA-256 mismatch/);
});
