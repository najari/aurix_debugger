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
| Live Watch (`liveWatch` 요청) | 확장 `najari.probe-rs-live-watch` | 만들 수 없음 (확장이 패널을 추가할 수 없음) |
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

## 그 밖의 관찰

- 이 PC에서는 F9가 Zed에 닿지 않았다(다른 프로그램이 가로채는 것으로 보임). 명령 팔레트의 `editor: toggle breakpoint`로 BP를 걸었다.
- `.zed/debug.json`이 있으면 `.vscode/launch.json`의 설정은 목록에 나오지 않는다.
- Zed는 처음 여는 폴더를 제한 모드로 열고 신뢰 여부를 묻는다.

## 다음 단계

1. ~~VS Code 회귀 확인~~ 완료(`7b97cbb`)
2. A1: Zed 포크에 디스어셈블리 패널 시제품 (`C:\work\zed`, v1.25.3)
3. RTT 터미널과 Live Watch 패널, 이후 B로 옮길 공통 크레이트 분리

## 출처

- [Zed Debugger 문서](https://zed.dev/docs/debugger), [디버거 확장 문서](https://zed.dev/docs/extensions/debugger-extensions), [확장 개발 문서](https://zed.dev/docs/extensions/developing-extensions)
- Zed `v1.25.3` 소스: `crates/project/src/debugger/session.rs`(`push_to_history`, `handle_stopped_event`), `crates/debugger_ui/src/session/running.rs`, 크레이트별 `Cargo.toml`의 라이선스
- [zed-industries/dap-types](https://github.com/zed-industries/dap-types) `1b461b3`: `OutputEventGroup`, `StoppedEventReason`
- [probe-rs/zed-extension](https://github.com/probe-rs/zed-extension) `1570bed`
