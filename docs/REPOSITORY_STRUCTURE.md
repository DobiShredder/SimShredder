# Repository structure

SimShredder는 실행 코드와 자료를 역할별 디렉터리에 모읍니다.

| 경로 | 역할 |
|---|---|
| `apps/` | 실행 가능한 애플리케이션 |
| `crates/` | 계층별 Rust workspace crate |
| `docs/` | 공개 사용자·개발 문서 |
| `legal/` | 법적 고지, license 설정과 생성 보고서 |
| `resources/` | versioned runtime resource |
| `test-data/` | 공용 fixture와 검증 evidence |
| `tooling/` | 제품에 포함되지 않는 유지보수 도구 |

`apps/desktop/config/`에는 Vite·Vitest·TypeScript·WebDriver 설정을 모읍니다. Tauri의 Rust manifest와 설정은 Tauri CLI가 기대하는 `apps/desktop/src-tauri/`에 유지합니다.

## 루트에 유지하는 파일

다음 파일은 저장소 전체의 진입점이거나 개발 도구가 기본 위치에서 자동 탐색하므로 다른 디렉터리로 이동하지 않습니다.

| 파일 | 유지 이유 |
|---|---|
| `Cargo.toml`, `Cargo.lock` | Cargo workspace root와 고정 dependency graph |
| `rust-toolchain.toml` | 디렉터리 진입 시 Rust toolchain 자동 선택 |
| `package.json`, `pnpm-workspace.yaml`, `pnpm-lock.yaml` | pnpm workspace root와 고정 dependency graph |
| `.node-version` | Node version manager 자동 선택 |
| `.gitignore`, `.gitattributes` | Git repository 전체 정책 |
| `README.md`, `LICENSE`, `NOTICE` | hosting·package 도구가 찾는 공개 표준 문서 |

GitHub Actions workflow는 GitHub가 인식하는 고정 경로인 `.github/workflows/`에 둡니다. 각 Rust crate의 `Cargo.toml` 역시 Cargo package 경계이므로 해당 crate 루트에 둡니다. 이 파일들을 별도 `config/`로 옮기면 일반적인 `cargo`, `pnpm`과 GitHub Actions 자동 탐색이 깨지므로 이동하지 않습니다.
