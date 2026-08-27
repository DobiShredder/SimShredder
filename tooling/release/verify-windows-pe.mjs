import { readFile } from "node:fs/promises";

const [path] = process.argv.slice(2);
if (!path) throw new Error("usage: node verify-windows-pe.mjs <application.exe>");

const bytes = await readFile(path);
function requireRange(offset, length, label) {
  if (!Number.isSafeInteger(offset) || offset < 0 || offset + length > bytes.length) {
    throw new Error(`${label} is outside the PE file`);
  }
}

requireRange(0, 0x40, "DOS header");
if (bytes.readUInt16LE(0) !== 0x5a4d) throw new Error("application does not have an MZ header");
const peOffset = bytes.readUInt32LE(0x3c);
requireRange(peOffset, 24, "PE header");
if (bytes.readUInt32LE(peOffset) !== 0x00004550) throw new Error("application does not have a PE signature");
if (bytes.readUInt16LE(peOffset + 4) !== 0x8664) throw new Error("application is not Windows x64");

const optionalSize = bytes.readUInt16LE(peOffset + 20);
const optionalOffset = peOffset + 24;
requireRange(optionalOffset, optionalSize, "optional header");
if (optionalSize < 70 || bytes.readUInt16LE(optionalOffset) !== 0x20b) {
  throw new Error("application is not PE32+");
}
if (bytes.readUInt16LE(optionalOffset + 68) !== 2) {
  throw new Error("application is not a Windows GUI subsystem binary");
}

process.stdout.write("verified Windows x64 PE32+ GUI subsystem\n");
