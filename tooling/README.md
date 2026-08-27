# Repository tooling

제품 runtime에 포함되지 않는 저장소 유지보수 도구를 역할별로 둡니다.

- `localization/`: 번역 catalog 완전성 검사
- `licenses/`: package manager 출력에서 배포용 license 보고서를 생성하고 runner별 줄바꿈 차이를 LF로 정규화
- `release/`: public tag/version 일치, commit-bound artifact provenance, workflow action·credential 경계와 전체 Git history 검증
- `runtime/`: signed runtime catalog의 공식 URL 경계, availability와 전체 size·SHA-256 검증

Release 검증기 중 `release/verify-windows-clean-user.ps1`은 ephemeral Windows standard account에서 NSIS 설치·GUI launch·제거와 Authenticode 기대 상태를 검증합니다. 공개 evidence에는 임시 account SID를 기록하지 않습니다. `release/verify-macos-clean-user.sh`는 실제 로그인된 macOS standard account에서만 DMG의 per-user copy·기본 `SimShredder` data root·launch를 검사하고 이번 실행이 만든 data를 정리합니다. 기존 data, admin 또는 root account에서는 거부합니다.

Windows verifier는 관리자 runner가 임시 local account와 profile을 만들고 medium-integrity token을 확인한 뒤 둘을 정리하므로 CI에서만 실행합니다. macOS verifier는 별도로 만든 표준 사용자로 실제 로그인한 Terminal에서 다음처럼 실행합니다. 이 smoke는 설치 권한과 5초 launch만 검증하며 Quick Sim·Top Gear와 VoiceOver 수동 검증을 대신하지 않습니다.

```bash
tooling/release/verify-macos-clean-user.sh /absolute/path/to/SimShredder.dmg /absolute/path/to/macos-clean-user-evidence.json
```

`release/verify-manual-release-evidence.mjs`는 `docs/RELEASE_MANUAL_VERIFICATION.md`에 따라 실제 두 OS release package에서 관찰한 clean-user Quick Sim·Top Gear, 한·영, keyboard와 screen reader 결과를 release commit과 두 artifact SHA-256에 묶습니다. 자동 검사 결과로 이 수동 evidence를 대신 만들지 않습니다.

`release/verify-unsigned-candidate.mjs`는 prepare workflow의 두 installer, platform provenance와 Windows clean-user evidence를 재검증합니다. Publish workflow는 successful prepare run의 artifact를 이 verifier로 확인하고 completed manual evidence까지 일치할 때만 새 prerelease를 만들며 candidate를 다시 build하지 않습니다.

개인용 publish script와 private credential material은 공개 저장소 밖에서 관리합니다. GitHub Releases용 재현 가능한 검증 도구는 public supply-chain interface이므로 이 디렉터리에 둡니다.
