import assert from "node:assert/strict";
import test from "node:test";
import { verifyManualReleaseEvidence } from "./verify-manual-release-evidence.mjs";

const commit = "a".repeat(40);
const tag = "v0.1.0";
const macosSha256 = "b".repeat(64);
const windowsSha256 = "c".repeat(64);
const checks = {
  checksum_verified: true,
  clean_app_data: true,
  no_elevation: true,
  installed: true,
  app_launch: true,
  simc_auto_install: true,
  quick_sim_en: true,
  quick_sim_ko: true,
  top_gear_en: true,
  top_gear_ko: true,
  artifact_export: true,
  keyboard_only: true,
  screen_reader: true,
  scale_200_percent: true,
  uninstalled: true,
};

function validEvidence() {
  return {
    schema: 1,
    commit,
    release_tag: tag,
    artifacts: {
      macos_aarch64_dmg_sha256: macosSha256,
      windows_x64_setup_sha256: windowsSha256,
    },
    runs: [
      {
        gate: "macos-aarch64-26-standard",
        architecture: "aarch64",
        account_kind: "standard",
        admin_member: false,
        os_version: "26.0",
        screen_reader: "VoiceOver",
        game_channel: "retail-live",
        simc_version: "1210-01",
        simc_revision: "02b39ce",
        observed_at: "2026-08-26T12:00:00Z",
        package_sha256: macosSha256,
        security_prompt_outcome: "opened-with-documented-exception",
        checks: { ...checks },
      },
      {
        gate: "windows-x64-minimum-standard",
        architecture: "x86_64",
        account_kind: "standard",
        admin_member: false,
        os_version: "Windows 10 21H2 build 19044",
        screen_reader: "Narrator",
        game_channel: "retail-live",
        simc_version: "1210-01",
        simc_revision: "02b39ce",
        observed_at: "2026-08-26T12:00:00Z",
        package_sha256: windowsSha256,
        security_prompt_outcome: "not-shown",
        checks: { ...checks },
      },
    ],
  };
}

test("accepts commit-bound complete evidence for both release platforms", () => {
  const result = verifyManualReleaseEvidence(validEvidence(), { commit, tag, macosSha256, windowsSha256 });
  assert.deepEqual(result.gates, ["macos-aarch64-26-standard", "windows-x64-minimum-standard"]);
});

test("rejects a missing manual accessibility result", () => {
  const evidence = validEvidence();
  evidence.runs[0].checks.screen_reader = false;
  assert.throws(
    () => verifyManualReleaseEvidence(evidence, { commit, tag, macosSha256, windowsSha256 }),
    /screen_reader must be true/,
  );
});

test("rejects release evidence bound to a different artifact", () => {
  const evidence = validEvidence();
  evidence.runs[1].package_sha256 = "d".repeat(64);
  assert.throws(
    () => verifyManualReleaseEvidence(evidence, { commit, tag, macosSha256, windowsSha256 }),
    /package hash is invalid/,
  );
});

test("rejects admin-account and placeholder claims", () => {
  const adminEvidence = validEvidence();
  adminEvidence.runs[1].admin_member = true;
  assert.throws(
    () => verifyManualReleaseEvidence(adminEvidence, { commit, tag, macosSha256, windowsSha256 }),
    /must not be an administrator/,
  );

  const placeholderEvidence = validEvidence();
  placeholderEvidence.runs[0].simc_revision = "TODO";
  assert.throws(
    () => verifyManualReleaseEvidence(placeholderEvidence, { commit, tag, macosSha256, windowsSha256 }),
    /placeholder text/,
  );
});

test("rejects evidence for a different release tag", () => {
  const evidence = validEvidence();
  assert.throws(
    () => verifyManualReleaseEvidence(evidence, { commit, tag: "v0.1.1", macosSha256, windowsSha256 }),
    /release_tag does not match/,
  );
});

test("rejects unexpected identifying fields", () => {
  const evidence = validEvidence();
  evidence.runs[0].account_name = "real-user";
  assert.throws(
    () => verifyManualReleaseEvidence(evidence, { commit, tag, macosSha256, windowsSha256 }),
    /unexpected fields: account_name/,
  );
});
