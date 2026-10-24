# 한 세션 멀티코어 개발지시서 (PRD)

2026-10-24 · 원본(구조 그림 포함): [Claude 문서](https://claude.ai/artifact/LaAQhorNNsiccUpDPhAhhZ)

## 0. 구현 현황 (2026-10-24)

WP-M1–WP-M7을 9절의 권장안대로 구현했고, 실기 시험 T-M1–T-M15를 모두 통과했다. 그 뒤 남은 작업(`singleThread`, 섹터 겹침 거부, 트리거 유닛 저널, 앱 리셋 뒤 그룹 잠김)과 TC36x(2코어) 실기 시험도 마쳤다. 커밋은 [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore)에 있다.

| 커밋 | WP | 내용 |
| --- | --- | --- |
| `763c12a` | WP-M1, WP-M2 | `coreConfigs` 여러 개 허용, 코어마다 스레드, 요청 라우터(`core_routing.rs`: `threadId`·`frameId`·`variablesReference`·`@코어`·`core` 인자, 나머지는 포커스 코어), 코어 전부에 보내고 응답을 합치는 요청(setBreakpoints, setInstructionBreakpoints, setDataBreakpoints, configurationDone, disconnect), 코어별 정지 이벤트(`allThreadsStopped: false`), continued 스레드 번호 버그 |
| `cb0e00d` | WP-M5 | `Session::status_many`: TriCore는 코어 전부의 `DBGSR`·`CBS_TRCn`·`EXEVT`·PC, 다시 `DBGSR`, STM을 TAS 요청 하나로 읽고 코어별 판단에 쓴다. 두 확인 사이에 실행됐다가 다시 멈춘 코어(정지→정지, `DBGSR`이나 PC가 다름)를 새 정지로 보고한다 |
| `2e8d3a9` | WP-M3, WP-M4, WP-M5 | 코어별 디버그 정보(`LoadDebugInfoRequest.core`), 소스 BP·위치를 코어의 디버그 정보로 해석, 서로 다른 바이너리를 한 번씩 플래시, restart가 코어 전부의 BP를 다시 계산하고 바뀐 바이너리를 다시 씀, 폴링이 `status_many`를 한 번 부름, 정지→정지로 바뀐 코어의 스택 다시 풀기 |
| `879e4f9` | 시험 도구 | `poll_cost`(폴링 요청 수·시간), `start_in_flash --core N` |
| `63d721a` | 복구 | `--connect-under-reset`가 TAS 프로브에 전달되게 고침, `sw_reset --group N` |
| `0e7761c` | WP-M6 | 그룹이 멈춘 코어(`external`)의 정지 이벤트에 `preserveFocusHint`, 포커스 코어는 그대로 |
| `9e035fc` | 5.2 | `singleThread`: capability `supportsSingleThreadExecutionRequests`, `singleThread` continue는 그 코어만 실행(RPC `core/run_alone`, `Session::run_alone`). 그룹 동료는 멈춘 채 남고 다음 일반 run에 함께 돈다 |
| `9e23c7a` | 5.5 | 플래시가 켜진 launch에서 두 ELF가 같은 바이트나 같은 PFLASH 섹터(TC27x·TC36x 섹터 맵)를 쓰면 대상에 손대기 전에 거부 |
| `84a45aa` | FR-M11 | 저널이 `debug` 명령 외에 세션이 켠 트리거 유닛(`TRnEVT`)과 그룹 라우팅(`CBS_TRCn`, `EXEVT`)도 기록. 다음 세션이 그 값이 그대로인 곳만 되돌림 |
| `d7d32e7` | 동시 정지 | 그룹 라인에 정지 상태(레벨) 대신 break-out 펄스를 싣는다(`CBS_TRCn` BRKOUT 7:4 + BRKIN 23:20 = 라인 7). 앱 리셋 뒤 장치 잠김 해결 |
| `7036cc3` | 시험 도구 | `trigger_switch mask`·`brkout`, `sw_reset --trc` |
| `ca86354` | 동시 정지 | 데이터 watchpoint 정지도 펄스로 전파(`TRnEVT` 0x22: `BOD` 0, `SUSP` 유지), `SWEVT` 0x2A로 복귀. 동료가 자기 watchpoint에 멈춘 것은 `SUSP` 흔적과 걸린 watchpoint가 함께 있을 때만으로 판단 |
| `eb4f09c` | 5.5 | `perfWindow` 실패 메시지에 코어 번호 |
| `bf53965` | TC36x | 앱이 시작하지 않은 코어(부트 정지, TC36x 시험 보드의 CPU1)를 잠든 코어로 보인다. 이 코어가 `coreConfigs`에 있어도 attach가 실패하지 않고, 스레드는 잠든 상태, pause는 거절된다. 데모 설정 `TC36x: probe-rs CPU0–CPU1 one session`(aurix_debugger `e56c310`) |

데모와 확장(이 저장소): `TC275: probe-rs CPU0–CPU2 one session` 설정(`m25_tcf/examples/aurix_tcf_demo/.vscode/launch.json`), Live Watch 확장 0.2.0(`vscode/probe-rs-live-watch`: 코어마다 읽기, **Select Cores**).

**시험 결과** (TC275 ShieldBuddy, `dap_onesession.js` 모드 default·`noallstop`·`samecode`·`restart`·`reset`, `dap_kill.js` `CORES=3`)

| ID | 결과 | 내용 |
| --- | --- | --- |
| T-M1 | 통과 | 스레드 3개(`0-TC275`…), configurationDone 뒤 세 코어 실행 |
| T-M2 | 통과 | CPU0 `breakpoint`, CPU1·CPU2 `external`, 스레드마다 자기 PC(DBGSR로 읽은 PC와 같음), `allThreadsStopped: false` |
| T-M3 | 통과 | CPU0 continue에 CPU1·CPU2도 continued. 응답의 `allThreadsContinued`는 `false`(아래 차이점) |
| T-M4 | 통과 | CPU1 `pause`, 나머지 `external` |
| T-M5 | 통과 | CPU0 step, CPU1·CPU2 PC 그대로 |
| T-M6 | 통과 | CPU0이 자기 BP에 남고 CPU1·CPU2 실행, `allThreadsContinued: false` |
| T-M7 | 통과 | `allStop` 없이 BP·continue·pause가 그 코어만 |
| T-M8 | 통과 | 세 코어가 PF1 데모를 함께 돌 때 소스 BP 하나가 코어마다 트리거 유닛 하나, 세 코어 모두 `breakpoint`로 정지(2회 반복) |
| T-M9 | 통과 | 프레임마다 그 코어의 레지스터, CPU0 프레임의 evaluate. CPU1 프레임의 setVariable(`d0` = 0x12345678)이 CPU1의 D0만 바꾸고 되돌림(CPU0의 D0 그대로) |
| T-M10 | 통과 | `dataId` `0x7000a004/4@0`, CPU0에서 `data breakpoint` |
| T-M11 | 통과 | restart: 세 코어 라우팅과 BP가 되살아나고 pause·continue 정상. 앱 SW 리셋(`allStop` 켬): 33 ms 뒤 다시 접근되고, BP가 세 코어에 남고 CPU0이 시작 코드에서 정지, 라우팅 유지, 콘솔 알림 |
| T-M12 | 통과 | 강제 종료 뒤 다음 세션이 `debug` 명령 4개를 되돌리고, 트리거 유닛을 지우고(0,0,0), 라우팅을 원래 값(TRC0 0x2)으로 돌린다. 세 코어 실행 |
| singleThread | 통과 | 세 코어가 멈춘 상태에서 CPU0 `singleThread` continue: CPU0만 실행, `allThreadsContinued: false`. 이어서 CPU1 일반 continue에 CPU1·CPU2 실행 |
| perfWindow | 통과 | CPU0 성능 창(`square`–`0x8021c024`)이 `compute` 1회에 24명령, CPU2의 잘못된 설정은 "`perfWindow` of core 2 is ignored", CPU1 프레임은 CPU1의 카운터 |
| 섹터 겹침 | 통과 | `demo_flash.elf`와 그 사본을 CPU0·CPU1에 준 launch가 쓰기 전에 거부되고 코어는 그대로 실행 |
| T-M13 | 통과 | 실행 중 폴링 1회: 코어별 6요청 24.8 ms → 묶음 1요청 11.5 ms(`poll_cost`) |
| T-M14 | 통과 | 코어 하나 설정 회귀 20묶음 통과 |
| T-M15 | 통과 | VS Code에서 설정 하나로: 호출 스택에 스레드 3개(`0-TC275`…`2-TC275`), 17행 BP에 CPU0 정지와 콜스택·변수, CPU1·CPU2 `external`, F10 step은 CPU0만, continue 뒤 BP 재적중, 스레드 행의 continue·pause가 그 코어(CPU0은 BP에 남음), Live Watch 코어별 묶음과 Select Cores |
| FR-M13 | 통과 | `liveWatch` `core` 0·1·2·없음, 코어 계속 실행. 확장 로직은 가짜 `vscode` 모듈로 9항목 |

단위 테스트: probe-rs 564(TriCore 69), probe-rs-tools 218, probe-rs-debug 113, probe-rs-infineon 56(mcd 65). clippy 경고 없음. 펄스 라우팅 뒤 회귀 20묶음, 한 세션 시험 전 모드, `dap_allstop`(세션 3개) 10/10, `dap_swbp allstop` 5/5 통과.

**시험 중 찾아 고친 것**

- T-M8 첫 시도에서 CPU0만 `breakpoint`였다. 그룹 run이 풀어 준 동료가 다음 폴링 전에 자기 BP에 다시 멈춰, 정지→정지로 보여 이벤트도 스택 갱신도 없었다. 마지막 정지의 (`DBGSR`, PC)를 기억해 달라지면 새 정지로 본다(`cb0e00d`).
- 그 감지가 늦게 본 정지에서도 그룹 멤버를 멈춰, 세션 3개 SW BP 시험(`dap_swbp allstop`)이 10번 중 4번 실패했다. 그 사이 새 run으로 다시 돌던 코어를 멈췄기 때문이다. 늦게 본 정지에서는 멤버를 멈추지 않는다(`cb0e00d`, 고친 뒤 10/10).
- `--connect-under-reset`가 TAS 프로브에 전달되지 않았다(`63d721a`).
- T-M15에서 F10이 CPU0이 아니라 CPU2를 step했다. VS Code는 정지 이벤트마다 그 스레드로 포커스를 옮기는데, BP 정지 뒤에 그룹 정지(`external`) 이벤트가 왔기 때문이다. 그 이벤트에 `preserveFocusHint`를 실었다(`0e7761c`).
- 그룹에 코어가 둘 이상이면 앱 리셋 뒤 장치가 잠겼다(TAS `DEVICE_CONNECT` 0x46). 레벨 라우팅이 리셋 뒤에도 남고, 부트 정지 상태의 CPU1이 라인 7을 활성으로 잡아 시작 소프트웨어를 돌던 CPU0을 멈췄다(DAP 없이 `sw_reset --group 2`로도 재현). 실험으로 `CBS_TRCn` 7:4가 break-out 펄스 경로임을 확인하고 펄스 라우팅으로 바꿨다(`d7d32e7`). 복구 수단인 `--connect-under-reset`도 TAS 프로브에 전달되게 고쳤다(`63d721a`).
- 펄스 라우팅에서 정지 요청(pause, attach)은 펄스를 만들지 않는다. 그래서 (1) 세션 3개 `dap_allstop`이 6번 중 2번 실패했다. CPU1 pause 뒤 CPU2가 잠깐 더 도는 사이에 "함께 돈 동료" 목록에서 CPU1이 빠졌다. 목록을 run마다 새로 시작해 누적하게 고쳤다. (2) 정지한 코어가 그룹에 들어갈 때 도는 멤버를 소프트웨어로 멈추게 했다. 멈추지 않으면 다른 코어가 멈춘 동안 도는 코어가 워치독 서비스를 놓쳐 SMU 리셋이 났다(모두 `d7d32e7`).
- 강제 종료된 세션의 트리거 유닛이 남아 다음 세션 뒤에도 코어가 멈췄다(코어 하나 세션도 같음). 저널에 넣었다(`84a45aa`).
- 데이터 watchpoint에 break-out을 켜자 `dap_watch allstop`이 5번 중 4번 실패했다. `start_in_flash`의 정지가 남긴 `PREVSUSP`가 정지 요청으로는 바뀌지 않아, 그룹 가입 때 요청으로 멈춘 CPU0이 "자기 watchpoint에 멈춤"으로 보여 함께 실행되지 않았다. 걸린 watchpoint가 있을 때만 그렇게 보게 고쳤다(`ca86354`, 고친 뒤 10/10).

**설계와 다른 점**

- continue 응답과 continued 이벤트의 `allThreadsContinued`는 코어가 여럿이면 늘 `false`다. 실행된 코어마다 continued 이벤트가 따로 간다.
- setInstructionBreakpoints는 코어 전부에 건다(5.1의 "그 주소의 코드를 가진 ELF의 코어"가 아님). 트리거 유닛은 코어마다 하나씩 쓴다.
- 상태 묶음은 새 RPC `core/status_many` 대신 기존 `cores/status`가 `Session::status_many`를 쓴다.
- 디버그 정보는 ELF 경로별이 아니라 코어별이다. 첫 코어와 같은 파일이면 세션 것을 쓰고, 다른 파일이면 코어마다 읽는다.
- 일부 소스 위치 조회(`source_location`)는 첫 코어의 디버그 정보를 쓴다.
- 구현하지 않은 것: FR-M15(RTT·semihosting 코어 구분).
- 다른 아키텍처도 코드상 코어 여러 개를 받지만 실기 시험은 TriCore만 했다.

**알려진 제한**

- BP·`debug` 명령·데이터 watchpoint의 정지는 하드웨어 펄스로 나머지를 바로 멈춘다. pause(정지 요청)는 펄스가 없어 세션이 같은 요청 안에서 나머지를 소프트웨어로 멈춘다.
- `dap_watch allstop`(세션 3개, 데이터 watchpoint)에서 드물게 SMU(워치독) 리셋이 난다. 바꾸기 전 빌드와 새 빌드가 각각 40번 중 1번으로 같다(세션들이 붙는 동안, CPU0 세션 혼자 붙은 직후). 이번 변경과 관계없는 시험 환경의 문제다. 또 RAM 데모를 쓰는 시험(`dap_kill`) 바로 뒤에 리셋 없이 돌리면 `dap_watch allstop`이 매번 "CPU0이 저장 직후 PC에 없음"으로 실패한다. `start_in_flash`가 CPU0의 문맥 저장 영역(CSA) 목록을 새로 만들지 않아, 호출의 문맥 저장이 감시 주소를 덮어써 그 자리에서 멈추기 때문이다(그룹 run은 CPU0을 그대로 둔다). 사이에 `probe-rs reset`을 넣으면 통과한다.
- 이전 버전이 남긴 레벨 라우팅(`CBS_TRCn` 0x700007)이 세션 없는 코어에 남아 있으면 엉뚱한 external 정지가 날 수 있다. 버전을 바꾼 뒤 한 번 `probe-rs reset`한다.
- VS Code의 Stop(disconnect)은 멈춰 있던 코어를 멈춘 채 둔다(업스트림과 코어 하나 세션도 같다). 끝내기 전에 continue한다.

**TC36x 실기** (TriBoard TC3X5, `bf53965` 커밋 메시지 기준): CPU0–CPU1 한 세션을 `allStop` 켜고 끈 두 경우로, 플래시 BP·콜스택·liveWatch·pause·다른 BP까지 continue를 CPU0에서 확인했다. 시험 보드의 CPU1은 앱이 시작하지 않아 부트 정지 상태라 잠든 스레드로 보이고 pause가 거절된다. 이 변경 뒤 TC275에서도 회귀 20묶음과 한 세션 시험 7모드가 통과했다.

**남은 작업**: FR-M15(다른 아키텍처용 RTT·semihosting 코어 구분). TC36x에서 두 코어가 모두 도는 앱으로 동시 정지를 보는 시험은 그런 시험 이미지가 있을 때 한다.

## 1. 개요

VS Code에서 TC275의 코어 3개를 DAP 세션 하나로 디버깅한다. `probe-rs dap-server` 하나, launch 설정 하나로 띄우고, 코어마다 스레드 하나가 호출 스택 뷰에 보인다. 지금은 코어마다 세션을 하나씩 띄워 compound launch로 묶는다.

| 성공 기준 | 확인 방법 |
| --- | --- |
| 설정 하나 | `launch.json` 항목 하나(`coreConfigs` 3개)로 CPU0–CPU2를 디버깅한다 |
| 코어마다 스레드 | 호출 스택 뷰에 스레드 3개(CPU0, CPU1, CPU2)가 보이고, 스레드마다 자기 콜스택과 변수를 보인다 |
| 동시 정지 | 한 코어가 멈추면 세 스레드가 모두 멈춤으로 바뀌고, continue는 그룹을 함께 실행한다. 지금의 `dap_allstop` 시험 항목과 같은 결과 |
| 폴링 부하 감소 | 실행 중 상태 확인의 TAS 요청 수가 세션 3개 방식보다 적다(코어 상태를 한 요청에 묶음) |
| 회귀 없음 | 코어 하나 설정은 지금과 같게 동작한다. TC275 하드웨어 회귀 시험 20묶음 통과, 다른 아키텍처의 단위 테스트 회귀 없음 |

대상 코드는 [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore)(`b667e44`)의 DAP 서버(`probe-rs-tools/src/bin/probe-rs/cmd/dap_server/`)와 데모의 VS Code 설정(`m25_tcf/examples/aurix_tcf_demo/.vscode`)이다. 다른 개발지시서와 같이 업스트림에는 올리지 않는다.

## 2. 현황과 문제

백엔드와 RPC 서버는 이미 코어 인덱스로 동작하고, 막힌 곳은 DAP 계층이다. DAP 서버는 코어를 하나만 고르도록 짜여 있다. `coreConfigs`가 둘 이상이면 attach가 실패한다.

지금 방식(코어마다 DAP 서버 하나)의 한계는 다섯 가지다.

- **설정과 연결:** launch 설정 3개에 compound가 필요하고, TAS 연결 3개가 각자 코어 상태를 확인한다. 링크는 요청당 1~3 ms라 폴링 3배가 반응성에 드러난다.
- **정지 알림:** 다른 코어의 정지는 각 세션이 폴링으로 알아채서 40~60 ms 늦게 온다.
- **세션 사이 경쟁:** 세션들이 같은 코어를 서로 다른 시점에 읽는다. `58063ae`에서 고친 동시 정지 버그가 그 예다.
- **브레이크포인트:** 같은 코드를 여러 코어가 실행하면 세션마다 따로 걸어야 한다.
- **리셋과 종료:** 한 세션의 리셋을 다른 세션이 따로 알아채야 하고(TAS `DEV_RESET` 재전송 포함), 정리도 세션별로 한다.

DAP 서버 코드의 단일 코어 전제는 아래와 같다(`probe-rs-tools/src/bin/probe-rs/cmd/dap_server/` 기준).

| 위치 | 지금 | 한 세션에 필요한 것 |
| --- | --- | --- |
| `server/session_data.rs:1054` | `coreConfigs`가 1개가 아니면 attach 실패 | N개 허용 |
| `server/debugger.rs:242` | `let core_id = 0;` 모든 요청이 첫 코어로 간다 | `threadId`·`frameId`·`variablesReference`로 코어를 정한다 |
| `threadId` | continue, pause, next, stepIn, stepOut, stackTrace 어느 것도 읽지 않는다 | 읽어서 코어를 고른다 |
| `threads` | 스레드 하나(id = 코어 인덱스) | 코어마다 하나 |
| 정지·실행 이벤트 | stopped는 thread = 코어 인덱스, continued는 `core_data` 위치(지금도 CPU1·CPU2 세션이 thread 0으로 보내는 버그). `allThreadsStopped`는 전역 플래그 | 코어별 상태로 보내고, 그룹 여부는 `allStop`으로 |
| `all_cores_halted` | 전역 플래그. 폴링 순서에 따라 값이 달라지고, 스택 다시 풀기의 기준이다 | 코어별 이전 상태로 판단 |
| 디버그 정보 | 세션에 하나(첫 코어의 `programBinary`) | 코어마다(같은 ELF면 공유) |
| 소스 BP 해석 | 세션 수준 `resolve_source_breakpoints` | ELF별로 해석해 코어마다 건다 |
| launch·attach·restart | 첫 코어만: 플래시, `entryFunction`, `allStop`, `stepInterrupts`, `resetBehavior`, `perfWindow`, SVD | 코어 전부 |
| RTT·semihosting | RTT는 첫 RTT 코어만. semihosting 핸들 번호가 코어마다 1024부터 | 코어 구분(TriCore는 RTT·semihosting을 쓰지 않아 1차는 검사만) |

이미 되는 것도 많다.

- 백엔드(`backend/rpc.rs`)는 호출마다 코어 인덱스를 받는다. status, halt, run, step, BP, 레지스터, 변수, 동시 정지, TriCore 설정이 모두 그렇다.
- RPC 서버의 변수 상태는 코어별(`per_core`)이고, frame id와 `variablesReference`는 프로세스 전역 카운터라 코어끼리 겹치지 않는다.
- 코어 상태 확인(`poll_cores`)은 이미 모든 코어를 돈다. `CoreData`도 BP와 스택 캐시를 코어별로 갖는다.
- TriCore 코어 상태(`TriCoreCoreState`)는 코어별이고, 동시 정지는 하드웨어 라인으로 동작한다. 세션이 하나가 되면 트리거 유닛을 누가 맡는지 다투는 일이 없어진다.

## 3. 범위

1차는 DAP 서버 하나가 `coreConfigs`의 코어를 모두 맡는 것까지다. 코어별 세션 방식은 그대로 두고, 새 방식을 더한다.

| 구분 | 항목 |
| --- | --- |
| 1차 | 코어마다 스레드 하나. `threadId`·`frameId`·`variablesReference`로 요청을 코어에 나눠 보낸다 |
| 1차 | 정지·실행 이벤트의 스레드 구분. `allStop`이면 그룹 단위(`allThreadsStopped`, `allThreadsContinued`), 아니면 코어 단위 |
| 1차 | 브레이크포인트를 코어별로. 소스 BP는 그 소스의 코드를 가진 코어(ELF)마다 건다 |
| 1차 | 코어 상태 확인을 한 TAS 요청으로 묶음 |
| 1차 | 리셋, restart, disconnect, 강제 종료 복구를 코어 전부에 |
| 1차 | 데모의 VS Code 설정(세션 하나로 CPU0–CPU2) |
| 나중 | TC36x(2코어) 실기 시험. 보드가 있을 때 (2026-10-24 `bf53965`로 함) |
| 나중 | Live Watch 확장의 코어 선택, 성능 창(`perfWindow`)의 코어별 설정 표시 |
| 제외 | 업스트림 PR, 코어별 세션 방식 제거, 서로 다른 칩 여러 개를 한 세션에서(보드 여러 개) |

## 4. 기능 요구사항

Must는 1차 완료에 필요한 것, Should는 가능하면 1차에 넣을 것, Could는 나중에 해도 되는 것이다.

| ID | 내용 | 우선순위 |
| --- | --- | --- |
| FR-M1 | `coreConfigs`에 코어를 여러 개 준다. 코어 하나 설정은 지금과 같게 동작한다 | Must |
| FR-M2 | `threads`는 코어마다 스레드 하나를 돌려준다. id는 코어 인덱스, 이름은 `CPU0` 같은 코어 이름이다 | Must |
| FR-M3 | 요청을 코어에 나눠 보낸다. `threadId`(continue, pause, next, stepIn, stepOut, stackTrace), `frameId`(scopes, evaluate), `variablesReference`(variables, setVariable), 변수에서 나온 `memoryReference`와 `dataId`는 그 코어로 간다 | Must |
| FR-M4 | 정지·실행 이벤트는 코어마다 그 코어의 스레드로 보낸다. 이유도 코어별이다(BP를 맞은 코어는 `breakpoint`, 그룹이 멈춘 코어는 `external`). continued 이벤트의 스레드 번호 버그(`core_data` 위치를 보냄)도 고친다 | Must |
| FR-M5 | `allStop`이면 continue가 그룹을 함께 실행하고(`allThreadsContinued: true`), pause는 그룹을 멈춘다. step은 그 코어만 움직이고 나머지는 멈춘 채 둔다(지금과 같음) | Must |
| FR-M6 | `allStop`이 아니면 코어가 따로 돈다. continue·pause는 그 스레드의 코어만(`allThreadsContinued: false`) | Must |
| FR-M7 | 디버그 정보를 코어별 `programBinary`로 읽는다. 같은 파일이면 한 번만 읽고 공유한다 | Must |
| FR-M8 | 소스 BP는 그 줄을 포함하는 ELF의 코어마다 건다. 명령어 BP는 그 주소의 코드를 가진 코어에, 데이터 BP는 `dataId`의 코어에 건다. BP id는 코어가 달라도 겹치지 않는다 | Must |
| FR-M9 | launch·attach·restart가 코어 전부에 적용된다. 서로 다른 `programBinary`는 한 번씩 플래시하고, `entryFunction`, `stepInterrupts`, `resetBehavior`, `perfWindow`, SVD는 코어별 설정대로 쓴다. `allStop`이면 코어 전부가 그룹에 들어간다 | Must |
| FR-M10 | TriCore는 코어 상태 확인을 TAS 요청 하나로 묶는다(코어마다 `DBGSR`, STM 타이머는 한 번). 다른 아키텍처는 지금처럼 코어마다 확인한다 | Must |
| FR-M11 | disconnect와 강제 종료 복구가 코어 전부를 정리한다(동시 정지 라우팅 복원, `debug` 명령 되돌리기) | Must |
| FR-M12 | 코어 하나 설정과 다른 아키텍처의 동작은 바꾸지 않는다. 기존 단위 테스트와 TC275 회귀 시험이 그대로 통과한다 | Must |
| FR-M13 | `liveWatch`가 코어를 받는다(없으면 첫 코어). Live Watch 확장에 코어 선택 | Should |
| FR-M14 | 데모의 VS Code 설정에 세션 하나 구성(`TC275: probe-rs CPU0–CPU2 one session`)을 더한다. compound 구성은 남긴다 | Should |
| FR-M15 | RTT와 semihosting을 코어별로 구분한다(다른 아키텍처용. TriCore는 쓰지 않음) | Could |

## 5. 설계

DAP 서버 하나가 요청을 코어별로 나눠 보내고, 코어 상태는 한 요청으로 확인한다. 새로 만드는 것은 요청 라우터, 그룹 조정, 상태 확인 묶음 셋이다. 나머지(코어별 `CoreData`, 백엔드, TriCore 코어 상태)는 지금 것을 쓴다.

구조 그림(원본 문서)의 층은 위에서 아래로 다음과 같다.

1. **VS Code (DAP 클라이언트):** 호출 스택 뷰에 스레드 3개(CPU0 · CPU1 · CPU2).
2. **probe-rs dap-server (세션 하나, launch 설정 하나):**
   - 새로 만드는 부분은 세 가지다.
     - 요청 라우터: threadId → 그 코어, frameId·변수 → 발급한 코어, 주소만 있으면 → 포커스 코어.
     - 그룹 조정(allStop): continue·pause를 그룹에, 멈춘 채 남는 코어는 보고, step은 그 코어만.
     - 상태 확인 묶음: 코어 3개를 한 TAS 요청으로, 바뀐 코어마다 그 스레드로 stopped·continued.
   - 지금 것을 쓰는 부분은 `CoreData` CPU0·CPU1·CPU2다. 각각 ELF·디버그 정보, BP·스택 캐시·상태를 갖는다.
3. **RPC 백엔드 · probe-rs Session (TAS 연결 하나):** 코어 인덱스로 호출(status · run · step · BP · 변수). `TriCoreCoreState` × 3이 트리거 유닛, 동시 정지, 리셋 감지, 저널을 맡는다.
4. **TasServer → TC275:** CPU0 · CPU1 · CPU2, OCDS 트리거 스위치(동시 정지 라인). 이 층과는 TAS 요청으로 통신하고, 상태 확인은 한 요청에 묶는다.

요청은 위에서 아래로 흐른다. 라우터가 코어를 정하고, 백엔드는 지금처럼 코어 인덱스로 호출하며, TAS 연결은 하나다.

### 5.1 요청 라우팅

| 요청 | 코어를 정하는 방법 |
| --- | --- |
| continue, pause, next, stepIn, stepOut, stackTrace | `threadId`. 스레드 id는 코어 인덱스다 |
| scopes, evaluate(`frameId` 있음) | 그 `frameId`를 내준 코어. stackTrace 응답 때 frame id와 코어를 표에 적어 둔다 |
| variables, setVariable | 그 `variablesReference`를 내준 코어. scopes·variables 응답 때 적어 둔다 |
| 변수에서 나온 `memoryReference`, `dataId` | 문자열에 코어를 붙인다(`0x7000a004@1`, `0x7000a004/4@1`) |
| evaluate(`frameId` 없음: 호버, Watch), `liveWatch`, 주소만 있는 readMemory·writeMemory·disassemble | 포커스 코어: 마지막으로 정지 이벤트를 보낸 코어, 없으면 첫 코어. `liveWatch`는 `core` 인자를 받는다 |
| setBreakpoints | 그 소스를 포함한 ELF의 코어 전부 |
| setInstructionBreakpoints | 그 주소의 코드를 가진 ELF의 코어, 없으면 포커스 코어 |
| threads, configurationDone, restart, disconnect | 코어 전부 |

### 5.2 정지·실행과 이벤트

- **이벤트:** 상태 확인에서 상태가 바뀐 코어마다, 그 코어의 스레드로 stopped(이유는 코어별) 또는 continued를 보낸다. 각 코어의 이유를 보이려고 `allThreadsStopped`는 쓰지 않는다.
- **continue:** 그 코어를 실행한다. `allStop`이면 TriCore의 run이 그룹이 멈춘 동료를 같은 TAS 요청으로 함께 실행한다(지금 동작). 자기 BP에 멈춘 동료는 남는다. 응답의 `allThreadsContinued`는 코어가 모두 돌 때만 `true`이고, 남은 코어의 스레드는 멈춤으로 둔다.
- **pause:** 그 코어를 멈춘다. `allStop`이면 하드웨어가 나머지를 멈추고, 상태 확인이 그 스레드들에 stopped(`external`)를 보낸다.
- **step:** 그 코어만 움직이고 그 스레드에만 stopped(`step`)를 보낸다.
- **`allStop`이 아닐 때:** continue·pause는 그 코어만 다루고 `allThreadsContinued: false`로 답한다. capability `supportsSingleThreadExecutionRequests`를 켜고 `singleThread` 인자를 따른다.
- **스택 다시 풀기:** 전역 플래그 대신 그 코어의 이전 상태가 실행에서 정지로 바뀔 때만 한다.

### 5.3 상태 확인 묶음

- 백엔드에 여러 코어의 상태를 한 번에 묻는 호출(`status_many`, RPC `core/status_many`)을 더한다. 기본 구현은 코어마다 `status`를 부르므로 다른 아키텍처는 지금과 같다.
- TriCore는 이를 TAS 요청 하나로 처리한다. 코어마다 `DBGSR`, STM0 타이머는 한 번 읽는다. 그 값으로 코어별 판단(정지 이유, 리셋 감지, 동시 정지 확인)을 하고, 상태가 바뀐 코어만 추가로 읽는다.
- 폴링 주기는 그대로다(정지 중 100 ms, 실행 중 50 ms, 입력이 오면 바로 끝남). 목표는 실행 중 폴링 1회에 TAS 요청 1개다. 지금은 세션마다 상태 요청과 그룹 확인 요청을 따로 보낸다.

### 5.4 디버그 정보와 브레이크포인트

- RPC 서버의 디버그 정보를 ELF 경로별로 둔다. 코어는 자기 ELF를 가리키고, 같은 경로면 공유한다. `resolve_source_breakpoints`와 `resolve_source_locations`는 ELF(코어)를 받는다.
- 소스 BP 하나는 DAP 브레이크포인트 하나로 답하고, 안에서 코어마다 트리거 유닛을 건다. `verified`는 한 코어라도 걸리면 `true`이고, 걸린 코어는 `message`에 적는다. id는 소스와 줄로 만들어 코어가 달라도 겹치지 않는다.
- 트리거 유닛은 코어마다 8개라서, 세 코어에 걸어도 각 코어에서 하나씩만 쓴다.

### 5.5 launch·attach·restart·disconnect

- 코어 설정마다 `entryFunction`, `stepInterrupts`, `resetBehavior`, `perfWindow`, SVD를 적용한다. `allStop`은 세션 설정이라 코어 전부를 그룹에 넣는다.
- 플래시는 서로 다른 `programBinary`만 한 번씩 쓴다. 두 ELF가 같은 섹터를 쓰면 쓰기 전에 거부한다.
- restart는 장치를 한 번 리셋하고, 그 뒤 코어마다 BP, 그룹, 설정을 되살린다.
- disconnect는 코어마다 BP를 지우고, 그룹에서 빼고(라우팅 복원), 실행 상태를 되돌린다. 강제 종료 복구는 코어별 저널이 이미 맡는다.

### 5.6 TriCore 쪽 변경

- 5.3의 상태 묶음 함수를 더한다.
- 나머지는 코어별 상태(`TriCoreCoreState`)를 그대로 쓴다. 트리거 소유는 코어마다 첫 BP·step 때 넘겨받는 지금 규칙 그대로이고, 한 세션이 세 코어를 모두 맡게 된다.
- 동료 재개 규칙(`running_peers`)도 그대로다. 폴링이 하나라서 세션 사이의 경쟁은 없어진다.

### 5.7 호환

- `coreConfigs`가 하나면 지금 경로 그대로 동작한다. `threadId`가 없거나 코어가 하나면 그 코어로 간다.
- 코어별 세션 방식(compound)도 계속 쓸 수 있다.

## 6. 작업 패키지

일곱 패키지를 WP-M1 → (WP-M2, WP-M3) → WP-M4 → WP-M5 → WP-M6 → WP-M7 순서로 한다. WP-M2와 WP-M3은 WP-M1 뒤에 나란히 할 수 있다. 크기는 다른 개발지시서와 같이 소(며칠), 중(1~2주)로 잡았다.

| WP | 내용 | 선행 | 크기 | 완료 조건 |
| --- | --- | --- | --- | --- |
| WP-M1 | 코어 여러 개 설정 허용, `CoreData` 코어별 구성, `threads`, 요청 라우터(threadId·frameId·variablesReference 표, 포커스 코어), continued 스레드 번호 버그 | — | 중 | 가짜 어댑터 테스트: 코어 2개 설정에서 threads, stackTrace, scopes, variables가 맞는 코어로 간다. 코어 하나 설정 테스트 회귀 없음 |
| WP-M2 | 코어별 이벤트, continue·pause·step의 `allStop` 의미, `singleThread`, 코어별 스택 다시 풀기 | WP-M1 | 중 | 가짜 백엔드 테스트: 그룹 정지에 스레드 3개가 각자 이유로 stopped, 자기 BP에 남은 코어는 continued를 받지 않는다 |
| WP-M3 | ELF별 디버그 정보(RPC 서버), ELF별 소스 해석, 소스 BP의 코어별 확장과 id | WP-M1 | 중 | 같은 ELF를 쓰는 코어 3개에 소스 BP 하나가 각 코어의 트리거 유닛에 걸리고, DAP 응답은 하나다 |
| WP-M4 | launch·attach·restart·disconnect를 코어 전부에: 코어별 설정, 플래시 중복 제거와 섹터 겹침 거부, `allStop` 전 코어, 정리 | WP-M1, WP-M3 | 중 | restart 뒤 코어 3개의 BP·그룹·설정이 되살아나고, disconnect 뒤 라우팅과 코드가 복원된다 |
| WP-M5 | 상태 확인 묶음: 백엔드 `status_many`, RPC `core/status_many`, TriCore 한 요청 구현 | WP-M2 | 중 | 가짜 버스에서 코어 3개 폴링이 요청 1개다. 실기에서 실행 중 폴링의 TAS 요청 수를 세션 3개 방식과 비교한다 |
| WP-M6 | 데모 VS Code 설정(세션 하나 구성), `liveWatch`의 코어 인자와 Live Watch 확장의 코어 선택 | WP-M2, WP-M4 | 소 | VS Code에서 설정 하나로 CPU0–CPU2 디버깅 |
| WP-M7 | 회귀 시험과 문서 | 전부 | 소 | 7절의 시험 통과, 구현 현황을 PRD에 반영 |

## 7. 시험 계획

단위 테스트로 라우팅과 이벤트를 확인하고, TC275 ShieldBuddy에서 지금의 `dap_allstop` 시나리오를 세션 하나로 다시 돌린다. 플래시는 승인받은 PF1 시험 섹터만 쓴다.

**단위 테스트**

- 가짜 프로토콜 어댑터와 백엔드로: 코어 2–3개 설정의 threads, threadId·frameId·variablesReference 라우팅, 코어별 이벤트, `allThreadsContinued` 값.
- 코어 하나 설정의 기존 테스트 회귀 없음(probe-rs-tools 202개).
- 가짜 TriCore 버스로: 코어 3개 상태 확인이 요청 1개이고, 코어별 판단(정지 이유, 리셋, 동시 정지)이 지금과 같다.

**실기 시험** (TC275, `coreConfigs` 3개, 새 시험 스크립트 `dap_onesession.js`)

| ID | 시험 | 기대 결과 | FR |
| --- | --- | --- | --- |
| T-M1 | attach | 스레드 3개, 앱은 계속 돈다 | M1, M2 |
| T-M2 | CPU0 BP에 정지(`allStop`) | CPU0 `breakpoint`, CPU1·CPU2 `external`. 스레드마다 stackTrace가 자기 코어의 PC를 보인다 | M3, M4 |
| T-M3 | CPU0에서 continue | 세 코어 실행, continued 3개, `allThreadsContinued: true` | M5 |
| T-M4 | CPU1에서 pause | 세 코어 정지, CPU1 `pause`, 나머지 `external` | M5 |
| T-M5 | CPU0 step | CPU0만 움직이고 CPU1·CPU2의 PC는 그대로 | M5 |
| T-M6 | CPU0이 자기 BP에 멈춘 채 CPU1에서 continue | CPU1·CPU2 실행, CPU0 남음, `allThreadsContinued: false` | M5 |
| T-M7 | `allStop` 끈 설정 | continue·pause가 그 코어만 | M6 |
| T-M8 | 세 코어가 같은 코드를 실행할 때 소스 BP 하나 | 코어마다 트리거 유닛 하나, 어느 코어가 맞아도 그 스레드가 `breakpoint` | M7, M8 |
| T-M9 | CPU1 프레임의 변수·evaluate·setVariable | CPU1의 레지스터와 CSA를 직접 읽은 값과 같다 | M3 |
| T-M10 | 코어를 붙인 `dataId`의 데이터 BP | 그 코어에서만 정지 | M3, M8 |
| T-M11 | restart, 앱의 SW 리셋 | 세 코어의 BP와 그룹 유지, 콘솔 알림 | M9 |
| T-M12 | disconnect, DAP 서버 강제 종료 뒤 다음 세션 | 라우팅 복원, 세 코어 실행, `debug` 명령 되돌림 | M11 |
| T-M13 | 실행 중 폴링의 TAS 요청 수 | 폴링 1회에 요청 1개. 세션 3개 방식과 비교해 기록 | M10 |
| T-M14 | 코어 하나 설정의 하드웨어 회귀 시험 20묶음 | 모두 통과 | M12 |
| T-M15 | VS Code에서 설정 하나로 디버깅 | 호출 스택 뷰에 스레드 3개, BP·step·continue | M14 |

데모에서는 CPU1·CPU2가 PF0의 앱을 실행해 데모 ELF에 그 코드의 기호가 없다. 그래서 T-M2의 CPU1·CPU2 콜스택은 PC 주소로 확인하고, T-M8에는 세 코어가 같은 코드를 실행하는 시험 이미지가 필요하다(9절).

## 8. 위험과 대응

가장 큰 위험은 공통 DAP 코드를 크게 바꾸는 데서 오는 회귀다. 그래서 새 동작은 코어가 둘 이상일 때만 켜지게 한다.

| 위험 | 영향 | 대응 |
| --- | --- | --- |
| 공통 DAP 코드 변경이 크다 | 다른 아키텍처와 코어 하나 설정의 동작이 바뀔 수 있다 | 코어 하나면 지금 경로를 그대로 탄다. 기존 단위 테스트와 TC275 회귀 시험을 패키지마다 돌린다 |
| VS Code가 스레드 여러 개의 이벤트를 예상과 다르게 보인다(포커스 이동, 멈춤 표시) | 화면이 실제 상태와 다르다 | T-M15를 WP-M2 직후에 먼저 해 보고 이벤트 규칙을 맞춘다 |
| 폴링 묶음이 TriCore 판단의 읽기 순서를 바꾼다 | `58063ae` 같은 동시 정지 경쟁이 다시 생길 수 있다 | 자기 `DBGSR`을 동료 뒤에 읽는 규칙을 지킨다. 가짜 버스 테스트로 요청 사이 정지를 재현한다 |
| 코어별 ELF가 같은 PFLASH 섹터를 쓴다 | 앞에 쓴 이미지가 지워진다 | 쓰기 전에 섹터 겹침을 검사해 거부한다 |
| 세션 하나가 끝나면 세 코어의 디버깅이 함께 끝난다 | 한 코어의 오류가 전부에 퍼진다 | 오류는 요청 단위로 답하고 세션을 끝내지 않는다(지금 동작). 코어별 세션 방식을 남겨 둔다 |
| 업스트림과 갈라진다 | 업스트림을 받을 때 병합 비용이 커진다 | 변경을 라우터와 새 파일에 모으고, 업스트림의 멀티코어 작업(코드의 TODO)을 지켜본다 |

## 9. 결정이 필요한 점

각 항목에 권장안을 적었다. 권장안대로 가면 WP-M1부터 바로 시작할 수 있다. 2026-10-24에 모두 권장안으로 정해 구현했다(0절).

- [x] **정지 이벤트 방식:** 코어마다 stopped를 보낼지, 하나에 `allThreadsStopped`를 실을지. 권장은 코어마다다. 각 코어의 이유(`breakpoint`, `external`, `step`)가 화면에 남는다.
- [x] **포커스 코어:** 스레드를 알 수 없는 요청(호버, Watch, 주소만 있는 메모리 요청)을 어느 코어로 보낼지. 권장은 마지막으로 정지 이벤트를 보낸 코어다. VS Code도 그 스레드에 포커스를 둔다.
- [x] **코어 표시 형식:** `memoryReference`와 `dataId`에 코어를 붙이는 방법. 권장은 `0x7000a004@1`처럼 끝에 `@코어`다. 붙지 않은 값은 포커스 코어로 간다.
- [x] **T-M8 시험 이미지:** 세 코어가 같은 코드를 실행하게 하는 방법. 권장은 `start_in_flash` 예제에 코어 선택을 더해 PF1 데모를 CPU1·CPU2에서도 시작하는 것이다(코어마다 자기 DSPR에 스택). 플래시는 쓰지 않고 PF0는 건드리지 않는다.
- [x] **다른 아키텍처:** 코어 여러 개 설정을 ARM·RISC-V에도 열지. 권장은 코드상 허용하되 실기 시험은 TriCore만 하고, 그 사실을 문서에 적는 것이다.
- [x] **Live Watch 코어 선택(FR-M13):** 1차에 넣을지. 권장은 넣는 것이다. 크기가 작고, 세션이 하나가 되면 Live Watch가 코어를 고를 방법이 따로 없다.

## 10. 참고 자료

- [Debug Adapter Protocol 사양](https://microsoft.github.io/debug-adapter-protocol/specification): StoppedEvent의 `allThreadsStopped`(참이면 클라이언트가 모든 스레드를 펼칠 수 있고, 아니면 `threadId`의 스레드만), ContinuedEvent와 continue 응답의 `allThreadsContinued`(빠지면 모든 스레드 재개), continue·next·stepIn·stepOut의 `singleThread`(capability `supportsSingleThreadExecutionRequests`), `frameId` 없는 evaluate는 전역 범위
- [probe-rs TriCore · MCD · MCDS 연동 개발지시서 (PRD)](PROBE_RS_TRICORE_PRD.md): 동시 정지(`allStop`), 코어별 세션, `58063ae`와 `b667e44`
- [probe-rs TriCore vs aurix-tcf 확장 기능 비교](PROBE_RS_VS_AURIX_TCF.md): aurix-tcf는 세션 하나로 멀티코어를 다룬다
- [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore) `b667e44`: `probe-rs-tools/src/bin/probe-rs/cmd/dap_server/`(server/debugger.rs, server/session_data.rs, debug_adapter/dap/adapter.rs, backend/rpc.rs), `probe-rs/src/architecture/tricore/mod.rs`
- 데모 설정: `m25_tcf/examples/aurix_tcf_demo/.vscode/launch.json`(코어별 구성과 compound)
- 지금의 멀티코어 실기 시험: `dap_allstop.js`(세션 3개, 14항목)
