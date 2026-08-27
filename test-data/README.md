# Shared test data

- `fixtures/`: network와 실제 SimulationCraft 없이 재현하는 입력, manifest와 결과 fixture
- `evidence/`: 실제 binary·GUI·release 검증에서 만든 기계 판독 가능한 기록

테스트 코드는 각 애플리케이션 또는 crate 가까이에 두고, 여러 모듈이 공유하는 고정 데이터만 이곳에 둡니다.
