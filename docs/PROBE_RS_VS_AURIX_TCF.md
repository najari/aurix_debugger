# probe-rs TriCore vs aurix-tcf 확장 기능 비교

2026-10-23 · 원본: [Claude 문서](https://claude.ai/artifact/H9kLK73bCb8QuqCmJYgzdQ)

## 결론

소스 디버깅의 기본 흐름(브레이크포인트, step, 콜스택, 변수, 디스어셈블리, 주변장치 읽기)은 두 도구가 비슷하다. probe-rs TriCore에 부족한 것은 네 가지다. 하나는 모든 코어를 함께 멈추는 멀티코어 방식이고, 둘째는 지원 칩 범위(TC2x·TC3x·TC4x 전체 vs TC27x)다. 나머지 둘은 Live Watch와 주변장치 레지스터 쓰기다.

- 비교 대상: [ZeZhan/vscode-aurix-tcf-debugger](https://github.com/ZeZhan/vscode-aurix-tcf-debugger) `main`(README, `package.json`, `src/`)과 [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore)(커밋 `ff7ddc0`)
- probe-rs 쪽 내용은 TC275 ShieldBuddy 실기 시험 결과 기준이다. aurix-tcf 쪽 내용은 저장소 문서와 코드를 기준으로 했고, 실기로 다시 확인하지는 않았다.

## 부족한 점

중요도 순서다. 1–3은 디버깅 방식과 지원 범위의 차이고, 4–10은 편의 기능의 차이다.

| # | 항목 | aurix-tcf 확장 | probe-rs TriCore (현재) | 메우는 방법 |
| --- | --- | --- | --- | --- |
| 1 | 멀티코어 동작 | 세션 하나에 모든 코어가 스레드로 나옴. 한 코어가 멈추면 모든 코어가 멈춤(all-stop) | 코어별 세션 3개를 compound 설정으로 묶음. 코어마다 따로 멈춤. 모두 멈추게 하는 옵션 없음 | OCDS 정지 신호 브로드캐스트 옵션을 추가하거나, probe-rs DAP에 멀티코어를 구현(업스트림 TODO) |
| 2 | 지원 칩 | TC2x, TC3x, TC4x 전체 | TC27x(TC275)만 | WP-9: 칩별 타깃 정의, TC3xx 플래시 로더(DMU 방식), CPU5 CSFR 창 처리 |
| 3 | 플래시 | 모든 칩, PFLASH와 DFLASH, PPU/CDSP HEX | TC27x PFLASH만. DFLASH와 UCB 없음 | 칩 계열별 로더와 DFLASH 시퀀스 |
| 4 | Live Watch | 실행 중에 전역 변수를 주기적으로 갱신(최소 200 ms, `aurix-tcf/liveWatchEvaluate`) | 없음. WATCH는 멈췄을 때만 갱신 | DAP 사용자 정의 요청과 VS Code 뷰. TriCore는 실행 중에도 메모리를 읽을 수 있어 구현이 쉬움 |
| 5 | 주변장치 레지스터 쓰기 | Peripheral Viewer 확장(`mcu-debug.peripheral-viewer`)으로 읽기와 쓰기 | `Peripherals`가 읽기 전용. probe-rs `set_variable`이 SVD 변수를 다루지 않음 | SVD 변수 쓰기를 구현하거나 Peripheral Viewer 확장을 연결 |
| 6 | 리셋 후 진입 함수에서 정지 | `entryFunction`(예: `core0_main`)까지 실행해 멈춤. 리셋 뒤 모든 코어 정지 | attach만 검증. 데모는 `start_in_flash` 작업으로 대신함 | launch(리셋)와 진입 함수 임시 브레이크포인트 |
| 7 | 브레이크포인트 개수 | TCF 에이전트가 관리(RAM 코드에는 SW 브레이크포인트) | 코어당 HW 8개. SW 브레이크포인트 없음(FR-C5) | RAM 코드에 `debug` 명령을 넣는 SW 브레이크포인트 |
| 8 | 식 평가 | TCF Expressions 서비스(C 식) | probe-rs 평가기(변수 이름 위주, 연산 제한) | probe-rs 업스트림 기능에 의존 |
| 9 | PPU, CDSP, SCR 코어 | ELF를 붙여 디버깅 | 없음 | TriCore가 아닌 아키텍처라 범위가 큼 |
| 10 | 설정 UI | 설정 패널, 장치 검출 버튼, SVD 선택 명령 | `launch.json` 직접 작성. 장치와 SVD는 자동 매칭 | 필요하면 간단한 명령 추가 |

## 기능은 있지만 TriCore에서 시험하지 않은 것

probe-rs DAP 서버가 지원한다고 알리는 기능(capability)이다. aurix-tcf에는 있는 기능이지만, TriCore에서는 아직 돌려 보지 않았다.

- [ ] 명령어 단위 스텝(`supportsSteppingGranularity`)
- [ ] 디스어셈블리 창의 명령어 브레이크포인트(`supportsInstructionBreakpoints`)
- [ ] Memory Inspector를 통한 메모리 읽기와 쓰기(`readMemory`, `writeMemory`)
- [ ] Restart(리셋)
- [ ] `launch`와 `flashingEnabled` 조합(쓰고 바로 디버깅)
- [ ] 변수 값 바꾸기(`setVariable`)
- [ ] GDB 서버(TriCore 레지스터 기술만 구현)

## probe-rs 쪽이 나은 점

probe-rs는 백엔드 전체가 공개돼 있고, 디버거 밖에서도 쓸 수 있는 도구를 갖추고 있다.

| 항목 | probe-rs TriCore | aurix-tcf 확장 |
| --- | --- | --- |
| 백엔드 | 전체 오픈소스(Rust) | 비공개 TCF 에이전트와 DAS 구성요소를 함께 배포 |
| 프로브 | TAS와 MCD 라이브러리(`058b:0001`) | TCF 에이전트(DAS) |
| MCDS | `probe-rs mcds record`, `mcds timestamp` | 없음 |
| CLI | `download`, `read`, `info`, `mcds`를 스크립트와 CI에서 사용 | VS Code 안에서만 |
| 변수 쓰기 | `supportsSetVariable = true` | `supportsSetVariable = false` |
| 원격 접속 | TAS가 TCP라 다른 PC의 TasServer에 접속(CLI 기준) | 로컬 에이전트 |
| 플래시 안전장치 | 같은 섹터는 건너뜀, 지워진 페이지는 ECC로 판별, 금지 주소 거부 | 쓰기에 실패하면 `-erase all`로 다시 시도(기존 플래시 내용이 지워질 수 있음) |

## 권장 순서

지금 보드로 바로 구현하고 검증할 수 있는 4·5·6번을 먼저 하고, 하드웨어가 더 필요한 항목은 그 뒤로 미룬다.

1. **Live Watch (4번)**: 실행 중 메모리 읽기는 이미 동작한다. DAP 요청과 VS Code 뷰만 더하면 된다.
2. **주변장치 쓰기 (5번)**: SVD 변수 쓰기를 구현하거나 Peripheral Viewer 확장과 연결한다.
3. **진입 함수에서 정지 (6번)**: 리셋 후 정지(T-08)는 검증됐다. 임시 브레이크포인트만 더하면 된다.
4. **미시험 기능 확인**: 위 목록의 7가지를 VS Code에서 시험한다. `launch`와 `flashingEnabled` 조합은 플래시에 쓰므로 따로 승인을 받는다.
5. **동시 정지 (1번)**: OCDS 정지 브로드캐스트 동작을 실기로 먼저 확인해야 한다.
6. **칩과 플래시 범위 확대 (2·3번)**: TC3xx 등 해당 보드가 있어야 한다.

7–10번(SW 브레이크포인트, 식 평가, PPU·CDSP·SCR, 설정 UI)은 필요가 생기면 다룬다.

## 출처

- [ZeZhan/vscode-aurix-tcf-debugger](https://github.com/ZeZhan/vscode-aurix-tcf-debugger): `README.md`, `package.json`(명령, 뷰, 설정), `src/debugProvider.ts`(DAP 기능 플래그, `entryFunction`, 전체 코어 정지), `src/liveWatchView.ts`, `src/svdCatalog.ts`
- [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore): DAP 기능 플래그(`adapter.rs`, `debugger.rs`), `rpc/functions/debug_vars.rs`(변수와 SVD), 실기 시험 기록
- [probe-rs TriCore · MCD · MCDS 연동 개발지시서 (PRD)](https://claude.ai/artifact/GqYnnvgJjCWxkZUUsqJSzq): 작업 패키지와 시험 결과
