# Rust workspace

Rust crate는 의존성 방향과 역할에 따라 나눕니다.

- `core/`: 외부 시스템에 의존하지 않는 domain model, profile parser와 SimC input builder
- `features/`: core를 조합해 제품 기능을 구현하는 독립 모듈
- `application/`: desktop·headless 진입점이 호출하는 use case와 orchestration
- `infrastructure/`: SimulationCraft process, runtime 설치, 작업 큐, 저장소와 icon cache adapter

가능한 의존성 방향은 `application -> features/infrastructure/core`, `features -> core`, `infrastructure -> core`입니다. `core`가 상위 계층이나 GUI에 의존하지 않도록 유지합니다.
