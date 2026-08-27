import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import assert from "node:assert/strict";
import test from "node:test";
import { verifyUnsignedCandidate } from "./verify-unsigned-candidate.mjs";

const commit = "a".repeat(40);
const tag = "v0.1.0";

async function fixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), "simshredder-candidate-"));
  const macDirectory = path.join(root, "unsigned-release-macos-aarch64", "release-assets");
  const windowsDirectory = path.join(root, "unsigned-release-windows-x64", "release-assets");
  const evidenceDirectory = path.join(root, "unsigned-release-windows-x64", "release-evidence");
  await Promise.all([mkdir(macDirectory, { recursive: true }), mkdir(windowsDirectory, { recursive: true }), mkdir(evidenceDirectory, { recursive: true })]);
  const macName = `SimShredder-${tag}-aarch64.dmg`;
  const windowsName = `SimShredder-${tag}-windows-x64-setup.exe`;
  const macBytes = Buffer.from("mac package");
  const windowsBytes = Buffer.from("windows package");
  await writeFile(path.join(macDirectory, macName), macBytes);
  await writeFile(path.join(windowsDirectory, windowsName), windowsBytes);
  const macSha = createHash("sha256").update(macBytes).digest("hex");
  const windowsSha = createHash("sha256").update(windowsBytes).digest("hex");
  await writeFile(path.join(root, "unsigned-release-macos-aarch64", "macos-aarch64-provenance.json"), JSON.stringify({
    schema: 1,
    platform: "macos-aarch64",
    commit,
    artifacts: [{ name: macName, size: macBytes.length, sha256: macSha }],
  }));
  await writeFile(path.join(root, "unsigned-release-windows-x64", "windows-x64-provenance.json"), JSON.stringify({
    schema: 1,
    platform: "windows-x64",
    commit,
    artifacts: [{ name: windowsName, size: windowsBytes.length, sha256: windowsSha }],
  }));
  await writeFile(path.join(evidenceDirectory, "windows-clean-user-evidence.json"), JSON.stringify({
    schema: 1,
    platform: "windows-x64",
    commit,
    users_member: true,
    administrators_member: false,
    token_integrity: "medium",
    installer_sha256: windowsSha,
    authenticode_status: "NotSigned",
    install_root: "%USERPROFILE%\\AppData\\Local\\Programs\\SimShredder",
    launch_seconds: 5,
    install_exit_code: 0,
    uninstall_exit_code: 0,
  }));
  return { root, windowsSha };
}

test("accepts the exact two-platform candidate and clean-user evidence", async (context) => {
  const value = await fixture();
  context.after(() => rm(value.root, { recursive: true, force: true }));
  const result = await verifyUnsignedCandidate(value.root, { commit, tag });
  assert.equal(result.artifacts["windows-x64"].sha256, value.windowsSha);
});

test("rejects a modified installer", async (context) => {
  const value = await fixture();
  context.after(() => rm(value.root, { recursive: true, force: true }));
  await writeFile(path.join(value.root, "unsigned-release-windows-x64", "release-assets", `SimShredder-${tag}-windows-x64-setup.exe`), "tampered");
  await assert.rejects(() => verifyUnsignedCandidate(value.root, { commit, tag }), /size is invalid/);
});

test("rejects an unexpected candidate file", async (context) => {
  const value = await fixture();
  context.after(() => rm(value.root, { recursive: true, force: true }));
  await writeFile(path.join(value.root, "extra.txt"), "unexpected");
  await assert.rejects(() => verifyUnsignedCandidate(value.root, { commit, tag }), /unexpected files/);
});

test("rejects identifying fields in Windows clean-user evidence", async (context) => {
  const value = await fixture();
  context.after(() => rm(value.root, { recursive: true, force: true }));
  const evidencePath = path.join(value.root, "unsigned-release-windows-x64", "release-evidence", "windows-clean-user-evidence.json");
  const evidence = JSON.parse(await readFile(evidencePath, "utf8"));
  evidence.account_sid = "S-1-5-21-111-222-333-1001";
  await writeFile(evidencePath, JSON.stringify(evidence));
  await assert.rejects(() => verifyUnsignedCandidate(value.root, { commit, tag }), /unexpected fields: account_sid/);
});
