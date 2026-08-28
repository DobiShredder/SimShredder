# Versioned resources

애플리케이션 동작에 필요한 검토·버전 관리된 정적 resource를 둡니다.

- `rules/`: WoW Retail build와 revision에 고정된 장비 최적화 제약 규칙
- `game-data/enhancements/`: game build·season별 gem/enchant catalog. `0.x`는 의도적으로 빈 catalog이며 exact ID 수동 입력만 사용합니다.

강화 catalog는 작은 immutable 배포 자료이므로 SQLite server나 사용자 DB가 아니라 검토·서명 가능한 JSON을 원본으로 사용합니다. `crates/data/enhancement-catalog`가 schema 검증과 build별 provider 경계를 제공하며, 향후 공식 데이터 갱신은 앱 코드를 바꾸지 않고 새 catalog revision을 공급합니다.

사용자별 다운로드, cache와 생성 결과는 이 디렉터리에 저장하지 않습니다.
