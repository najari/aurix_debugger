# aurix_debugger

AURIX(TC27x) 디버그 환경 실험 — Infineon DAS/TAS 스택 위에서 동작하는 MCD API 구현과 분석 자료.

| 경로 | 내용 |
|---|---|
| [`mcd_tas/`](mcd_tas/README.md) | **MCD API v1.1 over TAS** — `mcdxdas.dll` 대체 DLL 프로토타입(C++, CMake). TAS 서버(TCP 24817)를 백엔드로 MCD 54개 함수 export, run/stop/step/reset/트리거 실구현 |
| [`mcdsrl/`](mcdsrl/README.md) | **MCDS Recorder Library over TAS** — `mcds_recorder.dll` API 호환 재구현(22 export), TAS 백엔드로 MCDS 트레이스 녹화 → `.mcds` |
| [`mcdsdl/`](mcdsdl/README.md) | **MCDS Decoder Library** — `mcds_decoder.dll` API 표면(25 export) 프로토타입: `.mcds` 로드/저장·ELF 심볼·메시지 문자열 (디코드 엔진은 미구현) |
| [`m25_tcf/`](m25_tcf/README.md) | **AURIX TCF 에이전트 백엔드** — Eclipse TCF 에이전트(`tcf.agent` c441453)에 얹는 TC27x 네이티브 백엔드: TAS 접속, 3코어 run/stop/step, HW 브레이크포인트(TR0–7), CSA 스택 크롤, TriCore 디스어셈블러, 리셋. VS Code(AURIX TCF 확장)에서 디버깅 |
| [`docs/`](docs/) | TAS 프로토콜 캡처, DAS/MCD 도구 분석, TC27x 디버그 레지스터, 프로토타입 설계 문서, probe-rs TriCore 연동 [기술 검토](docs/PROBE_RS_TRICORE_REVIEW.md)·[개발지시서(PRD)](docs/PROBE_RS_TRICORE_PRD.md)·[aurix-tcf 확장과 기능 비교](docs/PROBE_RS_VS_AURIX_TCF.md) |

## 빠른 시작

```powershell
cmake -S mcd_tas -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Release
build\Release\mcd_tas_smoke.exe          # 읽기 전용 스모크 (TAS 서버 필요)
```

MCD Basic Client 연동: 클라이언트 폴더 사본에 `mcd_tas.dll`을 `mcdxdas.dll` 이름으로 배치 —
`mcd_tas/drop_in/run_client.cmd` 참고(클라이언트 exe·DLL은 저장소에 포함하지 않음).

## 라이선스

- `*/include/mcd_api.h`, `mcd_types.h` SPRINT MCD API 헤더: 원 SPRINT 라이선스(헤더 내 고지 참조)
- `mcdsdl/include/`, `mcdsrl/include/`의 `mcds_*.h`: Infineon 헤더 원본(헤더 내 고지 —
  Infineon 마이크로컨트롤러용 개발 도구 제작에 자유 사용)
- 그 외 프로젝트 코드: MIT
