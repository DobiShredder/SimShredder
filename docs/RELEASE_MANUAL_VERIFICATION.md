# Manual release verification

This gate covers behavior that component tests and a privileged CI runner cannot prove: installing the exact release package from a genuinely standard account, the complete bilingual user flow, keyboard-only operation, and the operating system screen reader. Do not fill evidence from a debug build, an administrator account, or recollection of an earlier package.

## Required environments

| Gate | Environment | Assistive technology |
|---|---|---|
| `macos-aarch64-26-standard` | Apple Silicon, macOS 26 or newer, newly created standard account | VoiceOver |
| `windows-x64-minimum-standard` | x64 system at the current minimum supported by official SimC; for the pinned `1210-01` contract this is Windows 10 21H2 build 19044 | Narrator |

Use the exact DMG and NSIS installer that will be attached to the GitHub prerelease. Record their SHA-256 values before opening them. The account must not belong to `admin`/`Administrators`, and the SimShredder application-data directory must not exist at the start.

The macOS package smoke can first be checked from the standard account with:

```bash
tooling/release/verify-macos-clean-user.sh /absolute/path/to/SimShredder.dmg /absolute/path/to/macos-clean-user-evidence.json
```

That command verifies package ownership, the default user-owned data root, launch, and cleanup only. It refuses pre-existing SimShredder data and removes only data created during that smoke run, so the following interactive checks can still begin from a clean account. The following interactive checks are still required.

## Interactive checklist for each OS

1. Verify the downloaded package hash against `SHA256SUMS`. Install it without entering administrator credentials or accepting an elevation prompt.
2. If Gatekeeper or SmartScreen warns about the unsigned package, use only the documented per-application path in `UNSIGNED_INSTALLATION.md`. Never disable an operating-system security feature globally. A managed-policy block is a failed release gate.
3. Start with no SimShredder data directory. In English, let the app download and install the production-catalog SimC build, then confirm the displayed version and revision.
4. Import a Retail Live profile, inspect the generated `.simc`, run Quick Sim, open the result, and export the verified artifacts. Confirm that the exported input matches the GUI selections.
5. Run Top Gear with owned gear and at least one virtual gem, enchant, and upgrade plus a currency reserve. Confirm preview counts, complete all stages, inspect the result and enhancement order, and export the verified artifacts.
6. Switch to Korean and repeat the import/review, Quick Sim result, Top Gear result, Settings, history, error, and export paths. If the production catalog exposes a newer tested SimC candidate, also choose update `Later` and confirm the active runtime is unchanged. No untranslated key or clipped critical control is allowed.
7. Without using a pointer, traverse the complete flow with Tab and Shift+Tab; activate controls with Enter or Space; operate selects, dialogs, tables, and result actions; and confirm visible focus, logical order, skip navigation, and no keyboard trap.
8. Enable VoiceOver on macOS or Narrator on Windows. Confirm meaningful names, roles, values, progress announcements, error announcements, dialog focus, and result-table/chart summaries across the same Quick Sim and Top Gear flow.
9. Set the OS/application text scale to 200%. Repeat the main navigation, review, running, and result screens and confirm there is no hidden required action or horizontal page overflow.
10. Close the app, relaunch it, and confirm history and completed artifacts recover. Uninstall the application without elevation; then follow `PRIVACY.md` to remove retained per-user data and verify deletion.

Any crash, inaccessible required action, mismatched generated input, failed export, missing translation, missing production SimC catalog, or workaround outside the documented unsigned-install path fails the gate.

## Evidence contract

Create one JSON document containing exactly the two platform runs. Values below in angle brackets must be replaced; the verifier rejects placeholder text and any false or missing required check.

