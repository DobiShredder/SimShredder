import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const verifier = path.join(path.dirname(fileURLToPath(import.meta.url)), "verify-windows-pe.mjs");

function pe({ machine = 0x8664, magic = 0x20b, subsystem = 2 } = {}) {
  const bytes = Buffer.alloc(512);
  bytes.writeUInt16LE(0x5a4d, 0);
  bytes.writeUInt32LE(0x80, 0x3c);
  bytes.writeUInt32LE(0x00004550, 0x80);
  bytes.writeUInt16LE(machine, 0x84);
  bytes.writeUInt16LE(240, 0x94);
  bytes.writeUInt16LE(magic, 0x98);
  bytes.writeUInt16LE(subsystem, 0x98 + 68);
  return bytes;
}

async function verify(context, bytes) {
  const root = await mkdtemp(path.join(os.tmpdir(), "simshredder-pe-"));
  context.after(() => rm(root, { recursive: true }));
  const application = path.join(root, "application.exe");
  await writeFile(application, bytes);
  return spawnSync(process.execPath, [verifier, application], { encoding: "utf8" });
}

test("accepts a Windows x64 PE32+ GUI binary", async (context) => {
  const result = await verify(context, pe());
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Windows x64 PE32\+ GUI/);
});

test("rejects a console subsystem binary", async (context) => {
  const result = await verify(context, pe({ subsystem: 3 }));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Windows GUI subsystem/);
});

test("rejects a non-x64 binary", async (context) => {
  const result = await verify(context, pe({ machine: 0x14c, magic: 0x10b }));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /not Windows x64/);
});
