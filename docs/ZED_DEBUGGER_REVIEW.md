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
| RTT 출력 | 확장이 `probe-rs-rtt-data` 이벤트를 터미널로 표시 | 표시할 수 없음 (사용자 정의 이벤트를 받을 확장 API 없음) |
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

- `g_ticks`는 `0x7000a000`, 즉 각 코어가 자기 DSPR로 보는 로컬 주소에 있다. 그런데 코어 1·2에서 읽은 값도 CPU0의 카운터처럼 늘었다. 디버거 쪽 접근에서는 이 주소가 CPU0 DSPR로 가는 것으로 보인다. 코어별로 읽으려면 서버가 로컬 주소를 코어별 전역 주소(CPU1 DSPR `0x6000_0000` 등)로 바꿔야 한다(probe-rs 쪽 확인 필요).
- Zed의 메뉴에는 어댑터와 상관없이 Live Watch가 나온다(어댑터 기능 표시가 없어 가릴 수 없음). probe-rs가 아니면 오류만 보인다.
- RTT 터미널은 아직이다. 데모 프로그램에 RTT 제어 블록이 없어서, RTT를 쓰는 시험 프로그램(RAM이나 PF1 시험 섹터)이 먼저 필요하다.

## 그 밖의 관찰

- 이 PC에서는 F9가 Zed에 닿지 않았다(다른 프로그램이 가로채는 것으로 보임). 명령 팔레트의 `editor: toggle breakpoint`로 BP를 걸었다.
- `.zed/debug.json`이 있으면 `.vscode/launch.json`의 설정은 목록에 나오지 않는다.
- Zed는 처음 여는 폴더를 제한 모드로 열고 신뢰 여부를 묻는다.

## 다음 단계

1. ~~VS Code 회귀 확인~~ 완료(`7b97cbb`)
2. ~~A1: Zed 포크에 디스어셈블리 패널 시제품~~ 동작 확인(`39f53e4`), 명령 BP·명령 step·더 읽기(`db1bf8e`), 코어별 디스어셈블리(`82e6980`, probe-rs `18f99ec`), VS Code 디스어셈블리 뷰 재확인
3. ~~Live Watch 패널~~ 동작 확인(`7648baf`). 코어별 로컬 주소 읽기는 서버 확인 필요
4. RTT 터미널(RTT 시험 프로그램 필요), 이후 B로 옮길 공통 크레이트 분리

## 출처

- [Zed Debugger 문서](https://zed.dev/docs/debugger), [디버거 확장 문서](https://zed.dev/docs/extensions/debugger-extensions), [확장 개발 문서](https://zed.dev/docs/extensions/developing-extensions)
- Zed `v1.25.3` 소스: `crates/project/src/debugger/session.rs`(`push_to_history`, `handle_stopped_event`), `crates/debugger_ui/src/session/running.rs`, 크레이트별 `Cargo.toml`의 라이선스
- [zed-industries/dap-types](https://github.com/zed-industries/dap-types) `1b461b3`: `OutputEventGroup`, `StoppedEventReason`
- [probe-rs/zed-extension](https://github.com/probe-rs/zed-extension) `1570bed`