```json
{
  "schema": 1,
  "commit": "<40-character-release-commit>",
  "release_tag": "v0.1.0",
  "artifacts": {
    "macos_aarch64_dmg_sha256": "<64-character-sha256>",
    "windows_x64_setup_sha256": "<64-character-sha256>"
  },
  "runs": [
    {
      "gate": "macos-aarch64-26-standard",
      "architecture": "aarch64",
      "account_kind": "standard",
      "admin_member": false,
      "os_version": "26.0",
      "screen_reader": "VoiceOver",
      "game_channel": "retail-live",
      "simc_version": "1210-01",
      "simc_revision": "<revision>",
      "observed_at": "<RFC-3339-time>",
      "package_sha256": "<DMG-sha256>",
      "security_prompt_outcome": "opened-with-documented-exception",
      "checks": {
        "checksum_verified": true,
        "clean_app_data": true,
        "no_elevation": true,
        "installed": true,
        "app_launch": true,
        "simc_auto_install": true,
        "quick_sim_en": true,
        "quick_sim_ko": true,
        "top_gear_en": true,
        "top_gear_ko": true,
        "artifact_export": true,
        "keyboard_only": true,
        "screen_reader": true,
        "scale_200_percent": true,
        "uninstalled": true
      }
    },
    {
      "gate": "windows-x64-minimum-standard",
      "architecture": "x86_64",
      "account_kind": "standard",
      "admin_member": false,
      "os_version": "Windows 10 21H2 build 19044",
      "screen_reader": "Narrator",
      "game_channel": "retail-live",
      "simc_version": "1210-01",
      "simc_revision": "<revision>",
      "observed_at": "<RFC-3339-time>",
      "package_sha256": "<NSIS-sha256>",
      "security_prompt_outcome": "not-shown",
      "checks": {
        "checksum_verified": true,
        "clean_app_data": true,
        "no_elevation": true,
        "installed": true,
        "app_launch": true,
        "simc_auto_install": true,
        "quick_sim_en": true,
        "quick_sim_ko": true,
        "top_gear_en": true,
        "top_gear_ko": true,
        "artifact_export": true,
        "keyboard_only": true,
        "screen_reader": true,
        "scale_200_percent": true,
        "uninstalled": true
      }
    }
  ]
}
```

Validate the completed document against the exact release inputs:

```bash
node tooling/release/verify-manual-release-evidence.mjs \
  manual-release-evidence.json \
  <release-commit> \
  <release-tag> \
  <macOS-DMG-SHA256> \
  <Windows-setup-SHA256>
```

The verified document belongs with the internal release evidence and its digest belongs in release provenance. Do not record a personal name, account name, home path, device serial number, or other unnecessary identifier.

---

# 수동 release 검증

이 gate는 자동 test나 관리자 runner가 증명할 수 없는 실제 표준 사용자 설치, 한·영 전체 흐름, keyboard-only 조작과 OS screen reader를 검증합니다. Debug build, 관리자 계정 또는 이전 package의 기억을 근거로 evidence를 작성하면 안 됩니다.

Apple Silicon macOS 26 이상의 새 표준 계정에서는 VoiceOver를, 공식 SimC `1210-01`의 최소 지원 Windows인 x64 Windows 10 21H2 build 19044 표준 계정에서는 Narrator를 사용합니다. 실제 prerelease에 첨부할 DMG와 NSIS의 SHA-256을 먼저 기록하고 앱 데이터가 없는 상태에서 시작합니다.

각 OS에서 위 영문 checklist의 열 단계를 모두 수행합니다. 핵심은 관리자 암호나 elevation 없이 설치·제거하고, production catalog를 통한 SimC 자동 설치, Retail Live Quick Sim과 가상 보석·마법부여·업그레이드·화폐 reserve를 포함한 Top Gear, 양 언어의 결과와 export, keyboard-only, VoiceOver/Narrator, 200% scale, 재실행 복구를 같은 release package에서 확인하는 것입니다.

Unsigned 경고는 `UNSIGNED_INSTALLATION.md`의 앱별 정상 절차만 사용합니다. 보안 기능 전체 비활성화, 관리 정책 우회, 누락된 production catalog, crash, 입력 불일치, 필수 control 접근 불가, 미번역 key 또는 export 실패가 있으면 gate 실패입니다. 위 JSON의 placeholder를 실제 관찰값으로 바꾸고 같은 verifier 명령으로 commit과 두 artifact hash에 결합합니다. 개인 이름, 계정명, home path, 장치 serial 같은 불필요한 개인정보는 evidence에 기록하지 않습니다.
