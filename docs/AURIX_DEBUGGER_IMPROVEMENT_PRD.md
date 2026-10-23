# aurix\_debugger 보완 개발지시서 (PRD)

2026-10-23 · 원본: [Claude 문서](https://claude.ai/artifact/XuL2SVz4V9ky3NZgHLggUw)

## 개요

TRACE32와 비교해 모자란 디버깅 기능 가운데, 지금 보드(TC275 ShieldBuddy)에서 바로 만들 수 있는 네 가지를 구현한다.

1. 데이터 브레이크포인트(읽기·쓰기 감시)
2. step 중 인터럽트 막기
3. 성능 카운터
4. 앱이 일으킨 리셋의 처리

대상 코드는 [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore)(현재 `ad00ecc`)와 [najari/aurix\_debugger](https://github.com/najari/aurix_debugger)다. 네 기능이 VS Code와 GDB에서 동작하고, 기존 실기 시험(동시 정지, SW BP, DAP, GDB)이 그대로 통과하면 1차 완료다.

배경은 [TRACE32 vs aurix\_debugger 비교](TRACE32_VS_AURIX_DEBUGGER.md)의 "보완 가능한 점"이다.

## 구현 현황

1차 네 기능(WP-10–WP-13)을 구현했고 실기 시험을 모두 통과했다(2026-10-23). WP-14(회귀 시험과 문서)도 마쳤다. 코드는 [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore)에 있다.

| WP | 커밋 | 실기 결과 |
| --- | --- | --- |
| WP-10 step 중 인터럽트 | `79dd08d` | T-20: 앱만으로는 50·200 step 동안 ISR 진입 0회(인터럽트 소스가 적음). T-21: SRPN 60을 매 step 대기시키면 `allow`는 10 step 모두 ISR(벡터 `0xA00F0784`), 기본값은 0회이고 `ICR.IE` 복원. T-22: disable·enable·nop·disable·nop에서 IE 0, 1, 1, 0, 0 |
| WP-11 데이터 BP | `6826341` | T-23–T-28 통과. `g_result` 쓰기 BP가 `st.w` 다음(`0x8021c090`)에서 6회 정지, `g_ticks` 읽기 BP, 해제 뒤 정지 없음, 유닛 부족 거부, `allStop`, GDB `watch`·`rwatch`·`awatch` |
| WP-12 성능 카운터 | `f0d9b30` | T-29: DAP에서 `compute` 사이 `icnt` +24(정확). T-30: `square` 창에서 `compute` 1회당 24.0 명령(30만 회 평균). T-31: 실행 중 읽기 |
| WP-13 리셋 처리 | `fbf362f` | T-32: SW 리셋은 디버그 설정(트리거 유닛, `EXEVT`, `SWEVT`, `CBS_TRCn`)을 그대로 둔다. T-33–T-35: 콘솔 알림, 시작 코드 BP 유지·적중, `halt` 모드 정지. T-36: PF1 재기록+리셋 10회 오류 0 |
| WP-14 회귀·문서 | — | 동시 정지 20, SW BP 18, DAP 10, GDB 11항목 통과. 단위 테스트 probe-rs 547(TriCore 52), tools 197, infineon 54 |

설계와 달라진 점은 다음과 같다.

- **리셋 감지(FR-R1)**: 표지 값 방식은 쓸 수 없었다. SW 리셋이 디버그 레지스터를 지우지 않기 때문이다. 대신 STM0 64비트 타이머(두 번째 보기 `TIM0SV`·`CAPSV`)가 줄어드는 것으로 감지한다. `DBGSR`과 같은 요청으로 읽는다.
- **콘솔 알림**: 코어의 로그는 DAP 콘솔에 닿지 않았다(콘솔 로그 계층이 DAP 스레드 전용). 그래서 알림 큐와 RPC `core/notices`를 더했다.
- **소스 줄 step(FR-I3)**: 별도 설정 없이, 소스 줄 step을 이루는 단일 step은 모두 막고 임시 BP까지 run하는 구간은 막지 않는다.
- **카운터 쓰기(FR-P1)**: DAP 레지스터 뷰는 읽기 전용이다. 쓰기는 GDB(`set $cctrl = ...`)나 probe-rs API로 한다.
- **0x44 오류(FR-R4)**: PF0에 리셋 이미지가 있을 때만 나왔고, 그 조건 없이는 재현되지 않았다. 리셋 뒤 순서를 재부팅 중 최대 2초 다시 시도하도록 했다.
- **데이터 BP 표현식(FR-D7)**: 주소(`asAddress`)는 되고, Watch 표현식은 아직 안 된다.
- **동시 정지와의 관계**: 그룹이 멈춘 코어를 함께 실행한 직후 이 코어가 데이터 BP에 바로 멈추면, 다른 코어는 그 세션이 확인하기 전에 다시 멈춘다. VS Code에서는 계속 멈춘 것으로 보인다.

실기 시험용 예제를 세 개 더했다. `step_interrupts`(step이 끝난 곳과 `ICR`), `perf_counters`(실행 중 카운터와 창 설정), `sw_reset`(앱처럼 SW 리셋 요청)이다.

## 범위

1차는 지금 장비로 만들고 실기로 확인할 수 있는 것만 담았다. 2차는 하드웨어나 시험 대상 앱이 생기면 따로 지시한다.

| 구분 | 항목 | 필요한 것 |
| --- | --- | --- |
| 1차 | 데이터 BP, step 중 인터럽트 막기, 성능 카운터, 리셋 처리 | 지금 보드와 어댑터 |
| 2차 | MCDS 트레이스 검증과 VS Code 표시 | 맵 1을 여는 어댑터(예: DAP miniWiggler) |
| 2차 | TC3xx 칩과 플래시(기존 WP-9) | TC3xx 보드 |
| 2차 | AUTOSAR OS 인식(ORTI) | ORTI를 내는 OS의 앱 |
| 제외 | 보조 코어(GTM, HSM, SCR, PPU), 커버리지, ISO 26262 도구 인증, 한 세션 멀티코어 | — |

업스트림 probe-rs에는 올리지 않는다. 공통 코드(DAP, GDB, `CoreInterface`)를 바꿀 때도 다른 아키텍처의 동작은 바꾸지 않는다.

## 기능 요구사항

Must는 1차 완료에 필요한 것, Should는 가능하면 1차에 넣을 것, Could는 나중에 해도 되는 것이다.

| ID | 내용 | 우선순위 |
| --- | --- | --- |
| FR-D1 | 데이터 BP를 주소와 길이(1·2·4·8바이트)로 건다. 접근 종류는 읽기, 쓰기, 읽기·쓰기 | Must |
| FR-D2 | VS Code 변수 뷰의 "Break on Value Change" 등으로 전역·정적 변수에 건다(DAP `dataBreakpointInfo`, `setDataBreakpoints`) | Must |
| FR-D3 | 멈추면 이유는 `data breakpoint`이고, PC는 접근한 명령의 다음 명령이다. continue하면 그대로 진행한다 | Must |
| FR-D4 | 트리거 유닛 8개를 프로그램 BP와 나눠 쓴다. 남는 유닛이 없으면 거부하고 이유를 알린다. PSPR 밖 프로그램 BP가 우선이다 | Must |
| FR-D5 | 동시 정지(`allStop`)와 함께 동작한다. 데이터 BP에 멈춘 코어는 다른 코어가 continue해도 남는다 | Must |
| FR-D6 | GDB `watch`·`rwatch`·`awatch`(`Z2`–`Z4`) | Should |
| FR-D7 | Watch 표현식과 임의 주소(메모리 뷰)에도 건다 | Could |
| FR-I1 | step 중 인터럽트를 막는 설정. 켜면 step하는 동안 `ICR.IE`를 끄고 끝나면 되돌린다 | Must |
| FR-I2 | step할 명령이 `ICR.IE`를 바꾸는 명령(ENABLE, DISABLE, BISR, RFE, RFM, ICR에 쓰는 MTCR)이면 막지 않는다. 프로그램이 바꾼 값을 덮어쓰지 않기 위해서다 | Must |
| FR-I3 | 소스 줄 단위 step(next, step in, step out)에도 같은 설정을 적용할지 고를 수 있다 | Should |
| FR-P1 | `CCTRL`, `CCNT`, `ICNT`, `M1CNT`–`M3CNT`를 레지스터 뷰에 보이고 쓸 수 있다 | Must |
| FR-P2 | 두 주소 사이 구간만 센다(A-to-B). 코어 설정에 시작·끝 심볼이나 주소를 준다 | Should |
| FR-P3 | 실행 중에 카운터를 읽는다(Live Watch 또는 CLI) | Should |
| FR-R1 | 앱이 일으킨 리셋(소프트웨어, SMU·워치독, ESR)을 감지하고 원인을 디버그 콘솔에 알린다 | Must |
| FR-R2 | 리셋 뒤 동작을 고른다: `halt`(멈추고 알림) 또는 `restore`(BP·데이터 BP·정지 그룹을 되살리고 계속 실행) | Must |
| FR-R3 | 리셋 뒤 세션이 끊기지 않는다. 트리거 유닛, SWEVT, 정지 그룹 라우팅을 다시 쓴다 | Must |
| FR-R4 | 플래시를 쓴 직후 리셋에서 가끔 나는 0x44 오류를 없앤다 | Should |

비기능 요구사항은 세 가지다.

- 기존 실기 시험이 모두 그대로 통과한다. 동시 정지 20항목, SW BP 18항목, DAP 10항목, GDB 11항목, `core0_main` 9항목이다.
- 상태 확인(폴링) 한 번에 드는 TAS 요청은 늘지 않는다. 읽을 값이 늘면 기존 요청에 묶는다.
- 공통 코드에 더하는 API는 기본 구현을 `NotImplemented`로 두어 다른 아키텍처의 동작을 바꾸지 않는다.

## 설계

네 기능 모두 TriCore 코어의 CSFR만 쓰고, 트리거 유닛 8개(`TR0`–`TR7`)를 나눠 쓴다. 유닛 배정을 한 곳(`TriCoreCoreState`)에서 관리하고, step이 트리거 창을 다시 쓴 뒤에는 모든 종류의 유닛을 되살린다(지금의 `rearm_triggers` 확장).

### 데이터 BP (FR-D)

- **레지스터**: `TRnEVT`의 `TYP`=0(주소), 읽기는 `ALD`(비트 28), 쓰기는 `AST`(비트 27), `EVTA`=010(정지), `BOD`=1, `BBM`=0(접근한 뒤 정지)이다. 값은 읽기 `0x1000_0012`, 쓰기 `0x0800_0012`, 읽기·쓰기 `0x1800_0012`로 잡고 실기로 확인한다.
- **길이**: 하드웨어는 접근 주소만 비교한다. 1바이트는 유닛 하나(같음 비교), 2바이트 이상은 짝수·홀수 유닛 한 쌍의 범위(`RNG`=1, `[주소, 주소+길이)`)로 건다. 범위 앞에서 시작해 걸쳐 들어오는 접근은 잡지 못한다.
- **정지 이유**: `TRIG_ACC`의 비트가 데이터 유닛이면 `HaltReason::Watchpoint`다. 이 이유는 probe-rs에 이미 있고 DAP에서 `data breakpoint`로 바뀐다. 접근한 뒤 멈추므로 continue할 때 step으로 넘어갈 필요가 없다.
- **코어 API**: `CoreInterface`에 `set_watchpoint(address, len, access)`와 `clear_watchpoint(address)`를 더하고, 기본 구현은 `NotImplemented`로 둔다(`probe-rs/src/core.rs`, `architecture/tricore/mod.rs`).
- **RPC**: `core/watchpoint` 엔드포인트를 더한다. `84c21eb`의 `core/halt_group`과 같은 경로다(`probe-rs-rpc`, `probe-rs-rpc-client`, `probe-rs-tools/src/rpc`).
- **DAP**: 초기화 응답에 `supportsDataBreakpoints`를 켜고, 아래 두 요청을 처리한다. 프로토콜 타입은 `debugProtocol.rs`에 이미 있다(`dap_server/debug_adapter/dap/adapter.rs`).
  - `dataBreakpointInfo`: 변수 캐시에서 주소와 크기를 찾아 `dataId`(예: `0x7000a004/4`)를 돌려준다.
  - `setDataBreakpoints`: 목록 전체를 바꾼다.
- **GDB**: `gdb_server/target/breakpoints.rs`에서 `HwWatchpointOps`를 구현한다.

### step 중 인터럽트 막기 (FR-I)

- **현재**: step은 TR0을 모든 주소 범위의 PC 트리거(`0x3002`)로 건다. 그래서 인터럽트가 대기 중이면 ISR의 첫 명령에서 멈출 수 있다. 실기로 먼저 확인한다.
- **막기**: `step_core`에서 아래 순서로 한다.
  1. `ICR`(`0xFE2C`)을 읽는다.
  2. `IE`(비트 15)가 켜져 있고 PC의 명령이 IE를 바꾸는 명령이 아니면 IE를 끈 뒤 step한다.
  3. step이 끝나면 IE를 다시 켠다.
  4. 판별할 명령의 인코딩은 `tricore-elf-objdump`로 확인해 표로 둔다.
- **소스 줄 step**: probe-rs-debug는 임시 BP를 두고 run한다. 설정이 켜져 있으면 그 run 동안에도 IE를 끈다. 어느 run이 step용인지 아키텍처가 알도록 `CoreInterface`에 표시를 더하거나, DAP에서 `ICR`을 직접 다룬다.
- **설정**: 코어 설정 `stepInterrupts`를 둔다. 값은 `mask`, `maskInstruction`(명령 step만), `allow`다.

### 성능 카운터 (FR-P)

- **레지스터**: `CCTRL`(`0xFC00`: `CM` 비트 0, `CE` 비트 1, `M1`–`M3` 선택), `CCNT`(`0xFC04`), `ICNT`(`0xFC08`), `M1CNT`–`M3CNT`(`0xFC0C`–`0xFC14`)다.
- **제약**: 카운터는 31비트이고, 비트 31은 넘침 표시다. 카운터는 `CE`=0일 때만 쓸 수 있다.
- **레지스터 뷰**: `registers.rs`의 레지스터 표에 "Performance" 묶음으로 더한다.
- **A-to-B**: 두 단계로 건다.
  1. `CCTRL.CM`=1(task 모드)에 `CE`=1로 둔다.
  2. PC 트리거 두 개를 건다. 둘 다 `EVTA`=001, `BOD`=1이라 멈추지 않는다. 시작 주소는 `CNT`=01(시작), 끝 주소는 `CNT`=10(멈춤)이다.
- **A-to-B 설정**: 코어 설정 `perfWindow: { start, stop }`으로 건다.
- **실행 중 읽기**: Live Watch에 `$CCNT` 같은 레지스터 이름을 받도록 하거나, `probe-rs-infineon` 예제(`perf_counters`)로 읽는다.
- **M1–M3**: 세는 사건은 구현마다 다르다. TC27x 사용자 매뉴얼의 CPU 장에서 확인해 이름을 붙인다.

### 리셋 처리 (FR-R)

- **감지**: 상태 확인 때 `DBGSR`만 읽던 것을 `0xFD00`–`0xFD13` 블록 읽기로 바꾼다. 이 블록에는 `DBGSR`, `EXEVT`, `CREVT`, `SWEVT`가 들어 있고, 요청 수는 늘지 않는다. 세션이 쓴 표지 값(`SWEVT`=`0x2A`)이 사라지면 리셋으로 본다.
- **먼저 측정**: 앱 리셋이 어느 디버그 레지스터를 지우는지 실기로 잰다. 대상은 `TRnEVT`, `SWEVT`, `EXEVT`, `CBS_TRCn`이다. TRACE32 매뉴얼은 소프트 리셋이 디버그 로직을 리셋하지 않는다고 한다. 표지가 남아 있으면 `SCU_RSTSTAT`(`0xF0036050`) 값의 변화와 부트 정지(CPU1·CPU2가 `0xAFFFC000`에 멈춤)를 함께 본다.
- **원인**: `SCU_RSTSTAT`의 `SW`, `SMU`(워치독 포함), `ESR0`·`ESR1`, `STM0`–`STM2`, `CB`(디버거), `PORST`를 디버그 콘솔에 적는다.
- **리셋 뒤**: 기존 `reinit_triggers`, 정지 그룹 재라우팅(`check_group`), `SWEVT` 재설정을 리셋 감지 경로에서도 부른다. `halt`면 부트 후 코어를 멈추고 DAP에 정지 이벤트를 보낸다. 설정은 코어 설정 `resetBehavior`다.
- **SW BP**: 앱의 시작 코드가 PSPR에 코드를 다시 복사하면 `debug` 명령이 지워진다. 다음 정지 때 메모리를 확인해 다시 심는다.
- **0x44 오류**: 플래시 쓰기 뒤 리셋에서 첫 `DBGSR` 읽기가 성공한 뒤에도 장치가 다시 재부팅하는지 확인한다. 맞으면 `reset_and_halt_core`가 일정 시간 안정된 상태를 확인한 뒤에 다음 단계로 가게 한다.

## 작업 패키지

기존 PRD의 WP-0–WP-9에 이어 번호를 붙였다. 진행 순서는 WP-10 → WP-11 → WP-12 → WP-13 → WP-14다. WP-12의 A-to-B는 WP-11의 유닛 배정을 쓴다. WP-13은 리셋 뒤 데이터 BP를 되살리므로 WP-11 뒤에 한다.

| WP | 내용 | 주요 변경 | 완료 조건 | 크기 |
| --- | --- | --- | --- | --- |
| WP-10 | step 중 인터럽트(FR-I). 먼저 인터럽트가 도는 앱에서 현상을 확인하고, 막기와 IE를 바꾸는 명령 판별, `stepInterrupts` 설정을 넣는다 | `architecture/tricore/mod.rs`, DAP `configuration.rs` | T-20–T-22 통과 | 소 |
| WP-11 | 데이터 BP(FR-D). 유닛 배정기를 넣고 데이터 트리거, 코어 API, RPC, DAP, GDB를 연결한다 | `core.rs`, `architecture/tricore/mod.rs`, `probe-rs-rpc`, `dap_server`, `gdb_server` | T-23–T-28 통과 | 중 |
| WP-12 | 성능 카운터(FR-P). 레지스터 표, A-to-B, 실행 중 읽기를 넣는다 | `architecture/tricore/registers.rs`, `mod.rs`, DAP 설정, `perf_counters` 예제 | T-29–T-31 통과 | 소–중 |
| WP-13 | 리셋 처리(FR-R). 리셋이 무엇을 지우는지 측정하고, 감지와 원인 표시, `resetBehavior`, 복원, 0x44 오류 조사를 한다 | `architecture/tricore/mod.rs`, DAP 설정, 리셋 시험 이미지 | T-32–T-36 통과 | 중 |
| WP-14 | 회귀 시험과 문서. 기존 실기 시험 전부와 단위 테스트, clippy·fmt를 돌리고 PRD·비교 문서·데모 설정을 갱신한다 | `docs/`, 데모 `.vscode` | T-37 통과, 문서 반영 | 소 |

WP마다 probe-rs `tricore` 브랜치에 커밋하고 fork에 push한다. 단위 테스트는 기존 `FakeBus` 모델을 늘려 쓴다.

## 시험 계획

실기 시험은 TC275 ShieldBuddy에서 한다. 기존 PRD가 T-15까지 썼으므로 T-20부터 번호를 붙였다. 플래시는 PF1 시험 섹터(`0x8021C000`)에만 쓴다.

**단위 테스트**(`FakeBus`)에서는 다음을 확인한다.

- 유닛 배정: 프로그램 BP, 데이터 BP 단일·범위, A-to-B가 섞여도 격하지 않고, 모자라면 거부한다.
- step 뒤 모든 종류의 유닛이 되살아난다.
- 데이터 트리거 값과 정지 이유 `Watchpoint`가 맞다.
- step 중 `ICR.IE`를 끄고 되돌리며, IE를 바꾸는 명령에서는 건드리지 않는다.
- 리셋 표지가 사라지면 복원 순서를 밟는다.

**실기 시험**

| ID | 시험 | 통과 기준 | 요구사항 |
| --- | --- | --- | --- |
| T-20 | 인터럽트가 도는 CPU1 앱에서 막기 없이 명령 step 50회 | 현상 기록: PC가 ISR로 간 횟수. 0이면 WP-10은 설정만 넣고 마친다 | FR-I1 |
| T-21 | 같은 step 50회를 `stepInterrupts: mask`로 | PC가 앱 루프를 벗어나지 않고, 끝난 뒤 `ICR.IE`가 원래 값 | FR-I1 |
| T-22 | ENABLE·DISABLE 명령 위에서 step | `ICR.IE`가 명령대로 바뀜 | FR-I2 |
| T-23 | VS Code에서 `g_result` 쓰기 BP | `compute`의 `st.w` 다음 명령에서 정지, 이유 `data breakpoint` | FR-D1–D3 |
| T-24 | `g_ticks` 읽기 BP | `ld.w` 다음 명령에서 정지 | FR-D1–D3 |
| T-25 | continue 5회, 그다음 BP 해제 | 매번 같은 곳에서 정지, 해제 뒤 3초 동안 정지 없음 | FR-D3 |
| T-26 | PFLASH 프로그램 BP 7개에 4바이트 데이터 BP(유닛 2개) | 데이터 BP 거부, 메시지에 유닛 부족 | FR-D4 |
| T-27 | `allStop` 세션 3개에서 CPU0 데이터 BP | CPU1·CPU2는 `external`, CPU1 continue 뒤에도 CPU0는 멈춘 채 남음 | FR-D5 |
| T-28 | GDB `watch`, `rwatch`, `awatch` | 각각 쓰기·읽기·둘 다에서 정지 | FR-D6 |
| T-29 | 레지스터 뷰의 `CCNT`·`ICNT` | run·pause 뒤 값이 늘고, `CE`=0일 때 쓴 값이 남음 | FR-P1 |
| T-30 | `perfWindow`를 `square` 시작·끝으로, `compute` 한 번 | `ICNT`가 디스어셈블리로 센 명령 수와 같음 | FR-P2 |
| T-31 | 실행 중 카운터 읽기 | 1초 간격 읽기에서 `CCNT`가 계속 늘고 코어는 멈추지 않음 | FR-P3 |
| T-32 | 앱 소프트 리셋 전후의 디버그 레지스터 | `TRnEVT`, `SWEVT`, `EXEVT`, `CBS_TRCn`, `SCU_RSTSTAT` 변화를 기록(설계 확정용) | FR-R1 |
| T-33 | 소프트 리셋을 요청하는 시험 이미지(PF1) | 디버그 콘솔에 리셋과 원인 `SW` 표시, 세션 유지 | FR-R1, R3 |
| T-34 | `resetBehavior: halt` | 부트 뒤 CPU0 정지, DAP 정지 이벤트 | FR-R2 |
| T-35 | `resetBehavior: restore` | BP, 데이터 BP, 정지 그룹이 되살아나고 리셋 뒤 BP에 다시 멈춤 | FR-R2, R3 |
| T-36 | PF1 섹터를 실제로 다시 쓴 뒤 리셋, 10회 | 0x44 오류 0회 | FR-R4 |
| T-37 | 회귀 시험 | 동시 정지 20, SW BP 18, DAP 10, GDB 11항목 통과. `core0_main` 9항목은 PF0 쓰기가 다시 승인될 때만 | 비기능 |

## 위험과 대응

가장 큰 위험은 트리거 유닛 8개를 여러 기능이 나눠 쓰는 데서 생긴다.

| 위험 | 영향 | 대응 |
| --- | --- | --- |
| step이 트리거 창 전체를 다시 쓴다(TR0 사용) | step 뒤 데이터 BP나 A-to-B가 사라짐 | 유닛 배정을 한 곳에서 관리하고 step 뒤 모두 되살린다. 단위 테스트로 막는다 |
| 데이터 트리거가 접근 주소만 비교한다 | 범위 앞에서 시작해 걸치는 접근(예: 8바이트 `st.d`)을 놓침 | 범위를 변수 정렬 단위로 넓힐지 실기로 본 뒤 정한다. 한계는 문서에 적는다 |
| 인터럽트 막기가 프로그램 동작을 바꾼다 | IE 판별을 빠뜨리면 인터럽트가 꺼진 채 남거나 켜짐 | IE를 바꾸는 명령 표를 두고, step 뒤 `ICR`을 다시 읽어 기대값과 다르면 경고한다. 설정으로 끌 수 있게 한다 |
| 앱이 `CCTRL`을 직접 쓴다 | 카운터 값이 디버거와 앱 사이에서 어긋남 | 디버거는 요청받았을 때만 `CCTRL`을 쓴다 |
| 앱 리셋이 표지 값을 지우지 않는다 | 리셋을 감지하지 못함 | T-32에서 먼저 측정해 감지 방법(표지, `SCU_RSTSTAT`, 부트 정지)을 정한다 |
| 0x44 오류의 원인을 모른다 | 플래시 뒤 리셋이 가끔 실패 | 재부팅 횟수와 시간을 기록해 원인을 좁힌다. 원인을 못 찾으면 재시도로 막는다 |
| 공통 코드(`CoreInterface`, DAP, GDB)를 바꾼다 | 다른 아키텍처의 동작 변화 | 기본 구현을 `NotImplemented`로 두고, probe-rs-tools 197개와 probe-rs 전체 단위 테스트를 돌린다 |

## 결정이 필요한 점

각 항목에 권장안을 적었다. 정해지면 체크하고 결정을 덧붙인다.

- [x] **step 중 인터럽트 기본값**: 권장은 `maskInstruction`이다. 명령 step만 막고, 소스 줄 step은 ISR이 돌도록 둔다. → 결정(2026-10-23): 권장안대로.
- [x] **리셋 뒤 기본 동작**: 권장은 `restore`다. 워치독 리셋이 나도 앱은 계속 돌고, 디버그 콘솔에 원인을 남긴다. → 결정(2026-10-23): 권장안대로.
- [x] **데이터 BP의 값 조건**: 하드웨어는 주소만 비교한다. DAP `condition`을 받으면 멈춘 뒤 값을 보고 조건이 아니면 다시 실행해야 한다. 권장은 1차에서 빼는 것이다. → 결정(2026-10-23): 권장안대로.
- [x] **성능 카운터 표시 위치**: 권장은 레지스터 뷰와 `perf_counters` 예제다. Live Watch 확장은 필요하면 더한다. → 결정(2026-10-23): 권장안대로.
- [x] **리셋 시험 이미지**: PF1 데모에 소프트 리셋 요청(Safety ENDINIT 해제 후 `SCU_SWRSTCON`)을 넣는다. PF0는 건드리지 않는다. 이 방식으로 괜찮은지 정한다. → 결정(2026-10-23): 이 방식으로 한다.
- [x] **2차 착수 조건**: 맵 1 어댑터, TC3xx 보드, ORTI 앱 중 무엇을 먼저 구할지 정한다. → 결정(2026-10-23): 1차를 마친 뒤 다시 정한다.

## 참고 자료

- [TRACE32 vs aurix\_debugger 비교](TRACE32_VS_AURIX_DEBUGGER.md): 이 지시서의 배경
- [probe-rs TriCore · MCD · MCDS 연동 개발지시서 (PRD)](PROBE_RS_TRICORE_PRD.md): WP-0–WP-9, T-01–T-15, 구현 현황
- TriCore TC1.6P & TC1.6E Core Architecture User Manual Volume 1, Infineon, V1.0 2012-02: 디버그 트리거(12.3, `TRxEVT` 12-25–12-27쪽), 성능 카운터(12.10–12.11, `CCTRL` 12-38쪽), `SWEVT`(12-23쪽)
- TC27x D-Step User's Manual V2.2, Infineon, 2014-12: 워치독 접근(7.4.4), `RSTSTAT`(7-102쪽)
- [TriCore Debugger and Trace](https://www2.lauterbach.com/pdf/debugger_tricore.pdf), Lauterbach, Release 02.2026: 브레이크포인트(27쪽), step과 인터럽트(29쪽), 벤치마크 카운터(33쪽), 리셋(51쪽)
- 코드: [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore) `ad00ecc`. 트리거 유닛과 SW BP는 `probe-rs/src/architecture/tricore/mod.rs`, `HaltReason::Watchpoint`는 `probe-rs/src/core/core_status.rs`, DAP 프로토콜 타입은 `dap_server/debug_adapter/dap/debugProtocol.rs`, GDB BP는 `gdb_server/target/breakpoints.rs`에 있다
