# Zed 디버거 검토와 A0 실기 시험

2026-10-24 · 원본: [Claude 문서](https://claude.ai/artifact/Gh846ob7JKdHV8Ux77HpDH) · TC275 ShieldBuddy, Zed 1.25.3 (Windows), probe-rs fork `tricore` `ea6d0df`

## 결론

TRACE32 같은 디버거를 목표로 하면 화면은 VS Code를 주로 쓰고, 새 화면은 GPUI(Apache-2.0)로 독립 앱을 만드는 방향(B)이 맞다. Zed 확장은 패널을 추가할 수 없고 Zed에는 디스어셈블리 뷰가 없어, 트레이스·OS 태스크·Live Watch 같은 전용 창을 붙일 수 없기 때문이다. 순서는 Zed 포크로 기능을 빠르게 확인(A)한 뒤 B로 개발하는 것으로 정했다.

A0(포크 없이 Zed를 probe-rs DAP 클라이언트로 쓰기)는 서버 수정 3건과 확장 수정 1건 뒤에 TC275 3코어 한 세션에서 동작했다.

## VS Code와 Zed 비교

| 기능 | VS Code (현재) | Zed 1.25 |
| --- | --- | --- |
| 소스 BP, step, 콜스택, 변수 | 됨 | 됨 (실기 확인) |
| 조건·로그·히트 카운트 BP, 데이터 BP | 됨 | 지원 (데이터 BP는 0.196부터, 실기 미확인) |
| 메모리 보기 | Memory Inspector | 메모리 뷰 있음 (실기 미확인) |
| 디스어셈블리 뷰 | 기본 제공 | 없음. `debugger_ui`에 해당 소스가 없다 |
| 주변장치 (SVD) | 변수 뷰의 `Peripherals` | 됨. 서버가 변수 범위로 준다 (실기 확인) |
| 한 세션 멀티코어 | 됨 | 스레드 3개로 보임, 동시 정지 동작 (실기 확인) |
| RTT 출력 | 확장이 `probe-rs-rtt-data` 이벤트를 터미널로 표시 | 확장으로는 표시할 수 없음 (사용자 정의 이벤트를 받을 확장 API 없음). Zed 포크에 RTT 패널을 넣어 동작 확인(`7f255b4`) |
| Live Watch (`liveWatch` 요청) | 확장 `najari.probe-rs-live-watch` | 확장으로는 만들 수 없음 (패널 추가 불가). Zed 포크에 패널을 넣어 동작 확인(`7648baf`) |
| `tasks.json`의 `dependsOn` | 됨 | 읽지 못함 (시작 전 준비 작업은 직접 실행) |

Zed 확장은 Rust를 WASM으로 컴파일한 것이고 언어, 디버거 어댑터, 테마, 스니펫, MCP만 제공한다. 디버거 확장이 정하는 것은 어댑터 실행 방법과 설정 스키마뿐이다.

## 라이선스

- Zed의 `debugger_ui`, `dap`, `ui`, `editor` 크레이트는 GPL-3.0-or-later, `gpui`만 Apache-2.0이다.
- A(Zed 포크)는 사내 사용은 문제없지만 배포하면 포크와 추가 코드를 GPL로 공개해야 한다.
- B(GPUI 독립 앱)는 Zed의 GPL 크레이트(`ui`, `editor` 포함)를 쓸 수 없다. `gpui`와 `gpui-component`(Apache-2.0)로 화면을 새로 만든다.
- A에서 만든 코드 중 B로 옮길 것(DAP 클라이언트, 디스어셈블리 모델, RTT 수신 등)은 Zed에 의존하지 않는 별도 크레이트로 둔다.

## A0 실기 시험

구성: probe-rs `dap-server --port 50000`(fork `tricore`), Zed 확장 `probe-rs`(아래 수정본), 데모 `C:\work\aurix_tcf_demo`의 `.zed/debug.json`([사본](../zed/aurix_tcf_demo/debug.json)). 시작 전에 `start_in_flash.exe`로 CPU0를 PF1 데모의 `main`에 세웠다(플래시는 읽기만 함). Zed와 서버 사이에 DAP 메시지를 기록하는 TCP 중계기를 두고 확인했다.

| 항목 | 결과 |
| --- | --- |
| 연결 | `attach`, 스레드 `0-TC275`~`2-TC275` |
| BP | `demo.c:23` → `0x8021C028`, 코어 0·1·2에 검증된 BP |
| 정지 | CPU0가 BP에서 정지, CPU1·2는 `external`로 함께 정지 |
| 콜스택 | `scale → accumulate → compute → main` |
| 변수 | `v=0, k=3, s=0`, continue 뒤 `v=1` |
| 범위 | Static, Peripherals(XBAR, SBCU, CPU0 …), Registers, Variables |
| step over | 23:14 → 23:18 |
| continue | 다음 반복에서 같은 BP에 정지 |
| 일시정지 | 됨 |
| Console | 서버 메시지 표시 |

시험하지 않은 것: 스레드 전환(조작 도구가 포커스를 되돌릴 때 팝업이 닫힘), 메모리 뷰, 데이터 BP, 변수 쓰기.

## 찾아서 고친 문제

| 문제 | 증상 | 위치 | 조치 |
| --- | --- | --- | --- |
| 출력 이벤트의 `group: "probe-rs-debug"` (DAP는 `start`·`startCollapsed`·`end`만 허용) | Zed가 이벤트를 버려 Console이 빔 | 서버 | `group`을 뺌 (`ea6d0df`) |
| `pause` 응답의 문자열 본문 | Zed 로그 `invalid type: string ..., expected unit` | 서버 | 본문 없이 응답 (`ea6d0df`) |
| 동시 정지에서 모든 `stopped` 이벤트가 `allThreadsStopped:false` | Zed는 `stopped`마다 상태를 새로 시작해 마지막 코어만 정지로 남김. BP에 멈춘 CPU0가 실행 중으로 보이고 콜스택 요청이 없음 | 서버 (한 세션 라우팅) | 라우팅이 코어별 정지를 기억한다. 그룹이 멈춘 마지막 코어의 이벤트로 모든 코어가 멈추면, 스스로 멈춘 코어(CPU0)의 정지를 `allThreadsStopped:true`로 한 번 더 보낸다 (`ea6d0df`, `7b97cbb`) |
| 확장이 요청을 항상 `launch`로 보냄 | `attach` 설정인데 probe-rs가 칩을 리셋 | 확장 `src/lib.rs` | 설정의 `request`를 따르도록 수정 ([zed/probe-rs-extension](../zed/probe-rs-extension/README.md)) |

VS Code는 위 세 서버 문제를 그냥 넘어갔다. 처음 수정(`ea6d0df`)은 그룹이 멈춘 마지막 코어(CPU2, 초점 유지 표시)의 이벤트에 `true`를 넣었는데, VS Code는 이 값을 받으면 모든 콜스택을 비우고 그 이벤트의 스레드만 다시 읽는다. 그래서 VS Code에서 CPU0의 현재 줄이 사라졌다. `7b97cbb`부터는 그 이벤트는 `false`로 두고 CPU0의 정지를 `true`로 다시 보낸다.

2026-10-24 재확인(`7b97cbb`): VS Code 1.143은 CPU0 `paused on breakpoint`(콜스택 `square → scale → accumulate → compute`, 17번 줄 강조, `x = 2`), CPU1·2 `paused on external`, continue 뒤 `x = 3`에서도 초점 유지. Zed 1.25.3은 CPU0가 `demo.c:23`에 정지, 콜스택 표시.

## A1: 디스어셈블리 패널 시제품

Zed v1.25.3을 포크해([najari/zed](https://github.com/najari/zed/tree/tricore-disassembly), 브랜치 `tricore-disassembly`, `39f53e4`·`db1bf8e`, 로컬 `C:\work\zed`) 디버그 패널에 "Disassembly" 탭을 더했고, TC275에서 동작을 확인했다.

| 항목 | 내용 |
| --- | --- |
| 표시 조건 | 어댑터가 `supportsDisassembleRequest`를 알릴 때만 메뉴에 나옴 |
| 내용 | 선택한 프레임(없으면 첫 프레임)의 PC 앞 24개부터 명령 80개: 주소, 명령 바이트, 명령, 심볼, 소스 줄이 바뀌는 곳에 `demo.c:23` 같은 구분 |
| PC 줄 | ▶ 표시와 강조, 열 때 PC 앞뒤로 스크롤 |
| 갱신 | 프레임이 바뀔 때. 프레임 id는 정지마다 새로 정해져서 정지 직후 남아 있는 옛 프레임으로는 다시 요청하지 않음(정지 1번에 요청 1번) |
| 명령 BP (`db1bf8e`) | 줄 왼쪽 칸을 누르면 그 명령에 BP를 걸거나 지움(`setInstructionBreakpoints`, 어댑터가 `supportsInstructionBreakpoints`를 알릴 때). 바뀔 때마다 전체 목록을 보냄. 설정된 BP는 빨간 점, 거절됐거나 응답 전이면 흐린 점, 툴팁에 서버 메시지 |
| 툴바 (`db1bf8e`) | 명령 단위 Step Over·Step Into(`granularity: instruction`), 앞·뒤로 40개 더 읽기(보던 자리 유지, 이미 보이는 주소와 겹치는 명령은 버림), PC로 돌아가기 |
| 바꾼 파일 | `project`: `Session::disassemble`, 부호 있는 오프셋의 `disassemble` 요청, `Session::toggle_instruction_breakpoint`, `SetInstructionBreakpointsCommand`. `debugger_ui`: `disassembly_view.rs`, `DebuggerPaneItem::Disassembly`, `StackFrameList::current_stack_frame` |

실기(TC275, CPU0–CPU2 한 세션):

| 시험 | 결과 |
| --- | --- |
| PC 표시 | `demo.c:23` BP에서 `0x8021C028 ld.w d3, [a14]#-4`에 PC 표시. continue 뒤 같은 BP에서 다시 표시, step over 뒤 `0x8021C032 add d2, #1`로 이동 |
| 명령 Step Over | `next` `granularity: instruction`, PC `0x8021C028` → `0x8021C02C` |
| 명령 BP 설정 | `0x8021C030 mul d2, d3`: 코어 0·1·2에 설정(`verified: true`), continue 뒤 그 주소에서 정지 |
| 명령 BP 해제 | 빈 목록 요청, 다음 continue는 소스 BP `0x8021C028`에서 정지 |
| 앞·뒤 더 읽기 | `0x8021BFEE` 오프셋 -40, `0x8021C0CC` 오프셋 1로 요청, 화면 위치 유지. 데모 앞뒤는 지운 플래시라 `<not readable>` |
| 스레드 전환 | `1-TC275`를 고르면 그 PC `0x8003A552`(PF0 보드 펌웨어) 주변을 표시, `0-TC275`로 돌아오면 `0x8021C030` |

작업 중 찾은 것:

- Zed의 dap-types는 `instructionOffset`을 `u64`로 정의해 PC 앞쪽 명령(음수)을 요청할 수 없다. 포크에 부호 있는 요청 타입을 따로 두었다.
- probe-rs는 읽을 수 없는 주소마다 긴 오류 문구를 명령 칸에 넣고 표시 힌트가 없었다. `<not readable>`과 `presentationHint: invalid`로 바꿨다(probe-rs `d6c8b63`).
- Windows에서 Zed를 빌드하려면 Visual Studio의 Spectre 완화 라이브러리가 필요하다(`pet` 크레이트가 없으면 빌드를 멈춤). 이 PC(메모리 15.6 GB)에서는 `-j 4`로 빌드하다 메모리 부족으로 중단됐고, `-j 2`와 `CARGO_PROFILE_DEV_DEBUG=line-tables-only`로 첫 빌드 30분, 이후 다시 빌드는 약 2분 걸렸다.

남은 것:

- ~~`disassemble` 요청이 초점 코어로 감~~ 고침(probe-rs `18f99ec`, Zed `82e6980`, 아래).
- 스레드를 바꿨다 돌아오면 새로 읽어서 앞·뒤로 더 읽은 줄이 사라진다.
- CPU1을 골랐을 때 Variables 패널은 CPU0의 값(`v`, `k`, `s`)을 그대로 보였다. CPU1 프레임에 대한 `scopes` 요청이 없었다(Zed 쪽 동작, 원인 미확인).
- 명령 BP는 세션이 끝나면 사라진다(저장하지 않음).

### 코어별 디스어셈블리와 명령 BP (probe-rs `18f99ec`, Zed `82e6980`)

`disassemble` 요청에는 스레드가 없어서, 한 세션 멀티코어에서는 서버가 초점 코어(CPU0)로 보냈다. 플래시는 코어끼리 같아 결과가 맞았지만, 코어 전용 메모리(PSPR 등)였다면 다른 코어의 내용을 보여 줬을 것이다.

- 서버: `stackTrace` 응답의 `instructionPointerReference`에 `@코어`를 붙인다(`0x8003cc80@1`). 변수의 `memoryReference`와 같은 방식이고, 그 참조로 오는 `disassemble`은 해당 코어로 간다.
- 서버: 명령 BP는 참조에 코어가 붙어 있으면 그 코어에만, 없으면 모든 코어에 건다. 응답은 요청 순서대로 합친다.
- 서버: 명령 BP의 `offset`(바이트)을 무시하고 있었다. VS Code 디스어셈블리 뷰는 "읽은 기준 참조 + 오프셋"으로 BP를 보내므로, 지금까지는 기준 주소에 걸렸을 것이다. 이제 더한다.
- Zed: 앞·뒤 더 읽기를 화면 첫·끝 주소가 아니라 프레임의 참조와 명령 오프셋으로 요청한다. 참조 앞부분의 숫자를 PC로 읽는다.

| 시험 (TC275, CPU0–CPU2 한 세션) | 결과 |
| --- | --- |
| CPU1 선택 | `disassemble 0x8003cc80@1`, 더 읽기도 같은 참조에 오프셋 -64, 56. 받은 명령이 빈틈없이 이어짐, PC 표시 |
| CPU0 | `0x8021c028@0`, PC 표시 |
| Zed 명령 BP(코어 없음) | `0x8021C02C`가 코어 0·1·2에 설정, 해제 |
| DAP 스크립트: `0x8021c028@0` + `offset: 8` | `0x8021c030`이 코어 0에만 설정(`cores 0`), CPU0이 그 주소에서 정지 |

단위 테스트: probe-rs dap_server 113개(새 2개), Zed `parse_address` 1개. clippy `-D warnings`는 mcd 켬·끔 모두 통과.

VS Code 1.143 재확인(같은 한 세션 구성): 콜스택의 `square`에서 "Open Disassembly View"를 열면 `@0` 참조로도 PC 줄(`0x8021C008`)이 표시됐다. `0x8021C030 mul d2, d3` 줄에 BP를 걸자 BP 목록에 `0x8021c030`이 생겼고, continue 뒤 `0x8021c030`(23번 줄)에서 멈췄다. 수정 전이었다면 VS Code가 보낸 "기준 참조 + 오프셋" 중 기준 주소(`0x8021C008`)에 걸렸을 것이다.

## Live Watch 패널 (Zed `7648baf`)

디버그 패널에 "Live Watch" 탭을 더했다. VS Code 확장 `najari.probe-rs-live-watch`와 같은 방식으로, probe-rs의 `liveWatch` 요청(DAP 밖)으로 전역 변수를 타깃을 멈추지 않고 읽는다.

| 항목 | 내용 |
| --- | --- |
| 식 추가·삭제 | 위쪽 입력 칸에 쓰고 Enter, 줄마다 삭제 버튼. 식 목록은 Zed 키-값 저장소에 남음 |
| 읽기 | 500 ms마다. 스레드가 여럿이면 스레드마다(코어마다) `core`를 붙여 읽고 코어별로 묶어 표시. 앞 요청이 끝나지 않은 코어는 건너뜀 |
| 표시 | 값과 타입, 지난번과 달라진 값은 주황색, 읽지 못한 식은 서버 오류 |
| 일시정지 | 버튼으로 읽기를 멈춤. 세션이 끝나면 읽기도 끝남 |
| 바꾼 파일 | `project`: `Session::live_watch`, `liveWatch` 요청 타입. `debugger_ui`: `live_watch.rs`, `DebuggerPaneItem::LiveWatch`. 키맵: `LiveWatch > Editor`의 Enter |

실기(TC275, CPU0–CPU2 한 세션, probe-rs `18f99ec`):

| 시험 | 결과 |
| --- | --- |
| 정지 중 | `g_ticks = 36215`, `g_result = 46`이 코어 0·1·2에 표시 |
| 실행 중(`demo.c:23` BP를 잠시 끄고 continue) | 6초 동안 `liveWatch` 60번, `g_ticks`가 약 485만까지 늘며 주황색 표시, `g_result`는 46 유지 |
| 일시정지·재개 | 일시정지 3초 동안 요청 0번, 재개 뒤 3초에 18번(코어 3개 × 초당 2번) |

남은 것:

- ~~코어 1·2가 CPU0 메모리를 읽는 것 같음~~ 문제 아님(처음 판단이 틀렸다). TC27x에서 `0x7000_0000`은 CPU0 DSPR의 전역 주소이고, 코어가 자기 DSPR로 보는 로컬 주소는 `0xD000_0000`이다. `g_ticks`(`0x7000a000`)는 CPU0의 변수라 어느 코어에서 읽어도 같은 값이 맞다. probe-rs는 로컬 주소를 코어별 전역 주소로 바꿔 읽는다(`translate_local_address`). 보드에서 읽기만 하는 DAP 스크립트로 확인: `0xd000a000@0` = `0x7000a000`(`g_ticks`, `g_result` 값), `0xd000a000@1` = `0x6000a000`, `0xd000a000@2` = `0x5000a000`(둘 다 0, CPU0 값이 아님).
- Zed의 메뉴에는 어댑터와 상관없이 Live Watch가 나온다(어댑터 기능 표시가 없어 가릴 수 없음). probe-rs가 아니면 오류만 보인다.
## RTT 패널 (Zed `7f255b4`)

디버그 패널에 "RTT" 탭을 더했다. probe-rs는 RTT 채널마다 `probe-rs-rtt-channel-config` 이벤트를 보내고, 클라이언트가 `rttWindowOpened`로 답한 채널의 데이터만 `probe-rs-rtt-data` 이벤트로 보낸다(모두 DAP 밖).

| 항목 | 내용 |
| --- | --- |
| 세션(`project`) | 채널 설정 이벤트를 받으면 채널을 기록하고 `rttWindowOpened`(`windowIsOpen: true`)를 보낸다. 데이터는 줄로 나눠 채널마다 최대 10,000줄(넘으면 오래된 줄부터 버림) |
| 패널(`debugger_ui`) | 채널마다 버튼, 고른 채널의 줄을 목록으로 보여 주고 끝을 따라감, 지우기 버튼. `DebuggerPaneItem::Rtt` |

시험 프로그램: 데모에 RTT가 없어 [`rtt_demo`](../zed/aurix_tcf_demo/src/rtt_demo.c)를 새로 만들었다. RAM 데모처럼 CPU0 PSPR(코드)과 DSPR(`0x7000a000`, 제어 블록 `_SEGGER_RTT`와 512바이트 버퍼)에만 올라가고, `start_in_flash`가 RAM에 써 준다(플래시는 건드리지 않음). SEGGER 소스 없이 제어 블록 구조만 맞춘 최소 구현이고, 0.5초마다(STM0 100 MHz 기준) `rtt_demo line N, ticks M` 한 줄을 쓴다. 링커 스크립트는 PHDRS로 ELF 헤더가 세그먼트에 들어가지 않게 했다(넣으면 DSPR `0x7000A000` 아래를 덮어씀). 빌드는 `build_rtt.ps1`, Zed 시나리오는 "TC275: probe-rs RTT RAM CPU0"(`rttEnabled: true`).

실기(TC275, CPU0 한 세션, probe-rs `18f99ec`):

| 시험 | 결과 |
| --- | --- |
| 서버만(DAP 스크립트) | 채널 0 "Terminal"(String) 설정 이벤트, `rttWindowOpened` 뒤 0.5초마다 한 줄 |
| Zed | 채널 설정 이벤트 직후 Zed가 `rttWindowOpened`를 보냄, 패널에 `0: Terminal`과 줄 표시(142줄째까지 끝을 따라감) |
| 지우기 | 이전 줄이 사라지고 새 줄부터 다시 쌓임 |
| 일시정지 | 정지 뒤 RTT 데이터 이벤트 없음 |
| Live Watch 동시 | 앞 세션에서 넣은 식(`g_ticks`, `g_result`)이 Zed를 다시 열어도 남아 있었고, 이 프로그램의 `g_ticks`를 읽음(`g_result`는 없어서 오류 표시) |

### 여러 코어의 RTT (probe-rs `4b4b107`, Zed `76123f4`)

한 세션 멀티코어에서는 코어마다 RTT 제어 블록이 따로 있고 채널 번호도 0부터 시작한다. 그런데 RTT 이벤트에 코어가 없어 두 코어의 채널 0이 구분되지 않았고, `rttWindowOpened`도 초점 코어로만 갔다.

- 서버: 코어가 여럿인 세션에서는 `probe-rs-rtt-channel-config`, `probe-rs-rtt-data`, `probe-rs-create-prompt` 이벤트에 `core`를 넣는다(코어가 하나면 예전과 같음). 클라이언트는 `rttWindowOpened`에 그 `core`를 돌려주고, 라우팅이 그 코어로 보낸다.
- Zed: 채널을 (코어, 번호)로 구분하고 `core N · 0: Terminal`처럼 표시한다.
- 시험 프로그램: `build_rtt.ps1 -Core 1`이 CPU1 PSPR `0x60100000`/DSPR `0x6000a000`용 `rtt_demo_cpu1.elf`를 만든다(`start_in_flash --core 1`로 RAM에 올림, CPU1은 보드 펌웨어를 떠나므로 끝나면 리셋). Zed 시나리오 "TC275: probe-rs RTT RAM CPU0 + CPU1".

| 시험 (TC275, CPU0·CPU1 한 세션) | 결과 |
| --- | --- |
| DAP 스크립트 | 채널 0이 `core: 0`, `core: 1`로 각각 알려지고, 각자 `rttWindowOpened` 뒤 `rtt_demo CPU0 ...`, `rtt_demo CPU1 ...` 줄이 따로 옴 |
| Zed | `rttWindowOpened`를 `core: 0`, `core: 1`로 보냄, 두 코어 데이터 이벤트 43개씩. 패널에 버튼 두 개, 고른 코어의 줄만 표시 |
| 단일 코어 | 이벤트에 `core`가 없음(예전과 같음) |

단위 테스트: dap_server 114개(새 1개: `core` 인자로 `rttWindowOpened` 라우팅). clippy는 mcd 켬·끔 모두 통과.

남은 것: down 채널(입력)은 서버가 `probe-rs-create-prompt` 이벤트로 알리지만 Zed 패널은 아직 입력을 받지 않는다.

## B로 옮길 공통 크레이트

A에서 만든 패널들의 로직을 Zed 없이 쓸 수 있는 크레이트로 떼어 냈다([`crates/`](../crates/README.md)). Zed의 GPL 크레이트에 의존하지 않고 serde, serde_json, dap-types(MIT OR Apache-2.0, Zed와 같은 리비전이라 Zed 포크와 타입이 같다)만 쓴다. 라이선스는 아직 정하지 않았다(`publish = false`).

| 크레이트 | 내용 |
| --- | --- |
| `aurix-debug-model` | 입출력·UI 없는 상태. `reference`(주소, probe-rs `@core`), `protocol`(`liveWatch`, `rttWindowOpened`, RTT 이벤트, 부호 있는 오프셋의 `disassemble`), `disassembly`(PC 주변 창, 앞뒤 더 읽기와 겹침 제거), `breakpoints`(명령 BP), `live_watch`(코어별 읽기, 바뀐 값), `rtt`(코어·채널별 줄) |
| `aurix-dap-client` | TCP DAP 클라이언트. dap-types로 타입이 정해진 요청, 이벤트는 채널로. 스레드만 쓰고 async 런타임 없음(GPUI 백그라운드 작업이나 CLI에서 그대로 사용) |

Zed 포크의 로직(디스어셈블리 창, 명령 BP, Live Watch, RTT)을 그대로 옮겼고, 단위 테스트 11개(모델 10, 클라이언트 1)와 clippy `-D warnings`를 통과한다.

보드 확인(Zed 없이 예제만으로, probe-rs `4b4b107`):

| 예제 | 결과 |
| --- | --- |
| `rtt_tail` (CPU0·CPU1 RTT 시험 프로그램) | `core 0 · 0: Terminal`과 `core 1 · 0: Terminal` 채널을 열고 두 코어의 줄을 따로 출력(4초에 9줄, 14줄) |
| `live_watch` (플래시 데모, 3코어) | `g_ticks`가 읽을 때마다 늘며 `*` 표시, `g_result = 46`, 없는 변수는 `<not a global variable>` |

## B: GPUI 독립 앱 첫 화면 ([`app/`](../app/README.md))

공통 크레이트 위에 GPUI 창을 올린 첫 화면이다. GPUI는 옆의 Zed 체크아웃(`C:\work\zed`, v1.25.3)을 경로로 참조하고, Zed의 patch와 `Cargo.lock`을 가져와 같은 버전으로 빌드한다. 첫 빌드 2분 49초(`-j 2`).

라이선스 확인(`cargo metadata`): 빌드 그래프의 패키지 618개 중 GPL은 없다. 그중 Zed 크레이트 19개(gpui, gpui_platform, gpui_windows, collections, sum_tree 등)는 모두 Apache-2.0이다. `r-efi`, `self_cell`은 MIT·Apache를 고를 수 있는 다중 라이선스다.

| 화면 | 내용 |
| --- | --- |
| 툴바 | 선택한 스레드의 Continue, Pause, Step Over, Step Instruction, Step Into Instruction, 연결 상태 |
| 스레드 | 코어마다 하나, 상태(running, stopped와 이유) |
| 콜스택 | 선택한 스레드의 프레임, 누르면 그 프레임의 디스어셈블리 |
| 디스어셈블리 | PC(▶) 주변 명령, 위·아래 더 읽기, PC로 가기, 줄 왼쪽을 누르면 명령 BP(● 설정, ○ 거절) |
| 콘솔 | 서버 메시지, 정지 설명, 모든 코어의 RTT 출력 |

실기(TC275, 플래시 데모, CPU0–CPU2 한 세션):

| 시험 | 결과 |
| --- | --- |
| 연결 | attach 뒤 스레드 3개 running(`configurationDone` 뒤 서버가 코어를 실행) |
| Pause | CPU0 `stopped: pause`, 콜스택 `square → scale → accumulate → compute → main`, `0x8021c008` 주변 디스어셈블리와 `demo.c:16`·`:17` 줄 표시 |
| 명령 BP | `0x8021C00C mul`에 ●, 서버가 `verified`로 답함. Continue 뒤 그 주소에서 정지 |
| Step Instruction | `next` `granularity: instruction`, `0x8021c00e`로 이동 |
| 바깥 프레임 | `main`을 고르면 `0x8021c0ae`(`call` 안쪽)를 포함하는 `0x8021C0AC call` 줄에 ▶(모델이 주소를 포함하는 명령을 찾도록 고침) |
| 실행 중 스레드 | CPU1을 고르면 콜스택·디스어셈블리를 비우고 `stackTrace`를 보내지 않음 |
| 창 닫기 | `disconnect`를 보내고 종료, 다시 띄우면 새 세션으로 붙음 |

작업 중 고친 것: 바깥 프레임의 PC 표시(모델), 실행 중 스레드에 `stackTrace`를 보내던 것, 앞 스레드의 디스어셈블리가 남던 것, 콘솔에 같은 정지 메시지가 두 번 찍히던 것, 창을 닫을 때 `disconnect`가 없어 서버가 다음 세션을 받지 못하던 것(Windows에서는 앱 종료 훅이 돌지 않아 `on_window_should_close`에서 보냄).

남은 것: 메모리 쓰기, RTT 입력 채널.

### 소스 보기, 소스 BP, 변수·레지스터

가운데를 위 소스/아래 디스어셈블리로 나누고 오른쪽에 변수 패널을 붙였다. 상태 규칙은 공통 크레이트에 새로 둔 `source_breakpoints`(파일마다 `setBreakpoints`, 답을 줄에 연결, 서버가 옮긴 줄과 주소 기억), `variables`(scope·변수 트리)가 맡는다. 공통 크레이트 테스트 16개.

| 화면 | 내용 |
| --- | --- |
| 소스 | 프레임의 소스 파일(디버그 정보의 경로를 그대로 읽음), 현재 줄 ▶·강조, PC로 가기, 파일을 고친 뒤 Reload. 줄 왼쪽을 누르면 소스 BP(● 설정, ○ 거절) |
| 디스어셈블리 | 소스 BP가 실제로 걸린 명령에 ◆ |
| 변수 | Static, Peripherals, Registers, Variables. 누르면 펼치고 접음. 펼친 상태는 이름 경로로 기억해 다음 정지에도 유지(서버가 정지마다 참조 번호를 새로 매김). 이전 정지와 값이 다르면 빨간색. 지역 변수·레지스터는 같은 코어의 같은 함수끼리, 전역(Static, Peripherals)은 같은 코어끼리 비교 |

실기(TC275, 플래시 데모, CPU0–CPU2 한 세션):

| 시험 | 결과 |
| --- | --- |
| Pause | `scale` 23행 ▶, 디스어셈블리 `0x8021c028`, 지역 변수 `v=1, k=3, s=1` |
| 소스 BP | 22행에 ●(서버 `verified`), Continue 뒤 22행(`0x8021c01c`)에서 정지, `v=2`가 빨간색 |
| Step Over | 23행으로, 펼친 Registers(PC, D0–D15, A0–A15)가 보임 |
| Step Instruction | Registers 펼침 유지, 바뀐 `PC`와 `d3`(방금 `s=4`를 읽음)만 빨간색 |
| 바깥 프레임 | `accumulate` 32행과 그 변수 `n=4, i=2, sum=5`(`scale(0)+scale(1)`) |
| Static | `g_ticks`, `g_result`. `compute` 39행에서 멈췄다가 다음 호출의 `scale`에서 멈추면 늘어난 `g_ticks`가 빨간색 |
| BP 지우기 | 다시 누르면 지워지고, Continue 뒤 계속 실행 |
| 디스어셈블리 ◆ | 23행 BP는 `0x8021C028`, 32행은 `0x8021C04E`, 39행은 `0x8021C090`에 ◆ |

작업 중 고친 것: 콘솔이 같은 문장을 무조건 하나로 합쳐 같은 BP에 여러 번 멈춘 기록이 숨던 것(0.5초 안에 겹친 것만 합치도록), 붙는 순간 상태를 모르는 코어에 보낸 `stackTrace`의 실패(`cancelled`)가 콘솔에 찍히던 것, 값 비교를 함수 단위로만 해서 전역 변수 변화가 다른 함수에서는 표시되지 않던 것.

### Live Watch·RTT 패널

아래 콘솔 자리를 탭으로 바꿨다: Console, RTT 채널마다 하나(`RTT core 0 · 0: Terminal`), Live Watch. 상태는 공통 크레이트의 `live_watch`, `rtt` 모델 그대로다. 입력 칸은 GPUI의 텍스트 입력 핸들러를 구현한 작은 컴포넌트(`app/src/text_input.rs`)로, 키 입력뿐 아니라 IME·`WM_CHAR`로 오는 글자도 받는다(처음에 키 이벤트만 받게 만들었더니 `WM_CHAR`로 보낸 글자가 들어오지 않았다).

| 화면 | 내용 |
| --- | --- |
| Live Watch | 식을 입력하고 Enter, 또는 변수 패널 Static 항목의 "watch". 코어마다 한 열, 0.5초마다 읽고 바뀐 값은 빨간색. ×로 지우기, 읽기 일시정지. 식 목록은 attach 파일 옆 `<attach>.watch`에 저장돼 다음 실행에도 남음 |
| RTT | 코어·채널마다 탭, 새 줄을 따라 내려감, Clear. 콘솔에는 채널이 열렸다는 줄만 남김 |

실기(TC275):

| 시험 | 결과 |
| --- | --- |
| Live Watch, 실행 중 | 플래시 데모 CPU0–CPU2, `g_ticks`를 입력하자 세 코어 열이 0.5초마다 늘어남(빨간색) |
| Live Watch, 정지 | Pause하자 세 코어 값이 같은 162375828로 멈추고 변수 패널의 `g_ticks`와 같음 |
| watch 버튼 | Static의 `g_result`를 보내 46이 세 열에 나옴 |
| 저장 | 창을 닫고 다시 띄우자 두 식이 그대로, ×로 지우자 파일에서도 빠짐 |
| RTT 두 코어 | RAM 시험 프로그램 `rtt_demo`(CPU0), `rtt_demo_cpu1`(CPU1), 탭 두 개에 각자의 줄이 0.5초마다 쌓임, Clear 뒤 새 줄만 |

작업 중 고친 것: 읽기를 멈춰도 마지막 변화 표시가 남던 것(모델에서 지움), RTT 줄 수 옆의 안내가 지운 줄까지 "older not kept"로 세던 것.

### 메모리 보기

아래 패널에 Memory 탭을 더했다. 상태는 공통 크레이트의 새 `memory` 모델이 맡는다(256바이트 창, `@core`를 붙인 `readMemory`, base64 풀기, 이동 전 요청의 답 버리기, 바뀐 바이트 표시). 공통 크레이트 테스트 20개.

| 화면 | 내용 |
| --- | --- |
| 주소 | 16진(`0x`는 생략 가능)과 `@코어`(생략하면 선택한 스레드의 코어). 변수 패널 항목의 "mem"으로도 열림(변수의 `memoryReference`) |
| 표시 | 행마다 주소, 16바이트, ASCII. 요청한 주소의 바이트에 배경 |
| 읽기 | 탭이 보일 때 0.5초마다, 코어가 실행 중이어도 읽힘. 코어 버튼으로 읽을 코어를 바꿈. ◀/▶로 256바이트씩 이동, 읽기 일시정지 |
| 바뀐 바이트 | 빨간색. 다음 읽기가 직전과 같으면(멈춘 코어) 표시를 유지하고, 다시 바뀔 때 새로 표시 |

실기(TC275, 플래시 데모, CPU0–CPU2):

| 시험 | 결과 |
| --- | --- |
| 실행 중 `0x7000A000` | 첫 4바이트(`g_ticks`)가 계속 바뀌어 빨간색, 다음 4바이트 `2E 00 00 00` = `g_result` 46 |
| 코어 로컬 주소 | `d000a000@0`은 `7000A000`과 같은 내용, CPU1으로 바꾸면 CPU1 DSPR이라 0 |
| 변수의 mem | `accumulate`의 `n = 4` → `0x7000BFF4`에 `04 00 00 00`, `i`의 바이트에 배경 |
| ◀ | `7000BEF0`으로 256바이트 앞 |
| 멈춘 뒤 표시 | `compute`의 `g_ticks++`를 Step Over하자 `36` → `37`이 빨간색, 멈춘 뒤 6초 지나도 유지 |

작업 중 고친 것: 0.5초마다 다시 읽어 멈춘 코어에서 바뀐 표시가 바로 사라지던 것(같은 내용이면 표시 유지), 오류 뒤 첫 정상 읽기에서 모든 바이트가 바뀐 것으로 표시될 수 있던 것(양쪽 모두 읽힌 바이트만 비교).

### attach 설정 고르기와 다시 연결

세션이 없으면 가운데에 attach 화면이 나온다: 최근에 쓴 attach 파일(최신순, `%APPDATA%\aurix-debugger\recent.txt`)과 각 파일의 칩·코어별 프로그램 요약, "Open attach file…"(파일 대화상자), 서버 주소, 직전 실패 이유. 툴바에는 attach 파일 이름과 Disconnect. 실행 인자 없이도 뜬다.

세션마다 번호를 매겨, 끝난 세션의 이벤트 루프·Live Watch·메모리 읽기 루프는 멈추고 늦게 온 답은 버린다. 붙는 중에 포기하면 늦게 붙은 세션은 바로 끝낸다. 같은 attach 파일로 다시 붙으면 소스 BP와 명령 BP를 다시 보내고, 다른 파일이면 BP를 비우고 그 파일의 Live Watch 식을 읽는다. 연결 종료는 창 스레드를 막지 않게 뒤에서 보낸다.

실기(TC275):

| 시험 | 결과 |
| --- | --- |
| Disconnect | attach 화면으로, 최근 목록에 `tc275_demo_3cores.json`과 요약(TC275, 코어 0–2 `demo_flash.elf`) |
| 같은 파일로 다시 | 이전 세션에서 건 23행 BP를 다시 보내, 실행이 시작되자 23행(`0x8021c028`)에서 정지 |
| 파일 대화상자 | RTT 시험 프로그램을 RAM에 올린 뒤 `tc275_rtt_cpu0_cpu1.json`을 골라 코어 2개·RTT 탭 2개로 붙음, 이전 프로그램의 BP는 보내지 않음 |
| 틀린 서버 | `127.0.0.1:50001`: "연결을 거부했으므로 연결하지 못했습니다"를 화면과 콘솔에 표시 |

### 패널 크기 조절

경계선 네 곳(스레드|가운데, 가운데|변수, 소스/디스어셈블리, 아래 패널 위)을 끌어 크기를 바꾸고, 더블클릭하면 기본 크기로 돌아간다. 끌기는 누른 위치에서 움직인 거리만큼 바꾸므로 화면 좌표를 잴 필요가 없다. 크기는 `%APPDATA%\aurix-debugger\layout.json`에 저장되고, 창이 작아지면 가운데가 최소 폭·높이를 지키도록 패널을 줄인다(한쪽을 넓힐 때 반대쪽은 되도록 그대로). 규칙은 `app/src/layout.rs`에 두고 테스트 3개.

실기(TC275): 네 경계를 끌자 왼쪽 398, 오른쪽 477, 아래 348, 소스 비율 0.33으로 바뀌어 저장됨. 다시 띄우자 그대로. 더블클릭으로 260·340·210·0.5로 돌아옴. 작업 중 고친 것: 소스 칸을 줄이면 현재 줄이 화면 밖으로 밀리던 것(끌기가 끝나면 소스와 디스어셈블리를 현재 위치로 다시 맞춤).

## 그 밖의 관찰

- 이 PC에서는 F9가 Zed에 닿지 않았다(다른 프로그램이 가로채는 것으로 보임). 명령 팔레트의 `editor: toggle breakpoint`로 BP를 걸었다.
- `.zed/debug.json`이 있으면 `.vscode/launch.json`의 설정은 목록에 나오지 않는다.
- Zed는 처음 여는 폴더를 제한 모드로 열고 신뢰 여부를 묻는다.

## 다음 단계

1. ~~VS Code 회귀 확인~~ 완료(`7b97cbb`)
2. ~~A1: Zed 포크에 디스어셈블리 패널 시제품~~ 동작 확인(`39f53e4`), 명령 BP·명령 step·더 읽기(`db1bf8e`), 코어별 디스어셈블리(`82e6980`, probe-rs `18f99ec`), VS Code 디스어셈블리 뷰 재확인
3. ~~Live Watch 패널~~ 동작 확인(`7648baf`). 코어별 로컬 주소 읽기도 확인(문제 아님)
4. ~~RTT 패널~~ 동작 확인(`7f255b4`, 시험 프로그램 `rtt_demo`), 여러 코어 RTT(`76123f4`, probe-rs `4b4b107`). 입력 채널은 남음
5. ~~B로 옮길 공통 크레이트 분리~~ `crates/`의 `aurix-debug-model`, `aurix-dap-client`(보드에서 예제로 확인)
6. ~~B의 첫 화면~~ `app/`(스레드, 콜스택, 디스어셈블리와 명령 BP, 실행 제어, 콘솔) 보드에서 확인
7. ~~B에 소스 보기와 소스 BP, 변수·레지스터~~ 보드에서 확인
8. ~~B에 Live Watch·RTT 패널~~ 보드에서 확인
9. ~~B에 메모리 보기~~ 보드에서 확인
10. ~~B에 attach 설정 고르기~~ 보드에서 확인(다시 연결, BP 다시 보내기)
11. ~~B에 패널 크기 조절~~ 보드에서 확인
12. B에 메모리 쓰기(RAM만), RTT 입력 채널. 크레이트·앱 라이선스 결정

## 출처

- [Zed Debugger 문서](https://zed.dev/docs/debugger), [디버거 확장 문서](https://zed.dev/docs/extensions/debugger-extensions), [확장 개발 문서](https://zed.dev/docs/extensions/developing-extensions)
- Zed `v1.25.3` 소스: `crates/project/src/debugger/session.rs`(`push_to_history`, `handle_stopped_event`), `crates/debugger_ui/src/session/running.rs`, 크레이트별 `Cargo.toml`의 라이선스
- [zed-industries/dap-types](https://github.com/zed-industries/dap-types) `1b461b3`: `OutputEventGroup`, `StoppedEventReason`
- [probe-rs/zed-extension](https://github.com/probe-rs/zed-extension) `1570bed`
