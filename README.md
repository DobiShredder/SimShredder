# SimShredder

SimShredder는 SimulationCraft를 로컬에서 편리하고 재현 가능하게 실행하기 위한 World of Warcraft 데스크톱 도구입니다. 자체 전투 엔진을 만들지 않고 캐릭터 입력, 시뮬레이션 설정, 작업 관리, Top Gear 조합 생성과 결과 비교를 담당합니다.

GUI가 기본 사용 방식입니다. Quick Sim과 Top Gear의 입력부터 작업 진행, 복구와 상세 결과까지 데스크톱 앱 안에서 완료하는 것을 목표로 하며, Raidbots를 복제하지 않는 독자적인 local-first 인터페이스를 개발합니다.

> 현재 상태: Apple Silicon macOS 26과 Windows x64에서 Quick Sim과 Phase 4 Top Gear GUI까지 구현했습니다. 공식 SimC의 사용자별 설치·update·rollback, profile import, exact `.simc` preview, 영속 실행·복구, 결과와 raw artifact export를 앱 안에서 완료할 수 있습니다. 두 OS native CI에서 공식 SimC 자동 설치, Quick Sim, Top Gear, platform별 screenshot과 unsigned current-user installer를 검증했습니다. Phase 5에서는 Ed25519 서명 runtime catalog와 key rotation·rollback 방지 경계를 구현했지만 production GitHub Release asset과 별도 clean non-admin 계정 검증은 아직 완료되지 않았습니다.

> 현재 icon·tooltip 정책: 모든 `0.x` test version은 실제 WoW artwork와 remote icon provider를 제공하지 않고 자체 semantic glyph를 사용합니다. 장비에는 확인된 로컬 정보 tooltip을 표시하고, 사용자가 명시적으로 선택한 경우에만 숫자 item/spell ID의 Wowhead page를 OS 기본 browser로 엽니다. `1.0.0`에서는 scheduled GitHub Actions가 Blizzard 공식 API로 만든 서명·만료 정적 catalog와 공식 CDN의 사용자별 cache를 활성화할 계획입니다. Raidbots data, Wowhead script와 Wowhead image database는 사용하지 않습니다.

## 목표 기능

- SimulationCraft addon 문자열과 `.simc` 파일 가져오기
- Quick Sim 설정, 실행, 진행률과 상세 결과
- 생성된 최종 `.simc`, JSON, HTML과 실행 로그 보존
- 착용 및 가방 장비 기반 Top Gear 후보 선택
- 가상 보석·마법부여·업그레이드와 보유 문장·강화 화폐의 효율적인 사용 순서 추천
- 유효 조합 사전 계산, 중복 제거와 단계적 탐색
- 영속 작업 큐, 취소, 재시도, 완료 배치 단위 복구
- 통계 오차를 반영한 기준 장비 대비 비교
- SimC 설치 상태, 작업 queue, history와 raw artifact를 다루는 완전한 desktop GUI
- skill, buff, talent와 equipment를 `0.x`에서는 semantic glyph와 local tooltip으로, `1.0.0` 이상에서는 검증된 공식 icon의 on-demand local cache로 표시
- 한국어와 영어 동시 지원
- 공식 SimulationCraft의 per-user 자동 download, update와 rollback

Droptimizer, Blizzard API, MDT route simulation, APL Lab과 클라우드 실행은 첫 MVP에 포함하지 않습니다.

## 지원 게임

- World of Warcraft Retail Live만 지원합니다.
- WoW Classic, PTR와 Beta는 지원하지 않습니다.
- 지원 범위를 벗어난 profile, SimC build와 실행 option은 실행 전에 진단합니다.

## 기술 구성

- Desktop shell: Tauri 2
- UI: TypeScript 기반 UI
- UI framework: React + Vite, Radix Primitives
- Package manager: pnpm
- Charts와 icon: Apache ECharts, Lucide
- Localization: i18next/react-i18next와 검증되는 JSON catalog (`ko`, `en`)
- Core와 local runner: Rust
- Metadata: SQLite
- Simulation engine: 공식 server에서 사용자 장치로 직접 받아 검증하는 외부 SimulationCraft CLI

현재 개발 baseline은 Rust 1.98.0, Node.js 24.x와 pnpm 11.23.0입니다. Rust toolchain과 Cargo dependency는 repository의 toolchain file과 lockfile에, desktop dependency는 `pnpm-lock.yaml`에 exact resolution으로 고정합니다.

