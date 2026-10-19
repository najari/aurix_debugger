# aurix_debugger

AURIX(TC27x) 디버그 환경 실험 — Infineon DAS/TAS 스택 위에서 동작하는 MCD API 구현과 분석 자료.

| 경로 | 내용 |
|---|---|
| [`mcd_tas/`](mcd_tas/README.md) | **MCD API v1.1 over TAS** — `mcdxdas.dll` 대체 DLL 프로토타입(C++, CMake). TAS 서버(TCP 24817)를 백엔드로 MCD 54개 함수 export, run/stop/step/reset/트리거 실구현 |
| [`docs/`](docs/) | TAS 프로토콜 캡처, DAS/MCD 도구 분석, TC27x 디버그 레지스터, 프로토타입 설계 문서 |

## 빠른 시작

```powershell
cmake -S mcd_tas -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Release
build\Release\mcd_tas_smoke.exe          # 읽기 전용 스모크 (TAS 서버 필요)
```

MCD Basic Client 연동: 클라이언트 폴더 사본에 `mcd_tas.dll`을 `mcdxdas.dll` 이름으로 배치 —
`mcd_tas/drop_in/run_client.cmd` 참고(클라이언트 exe·DLL은 저장소에 포함하지 않음).

## 라이선스

- `mcd_tas/include/` SPRINT MCD API 헤더: 원 SPRINT 라이선스(헤더 내 고지 참조)
- 그 외 프로젝트 코드: MIT
