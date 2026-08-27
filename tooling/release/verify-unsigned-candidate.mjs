import { createHash } from "node:crypto";
import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const COMMIT = /^[0-9a-f]{40}$/;
const TAG = /^v0\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?$/;

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function assertExactKeys(value, expectedKeys, field) {
  const expected = new Set(expectedKeys);
  const missing = expectedKeys.filter((key) => !Object.hasOwn(value, key));
  const unexpected = Object.keys(value).filter((key) => !expected.has(key));
  assert(missing.length === 0, `${field} is missing fields: ${missing.join(", ")}`);
  assert(unexpected.length === 0, `${field} contains unexpected fields: ${unexpected.join(", ")}`);
}

async function filesBelow(root, relative = "") {
  const directory = path.join(root, relative);
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const child = path.join(relative, entry.name);
    if (entry.isSymbolicLink()) throw new Error(`candidate contains a symbolic link: ${child}`);
    if (entry.isDirectory()) files.push(...await filesBelow(root, child));
    else if (entry.isFile()) files.push(child);
    else throw new Error(`candidate contains an unsupported filesystem entry: ${child}`);
  }
  return files;
}

async function sha256(file) {
  return createHash("sha256").update(await readFile(file)).digest("hex");
}

function exactlyOne(files, predicate, label) {
  const matches = files.filter(predicate);
  assert(matches.length === 1, `expected exactly one ${label}, found ${matches.length}`);
  return matches[0];
}

export async function verifyUnsignedCandidate(root, expected) {
  assert(COMMIT.test(expected.commit), "expected commit must be a lowercase full Git SHA");
  assert(TAG.test(expected.tag), "expected tag must be a v0.x semantic version");
  const metadata = await stat(root);
  assert(metadata.isDirectory(), "candidate root must be a directory");
  const files = (await filesBelow(root)).sort();
  const expectedDmgName = `SimShredder-${expected.tag}-aarch64.dmg`;
  const expectedSetupName = `SimShredder-${expected.tag}-windows-x64-setup.exe`;
  const dmg = exactlyOne(files, (file) => path.basename(file) === expectedDmgName, expectedDmgName);
  const setup = exactlyOne(files, (file) => path.basename(file) === expectedSetupName, expectedSetupName);
  const macProvenanceFile = exactlyOne(files, (file) => path.basename(file) === "macos-aarch64-provenance.json", "macOS provenance");
  const windowsProvenanceFile = exactlyOne(files, (file) => path.basename(file) === "windows-x64-provenance.json", "Windows provenance");
  const windowsEvidenceFile = exactlyOne(files, (file) => path.basename(file) === "windows-clean-user-evidence.json", "Windows clean-user evidence");

  const allowedNames = new Set([
    expectedDmgName,
    expectedSetupName,
    "macos-aarch64-provenance.json",
    "windows-x64-provenance.json",
    "windows-clean-user-evidence.json",
  ]);
  const unexpected = files.filter((file) => !allowedNames.has(path.basename(file)));
  assert(unexpected.length === 0, `candidate contains unexpected files: ${unexpected.join(", ")}`);

  const results = {};
  for (const [platform, provenanceFile, artifact] of [
    ["macos-aarch64", macProvenanceFile, dmg],
    ["windows-x64", windowsProvenanceFile, setup],
  ]) {
    const provenance = JSON.parse(await readFile(path.join(root, provenanceFile), "utf8"));
    assert(provenance.schema === 1, `${platform} provenance schema is invalid`);
    assert(provenance.platform === platform, `${platform} provenance platform is invalid`);
    assert(provenance.commit === expected.commit, `${platform} provenance commit does not match the tag`);
    assert(Array.isArray(provenance.artifacts) && provenance.artifacts.length === 1, `${platform} provenance must describe exactly one installer`);
    const actualSize = (await stat(path.join(root, artifact))).size;
    const actualSha256 = await sha256(path.join(root, artifact));
    assert(provenance.artifacts[0].name === path.basename(artifact), `${platform} provenance artifact name is invalid`);
    assert(provenance.artifacts[0].size === actualSize, `${platform} provenance artifact size is invalid`);
    assert(provenance.artifacts[0].sha256 === actualSha256, `${platform} provenance artifact SHA-256 is invalid`);
    results[platform] = { file: artifact, size: actualSize, sha256: actualSha256 };
  }

  const windowsEvidence = JSON.parse(await readFile(path.join(root, windowsEvidenceFile), "utf8"));
  assert(windowsEvidence && typeof windowsEvidence === "object" && !Array.isArray(windowsEvidence), "Windows clean-user evidence must be an object");
  assertExactKeys(windowsEvidence, [
    "schema",
    "platform",
    "commit",
    "users_member",
    "administrators_member",
    "token_integrity",
    "installer_sha256",
    "authenticode_status",
    "install_root",
    "launch_seconds",
    "install_exit_code",
    "uninstall_exit_code",
  ], "Windows clean-user evidence");
  assert(windowsEvidence.schema === 1 && windowsEvidence.platform === "windows-x64", "Windows clean-user evidence identity is invalid");
  assert(windowsEvidence.commit === expected.commit, "Windows clean-user evidence commit does not match the tag");
  assert(windowsEvidence.users_member === true, "Windows clean-user evidence did not use the local Users group");
  assert(windowsEvidence.administrators_member === false, "Windows clean-user evidence used an administrator");
  assert(windowsEvidence.token_integrity === "medium", "Windows clean-user evidence did not use a medium-integrity token");
  assert(windowsEvidence.authenticode_status === "NotSigned", "Windows candidate must be unsigned");
  assert(windowsEvidence.install_root === "%USERPROFILE%\\AppData\\Local\\Programs\\SimShredder", "Windows clean-user install root is invalid");
  assert(Number.isInteger(windowsEvidence.launch_seconds) && windowsEvidence.launch_seconds >= 1 && windowsEvidence.launch_seconds <= 60, "Windows clean-user launch duration is invalid");
  assert(windowsEvidence.install_exit_code === 0 && windowsEvidence.uninstall_exit_code === 0, "Windows clean-user install or uninstall failed");
  assert(windowsEvidence.installer_sha256 === results["windows-x64"].sha256, "Windows clean-user evidence is bound to a different installer");

  return { files, artifacts: results };
}

async function main() {
  const [root, commit, tag] = process.argv.slice(2);
  if (!root || !commit || !tag) throw new Error("usage: node verify-unsigned-candidate.mjs <candidate-root> <commit> <tag>");
  const result = await verifyUnsignedCandidate(root, { commit, tag });
  process.stdout.write(`${JSON.stringify(result.artifacts, null, 2)}\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  await main();
}
