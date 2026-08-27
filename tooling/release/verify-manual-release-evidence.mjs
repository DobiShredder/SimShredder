import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SHA256 = /^[0-9a-f]{64}$/;
const COMMIT = /^[0-9a-f]{40}$/;
const PRERELEASE_TAG = /^v0\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?$/;
const REQUIRED_CHECKS = [
  "checksum_verified",
  "clean_app_data",
  "no_elevation",
  "installed",
  "app_launch",
  "simc_auto_install",
  "quick_sim_en",
  "quick_sim_ko",
  "top_gear_en",
  "top_gear_ko",
  "artifact_export",
  "keyboard_only",
  "screen_reader",
  "scale_200_percent",
  "uninstalled",
];

const GATES = {
  "macos-aarch64-26-standard": {
    architecture: "aarch64",
    screenReader: "VoiceOver",
    validateVersion(value) {
      const major = Number.parseInt(value.split(".")[0], 10);
      return Number.isInteger(major) && major >= 26;
    },
  },
  "windows-x64-minimum-standard": {
    architecture: "x86_64",
    screenReader: "Narrator",
    validateVersion(value) {
      const match = /^Windows 10 21H2(?:\s|$)/.exec(value);
      return match !== null;
    },
  },
};

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function assertPlainText(value, field) {
  assert(typeof value === "string" && value.trim().length > 0, `${field} must be non-empty text`);
  assert(!/(?:<[^>]+>|\bTODO\b|\bTBD\b|placeholder)/i.test(value), `${field} contains placeholder text`);
}

function assertExactKeys(value, expectedKeys, field) {
  const expected = new Set(expectedKeys);
  const missing = expectedKeys.filter((key) => !Object.hasOwn(value, key));
  const unexpected = Object.keys(value).filter((key) => !expected.has(key));
  assert(missing.length === 0, `${field} is missing fields: ${missing.join(", ")}`);
  assert(unexpected.length === 0, `${field} contains unexpected fields: ${unexpected.join(", ")}`);
}

export function verifyManualReleaseEvidence(evidence, expected) {
  assert(evidence && typeof evidence === "object" && !Array.isArray(evidence), "evidence must be an object");
  assertExactKeys(evidence, ["schema", "commit", "release_tag", "artifacts", "runs"], "evidence");
  assert(evidence.schema === 1, "evidence schema must be 1");
  assert(COMMIT.test(evidence.commit), "evidence commit must be a lowercase full Git SHA");
  assert(evidence.commit === expected.commit, "evidence commit does not match the release commit");
  assert(PRERELEASE_TAG.test(evidence.release_tag), "release_tag must be a v0.x semantic version");
  assert(evidence.release_tag === expected.tag, "evidence release_tag does not match the release tag");
  assert(evidence.artifacts && typeof evidence.artifacts === "object", "artifacts must be an object");
  assertExactKeys(evidence.artifacts, ["macos_aarch64_dmg_sha256", "windows_x64_setup_sha256"], "artifacts");
  assert(SHA256.test(evidence.artifacts.macos_aarch64_dmg_sha256), "macOS artifact SHA-256 is invalid");
  assert(SHA256.test(evidence.artifacts.windows_x64_setup_sha256), "Windows artifact SHA-256 is invalid");
  assert(evidence.artifacts.macos_aarch64_dmg_sha256 === expected.macosSha256, "macOS artifact SHA-256 does not match");
  assert(evidence.artifacts.windows_x64_setup_sha256 === expected.windowsSha256, "Windows artifact SHA-256 does not match");
  assert(Array.isArray(evidence.runs), "runs must be an array");
  assert(evidence.runs.length === Object.keys(GATES).length, "evidence must contain exactly the two required platform runs");

  const seen = new Set();
  for (const run of evidence.runs) {
    assert(run && typeof run === "object" && !Array.isArray(run), "each run must be an object");
    assertExactKeys(run, [
      "gate",
      "architecture",
      "account_kind",
      "admin_member",
      "os_version",
      "screen_reader",
      "game_channel",
      "simc_version",
      "simc_revision",
      "observed_at",
      "package_sha256",
      "security_prompt_outcome",
      "checks",
    ], "run");
    assertPlainText(run.gate, "run.gate");
    const contract = GATES[run.gate];
    assert(contract, `unsupported or unexpected gate: ${run.gate}`);
    assert(!seen.has(run.gate), `duplicate gate: ${run.gate}`);
    seen.add(run.gate);
    assert(run.architecture === contract.architecture, `${run.gate} architecture is invalid`);
    assert(run.account_kind === "standard", `${run.gate} must run from a standard account`);
    assert(run.admin_member === false, `${run.gate} account must not be an administrator`);
    assertPlainText(run.os_version, `${run.gate}.os_version`);
    assert(contract.validateVersion(run.os_version), `${run.gate} OS version is outside the release contract`);
    assert(run.screen_reader === contract.screenReader, `${run.gate} must use ${contract.screenReader}`);
    assert(run.game_channel === "retail-live", `${run.gate} must use Retail Live`);
    assertPlainText(run.simc_version, `${run.gate}.simc_version`);
    assertPlainText(run.simc_revision, `${run.gate}.simc_revision`);
    assertPlainText(run.observed_at, `${run.gate}.observed_at`);
    const observedAt = Date.parse(run.observed_at);
    assert(Number.isFinite(observedAt), `${run.gate}.observed_at must be RFC 3339-compatible`);
    assert(observedAt <= Date.now() + 5 * 60_000, `${run.gate}.observed_at is in the future`);
    assert(run.package_sha256 === (run.gate.startsWith("macos-") ? expected.macosSha256 : expected.windowsSha256), `${run.gate} package hash is invalid`);
    assert(run.checks && typeof run.checks === "object" && !Array.isArray(run.checks), `${run.gate}.checks must be an object`);
    for (const check of REQUIRED_CHECKS) {
      assert(run.checks[check] === true, `${run.gate}.checks.${check} must be true`);
    }
    assertExactKeys(run.checks, REQUIRED_CHECKS, `${run.gate}.checks`);
    assert(["not-shown", "opened-with-documented-exception"].includes(run.security_prompt_outcome), `${run.gate} security prompt outcome is not a successful documented path`);
  }

  return {
    schema: 1,
    commit: evidence.commit,
    releaseTag: evidence.release_tag,
    gates: [...seen].sort(),
  };
}

async function main() {
  const [file, commit, tag, macosSha256, windowsSha256] = process.argv.slice(2);
  if (!file || !commit || !tag || !macosSha256 || !windowsSha256) {
    throw new Error("usage: node verify-manual-release-evidence.mjs <evidence.json> <commit> <tag> <macOS-DMG-SHA256> <Windows-setup-SHA256>");
  }
  assert(COMMIT.test(commit), "expected commit must be a lowercase full Git SHA");
  assert(PRERELEASE_TAG.test(tag), "expected tag must be a v0.x semantic version");
  assert(SHA256.test(macosSha256), "expected macOS SHA-256 is invalid");
  assert(SHA256.test(windowsSha256), "expected Windows SHA-256 is invalid");
  const evidence = JSON.parse(await readFile(file, "utf8"));
  const result = verifyManualReleaseEvidence(evidence, { commit, tag, macosSha256, windowsSha256 });
  process.stdout.write(`verified manual release evidence for ${result.commit} (${result.gates.join(", ")})\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  await main();
}