## 요구 환경

현재 GUI는 Apple Silicon macOS 26과 Windows x64용으로 build되며 Quick Sim과 Top Gear 전체 흐름을 공식 Retail Live SimC nightly `1210-01` revision `02b39ce`로 검증했습니다. 기본 공개 package는 unsigned입니다.

| 운영체제와 architecture | 지원 상태 |
|---|---|
| Apple Silicon macOS 26 Tahoe 이상 | 초기 지원 대상·unsigned release 검증 중 |
| Windows x64 | 초기 지원 대상·native CI 검증 완료 |
| Intel x64 macOS | 지원하지 않음 |
| Windows ARM64 | 추후 지원 예정 |
| Linux | 지원하지 않음 |

초기 공개 배포는 Apple Silicon macOS와 Windows x64에서 핵심 기능과 installer 검증이 모두 끝나야 완료된 것으로 봅니다. Intel Mac은 legacy architecture로 판단해 지원하지 않습니다. World of Warcraft가 Linux를 공식 지원하지 않으므로 SimShredder도 Linux build와 호환성 지원을 제공하지 않습니다.

Windows x64에서는 해당 시점의 공식 SimulationCraft가 지원하는 Windows version 전체를 지원합니다. [SimulationCraft 공식 download 안내](https://www.simulationcraft.org/download.html)의 현재 공개 범위는 Windows 10/11 21H2 이상이며, 고정 SimC build를 바꿀 때마다 이 하한도 다시 검증합니다. SimShredder가 별도의 더 높은 최소 version을 두지 않습니다. Apple Silicon macOS의 최소 지원 버전은 macOS 26 Tahoe이며 macOS 25 이하는 지원하지 않습니다.

설계와 현재 installer 설정은 관리자 권한, system-wide PATH, Windows service와 machine-wide registry 변경을 요구하지 않고 사용자별 앱 데이터 경로를 사용합니다. 두 OS의 clean non-admin 계정 설치·실행은 공개 release 전에 완료해야 하는 미검증 gate입니다.

프로필과 결과는 로컬에 유지되며 telemetry나 crash upload를 보내지 않습니다. Production network endpoint, 저장 데이터와 삭제 방법은 [개인정보 처리 안내](docs/PRIVACY.md)에 공개합니다.

기본 저장 위치는 macOS의 `~/Library/Application Support/SimShredder`, Windows의 `%LOCALAPPDATA%\SimShredder`입니다. 시뮬레이션 기록, 관리형 SimulationCraft, icon cache와 export 폴더는 Settings에서 각각 직접 입력하거나 Finder/파일 탐색기의 `찾아보기…`로 변경할 수 있습니다. 위치 변경은 기존 파일을 자동 이동하거나 삭제하지 않습니다.

## 설치, 실행 및 테스트

사용자용 설치본은 아직 release하지 않습니다. 개발 검증에는 Rust 1.98.0, Node.js 24와 Corepack을 준비하고 다음 명령을 사용합니다.

```bash
corepack pnpm install --frozen-lockfile
corepack pnpm check
corepack pnpm test
corepack pnpm build
corepack pnpm tauri build --bundles app
corepack pnpm e2e:build
corepack pnpm e2e
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

실제 SimC까지 포함한 macOS GUI end-to-end 검증은 이미 검증된 executable을 다음처럼 명시합니다.

```bash
SIMSHREDDER_E2E_SIMC=<absolute-path-to-simc> corepack pnpm e2e
```

완전히 빈 사용자 data directory에서 앱의 `Download and install` 버튼부터 Quick Sim과 Top Gear까지 확인하려면 다음 release smoke를 실행합니다. 약 227 MB의 공식 artifact를 실제로 받으며 network 속도에 따라 수 분이 걸릴 수 있습니다.

```bash
SIMSHREDDER_E2E_AUTO_INSTALL=1 corepack pnpm e2e
```

이미 별도로 내려받아 hash를 확인한 공식 artifact를 사용하면서도 빈 runtime state와 앱 자체 catalog·installer·activation 경로를 검증하려면 `SIMSHREDDER_E2E_RUNTIME_ARTIFACT=<absolute-path>`를 함께 지정할 수 있습니다. 이 변수는 production 앱이 아니라 E2E 준비 단계에서만 verified download cache를 채웁니다.

E2E build에만 embedded WebDriver plugin과 capability를 넣으며 production Cargo graph와 frontend bundle에서는 둘 다 제외합니다.

각 target과 일치하는 build host에서 두 package를 다음 명령으로 생성합니다. macOS는 `.app`과 `.dmg`, Windows는 current-user x64 NSIS `-setup.exe`를 만듭니다. Apple Silicon host의 `cargo-xwin` package는 정적 사전 검증용이며 Windows native 실행 증거는 GitHub-hosted Windows workflow에서 별도로 확보합니다.

```bash
pnpm --filter @simshredder/desktop exec tauri build --target aarch64-apple-darwin
pnpm --filter @simshredder/desktop exec tauri build --target x86_64-pc-windows-msvc
```

Commit, push와 pull request는 GitHub Actions workflow를 자동으로 시작하지 않습니다. `.github/workflows/ci.yml`은 Actions 화면의 수동 `workflow_dispatch`로만 실행하며, `run_native`를 선택했을 때만 `macos-26`과 `windows-2025` formatter, static check, test, UI build, ko/en GUI E2E, unsigned current-user installer, locked license 보고서와 실제 공식 Windows SimC 계약을 수행합니다. `capture_gui_baselines`는 반드시 `run_native`와 함께 사용해야 합니다. Commit과 무관한 일일 SimC catalog freshness schedule은 upstream artifact 삭제·크기 변경 감지를 위해 별도로 유지합니다. Clean-root run `32964022873`에서 전체 native CI가 통과했습니다. 기본 GitHub Release도 unsigned package와 commit-bound SHA-256 provenance를 사용합니다. `.github/workflows/release-candidate.yml`은 향후 credential을 확보했을 때만 Developer ID/notarization과 Windows Authenticode를 추가하는 optional 경로입니다. 완전한 Rust/Node license 본문은 설치본 내부와 release sidecar에 모두 포함됩니다. 별도 clean non-admin 계정 설치 증거는 서명 여부와 관계없이 공개 release 전에 필요합니다.

Unsigned 공개 절차는 build와 publish를 분리합니다. `unsigned-release.yml`은 current `master`에 붙은 v0.x tag에서 두 OS candidate와 provenance만 만들고 Release를 생성하지 않습니다. Maintainer가 그 exact artifact를 `docs/RELEASE_MANUAL_VERIFICATION.md`에 따라 두 OS 표준 계정에서 검증한 뒤, `publish-unsigned-release.yml`에 candidate run ID와 completed evidence를 제공해야만 동일 artifact가 prerelease로 게시됩니다. Publisher는 artifact hash, source commit, Windows medium-integrity 설치 evidence, 한·영 Quick Sim·Top Gear와 수동 접근성 evidence가 하나라도 맞지 않으면 발행하지 않습니다.

`0.x` application은 GitHub Releases에서 수동으로 갱신합니다. `1.0.0`부터 GitHub Release asset을 사용하는 updater를 제공하되 application-owned update signature와 rollback 방지를 검증합니다. 이 무결성 서명과 SimC runtime catalog의 Ed25519 서명은 유료 운영체제 publisher signature와 별개입니다.

SimulationCraft nightly 파일은 upstream에서 예고 없이 교체될 수 있습니다. 저장소의 일일 freshness workflow가 signed catalog의 두 공식 URL과 크기를 확인하고, catalog 게시 workflow는 게시 직전에 두 파일 전체의 signed 크기·SHA-256을 검증한 뒤 게시 직후 가용성을 다시 확인합니다. Private signing key가 필요한 새 catalog 서명과 게시 승인은 자동화하지 않습니다.

현재 package는 macOS Developer ID/notarization과 Windows Authenticode가 없습니다. 따라서 Gatekeeper, SmartScreen 또는 Smart App Control이 경고하거나 실행을 막을 수 있습니다. Release note에는 unsigned 상태, SHA-256 확인 방법과 각 운영체제의 정상적인 수동 허용 절차를 한국어·영어로 제공합니다. SimShredder는 보안 기능을 자동으로 끄거나 우회하지 않습니다.

상세 절차와 unsigned package가 실행될 수 없는 관리형 환경의 제한은 [unsigned 설치 안내](docs/UNSIGNED_INSTALLATION.md)를 확인하세요.

Network와 실제 SimC 없이 실행되는 기본 test가 nightly listing parser, manifest validation과 고정 계약을 검사합니다. `crates/infrastructure/simc-adapter/tests/live_macos.rs`의 ignored test는 약 227 MB의 공식 DMG download, read-only mount와 실제 SimC 실행을 의도적으로 요청할 때만 환경 변수와 `--ignored`로 실행합니다.

GitHub Actions의 Windows 계약 job은 bundled Ed25519-signed catalog에서 현재 x64 manifest를 선택하고 공식 `win64.7z`를 직접 내려받아 size·SHA-256, 안전 추출, x64 PE, Retail Live identity, build별 JSON/HTML/exit/cancel/profileset 계약을 실제 `simc.exe`로 검사합니다. Windows 설치본은 관리자 권한을 요구하지 않는 한·영 NSIS current-user installer이며 `%LOCALAPPDATA%`만 사용합니다. Clean-root native run `32964022873`에서 이 계약과 설치·실행·제거가 통과했지만 GitHub-hosted `runneradmin`은 별도 clean non-admin OS account 증거를 대신하지 않습니다.

SimC update metadata는 SimShredder의 Ed25519 release key로 서명합니다. 앱은 서명, 만료, 단조 증가 sequence, 대상 OS·architecture, official URL, size와 SHA-256을 검증한 뒤에만 artifact를 받습니다. 신뢰 key 변경은 이전 key가 서명한 catalog chain으로만 허용하며, 수락한 chain은 사용자별 data directory에 원자적으로 저장해 offline 실행과 metadata rollback 방지를 함께 제공합니다. Private signing key는 source, application bundle과 release artifact에 포함하지 않습니다.

앱은 현지 날짜 기준 일일 최초 실행에서만 새 signed SimC build를 background로 확인하며 강제로 설치하지 않습니다. Update가 있으면 현재 build와 새 build를 보여주고 `Yes, update` 또는 `Later`를 선택하게 합니다. `Later`를 고르면 같은 날 다시 실행해도 재질문하지 않고 기존 build를 계속 사용하며, Settings의 수동 확인은 언제든 가능합니다.

Phase 2의 SQLite queue는 profile, job, ordered batch와 append-only attempt 이력을 저장합니다. 실행 중 앱이 종료되면 완료 batch는 재사용하고 중단 batch만 새 attempt로 재개합니다. 실시간 stdout/stderr는 stream별 1 MiB로 제한해 저장하며 잘림 상태를 기록합니다. Cache는 generated bytes, executable SHA-256, SimC version/revision, game version, normalized schema와 rule revision이 모두 같고 전체 artifact digest audit가 통과할 때만 사용합니다.

Top Gear는 가져온 착용·가방 장비와 사용자가 추가한 가상 보석·마법부여·한 단계 강화 상태를 조합합니다. 정확한 원본/유효/실행 수와 소켓·부위·고유 장착·장식·무기·예산·대칭 중복별 제외 사유를 실행 전에 보여주며, 최대 256개 조합을 저정밀 profileset으로 탐색한 뒤 사용자가 확인한 finalist만 고정밀로 재검증합니다. Engine은 전체 유효 개수를 계산하면서 emitted 조합만 제한해 보관하고, 원시 조합이 2,000,000개를 넘으면 후보 축소를 요구합니다. 결과는 결합 오차와 다중 화폐 Pareto 상태를 포함하며 필터·정렬·2개 조합 비교를 지원합니다.

강화 비용은 계정·캐릭터 할인까지 적용된 실제 값을 사용자가 확인해 입력합니다. 각 화폐의 reserve를 보존하며 선택된 최종 조합에 action이 있으면 dependency-valid 중간 상태를 실제 SimC로 다시 평가해 한계 DPS, 누적 DPS, 비용과 남은 화폐가 있는 순서를 생성합니다. 현재 bundled rule `12.1.0-69465-v1`은 Retail build, 소켓·마법부여·고유·장식·무기 제약을 고정합니다. 원격 signed rule catalog와 자동 비용 데이터는 서명 배포를 다루는 Phase 5 gate입니다.

개발용 headless CLI는 이미 검증된 SimC 실행 파일을 명시적으로 받아 addon export 또는 `.simc` 파일을 실행합니다. `<output-directory>`는 존재하지 않아야 하며 기존 결과를 덮어쓰지 않습니다.

```bash
cargo run --locked -p simshredder-core --bin simshredder-headless -- \
  --format addon \
  --source <profile.simc> \
  --executable <absolute-path-to-simc> \
  --revision <simc-git-revision> \
  --output <output-directory> \
  --timeout-seconds 60
```

결과 폴더에는 원본·생성 input, stdout/stderr, raw JSON2/HTML, normalized JSON과 SHA-256 manifest가 남습니다. 지원하지 않는 channel·option, 실행 실패, 결과 schema 불일치와 timeout은 성공으로 오인하지 않으며 실행이 시작된 뒤의 진단 산출물은 보존합니다.

Phase 0A에서 확인한 macOS 계약은 다음과 같습니다.

- official host/path allowlist, 고정 size·SHA-256, 512 MiB 상한과 redirect 거부
- read-only DMG mount, top-level `simc`만 제한적으로 복사하고 ARM64/Retail Live identity 검증
- Quick와 profileset의 JSON2 golden projection 및 HTML 문서 검증
- missing input exit code 60과 장기 실행 cancel deadline 검증
- stdout, stderr, JSON과 HTML 원본 보존

공식 artifact는 `downloads.simulationcraft.org/nightly/`의 정확한 allowlisted HTTP 또는 HTTPS path에서만 받으며 redirect, user info, 명시적 port, query와 fragment를 거부합니다. Production 자동 update는 HTTPS로 받은 SimShredder 서명 catalog의 고정 URL·size·SHA-256이 모두 일치해야 진행됩니다. 공식 nightly HTTPS endpoint의 인증서가 유효하지 않은 동안에는 TLS 검증을 우회하지 않고 서명 catalog가 고정한 공식 HTTP artifact를 사용합니다. 공개 binary라 전송 기밀성은 제공하지 않지만 Ed25519 catalog와 exact SHA-256으로 무결성과 출처 승인을 검증합니다. Nightly listing은 release manifest 후보 확인용일 뿐 trust root가 아닙니다.

## Repository 구조

```text
apps/                    실행 가능한 애플리케이션
  desktop/               Tauri와 React 데스크톱 앱 (`config/`에 UI tool 설정)
crates/                  Rust workspace
  application/           앱 use case와 orchestration
  core/                  순수 domain, profile parsing, SimC input 생성
  features/              Top Gear 같은 제품 기능
  infrastructure/        SimC, runtime, queue, storage, cache adapter
docs/                    공개 사용자·개발 문서
legal/                   고지, cargo-about 설정, 생성된 license 보고서
resources/               버전 관리되는 runtime rule과 정적 resource
test-data/               공용 fixture와 검증 evidence
tooling/                 localization·license 유지보수 도구
```

각 영역의 상세 책임은 해당 디렉터리의 `README.md`에 기록합니다. 빌드 설정과 lockfile, `README.md`, `LICENSE`, `NOTICE`는 저장소 전체에 적용되므로 루트에 둡니다.

## 공개 및 라이선스

SimShredder 자체 코드는 [Apache License 2.0](LICENSE)으로 공개합니다.

SimulationCraft binary는 SimShredder에 포함하거나 재배포하지 않습니다. 앱의 자동 설치 기능은 공식 SimulationCraft server에서 사용자의 장치로 직접 내려받아 검증·설치합니다. SimulationCraft, World of Warcraft와 관련 데이터 및 상표는 각각의 권리자와 라이선스 조건을 따릅니다. SimShredder는 SimulationCraft 또는 Blizzard Entertainment의 공식 제품이 아닙니다.

## 기여

개발 초기에는 외부 기여를 받지 않습니다. public 전환 시 기여 가이드, 행동 강령과 issue template을 추가할 예정입니다.

버그 제보와 테스트 피드백은 이 repository의 GitHub Issues에서만 받습니다. 별도 telemetry, analytics와 자동 crash upload는 사용하지 않습니다.

보안 문제는 [보안 정책](.github/SECURITY.md)을 따르고, 제3자 구성 요소는 [제3자 고지](legal/THIRD_PARTY_NOTICES.md)에서 확인할 수 있습니다. UI 문구를 추가하거나 번역할 때는 [localization 안내](docs/LOCALIZATION.md)의 JSON catalog 규칙과 자동 검증 절차를 따릅니다.
