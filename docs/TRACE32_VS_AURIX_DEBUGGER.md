# TRACE32 vs aurix_debugger 비교

2026-10-23 · 원본: [Claude 문서](https://claude.ai/artifact/5jqyM5sGLDs8UX6RKSBJ5L)

## 결론

TRACE32는 AURIX 전 제품군을 다루는 상용 디버거·트레이스 시스템이고, aurix_debugger는 TC275에서 소스 디버깅의 기본 흐름을 VS Code로 제공하는 오픈소스 도구다. 브레이크포인트, step, 동시 정지, 플래시처럼 일상적인 디버깅은 비슷하게 된다.

가장 큰 차이는 네 가지다.

1. **트레이스와 성능 분석**: TRACE32는 MCDS 온칩 트레이스, AGBT 고속 트레이스, DAP 스트리밍, 벤치마크 카운터, AUTOSAR(ORTI·ARTI) 프로파일링을 갖추었다. aurix_debugger는 MCDS 기록 CLI만 있고 아직 검증하지 못했다.
2. **칩과 코어 범위**: TRACE32는 TC1xxx–TC4x와 PPU·GTM·SCR·HSM·PCP까지, aurix_debugger는 TC27x의 TriCore CPU0–2만 다룬다.
3. **브레이크포인트 종류와 리셋 처리**: TRACE32는 데이터(읽기·쓰기) 브레이크포인트와 소프트 리셋 뒤 동작 선택을 제공한다. aurix_debugger에는 둘 다 없다.
4. **안전 인증과 자동화**: TRACE32는 ISO 26262 도구 인정 키트, 커버리지, PRACTICE·Python 스크립트를 제공한다.

이 보드에서 바로 메울 수 있는 것은 데이터 브레이크포인트, step 중 인터럽트 막기, 성능 카운터 표시, 소프트 리셋 처리다. 트레이스와 칩 범위는 다른 어댑터나 보드가 있어야 한다.

> **2026-10-23 갱신**: 이 네 가지는 [보완 개발지시서](AURIX_DEBUGGER_IMPROVEMENT_PRD.md)대로 구현해 실기로 확인했다(probe-rs `79dd08d`, `6826341`, `f0d9b30`, `fbf362f`). 아래 표의 aurix_debugger 열은 갱신한 내용이고, 3번 차이 가운데 남은 것은 없다.

## 비교 대상과 기준

- **TRACE32**: Lauterbach의 TriCore 디버거와 트레이스다. 공개 문서를 기준으로 했다. [TriCore Debugger and Trace](https://www2.lauterbach.com/pdf/debugger_tricore.pdf) Release 02.2026, [TriCore 제품 페이지](https://www.lauterbach.com/tricore), 보도자료, 지원 문서를 봤다. TRACE32 하드웨어가 없어 실기로 확인하지는 않았다.
- **aurix_debugger**: [najari/aurix_debugger](https://github.com/najari/aurix_debugger)와 [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore)(커밋 `ad00ecc`, 보완 뒤 `fbf362f`)다. probe-rs TriCore 포트, VS Code 설정, Live Watch 확장으로 이루어져 있다. TC275 ShieldBuddy(C-step, DAS JTAG AURDUINO. 예전에 ED로 적었으나 CHIPID.EEA = 0이라 ED가 아닌 것으로 보임, 2026-10-25)에서 확인한 결과를 기준으로 했다.
- 가격은 TRACE32가 공개하지 않아 비교하지 않았다. 하드웨어와 아키텍처별 라이선스를 따로 산다.

## 기능 비교

표의 TRACE32 내용은 위 문서에서 가져왔고, aurix_debugger 내용은 실기로 확인한 것이다.

| 항목 | TRACE32 | aurix_debugger |
| --- | --- | --- |
| 지원 칩 | TC1xxx(AUDO), TC2xx, TC3xx, TC4x | TC27x(TC275에서 확인), TC36x(TriBoard TC3X5에서 확인, 2코어) |
| 프로브 | Lauterbach 전용: AUTO26 Debug Probe(LA-3203), CombiProbe 2(LA-3081), PowerTrace Serial(LA-3325·3326) | Infineon DAS(TasServer, TCP 24817). 보드의 온보드 DAS로 충분, MCD 라이브러리도 가능 |
| 접속 | JTAG, DAP, DXCPL, DXCM. ETK·GSI·VX1000과 포트 공유, XCP | DAS가 처리. 세션 여러 개가 TAS 서버를 나눠 씀 |
| 디버깅 코어 | 모든 TriCore CPU와 PPU, GTM, SCR, HSM, PCP | TriCore CPU0–2 |
| 멀티코어 | SMP(한 GUI, 자동 동기), AMP(GUI별, `SYnch`로 동기), iAMP(한 GUI, 코어별 심볼) | 코어별 DAP 세션(AMP와 비슷). `allStop`을 켜면 OCDS 트리거 스위치로 함께 정지·실행 |
| 프로그램 BP | 온칩 8개. RAM에는 SW BP, 플래시 SW BP는 `FLASH.AUTO`로 가능하나 수명 때문에 비권장 | 트리거 유닛 8개. 코어 자신의 PSPR에서는 유닛이 모자라면 SW(`debug` 명령) 32개까지. 유닛 BP는 리셋과 코드 복사에도 남는다 |
| 데이터 BP | 읽기·쓰기 8개(단일 주소) 또는 4개(범위) | 읽기·쓰기·둘 다, 트리거 유닛을 프로그램 BP와 나눠 씀(1바이트는 1개, 범위는 2개). VS Code "Break on Value Change"(변수 뷰, Watch 표현식, 주소)와 GDB `watch`·`rwatch`·`awatch`. 값 조건·횟수는 DAP에서 멈춘 뒤 서버가 따진다(2026-10-25, probe-rs `c18738d`; 실시간은 아님) |
| step | 온칩 BP 범위로 예외를 감지. step 중 인터럽트 막기 옵션(`IMASKASM`, `IMASKHLL`) | Infineon 참조 디버거의 step trap 순서. 명령 step 동안 `ICR.IE`를 막음(`stepInterrupts`, 기본 `maskInstruction`, `allow`) |
| 리셋 | 소프트·하드 리셋과 전원 재인가를 넘어 디버깅. 리셋 뒤 동작 선택(`Halt`, `RestoreGo`, `RunRestore`) | DAP restart, `entryFunction`. 다른 세션의 리셋을 2초까지 버티고 정지 그룹을 다시 연결. 앱이 일으킨 리셋을 감지해 원인을 알리고, BP를 되살려 계속 실행(`restore`) 또는 정지(`halt`) |
| 트레이스 | MCDS 온칩 트레이스(최대 6코어, TC275TF는 동시 2코어), AGBT·SGBT, DAP 스트리밍(CombiProbe 2), 복합 트리거 언어(CTL) | `probe-rs mcds record`·`timestamp` CLI. 기록은 이 어댑터가 맵 1을 막아 미검증. VS Code 연동 없음 |
| 성능 측정 | 벤치마크 카운터: CCNT, ICNT, M1–M3CNT, OTGS·MCDS 카운터, BP 사이 구간 측정(A-to-B) | CCNT, ICNT, M1–M3CNT를 레지스터 뷰에 표시, 두 주소 사이 구간 측정(`perfWindow`), 실행 중 읽기. MCDS 타임스탬프 CLI |
| 플래시 | `FLASH.ReProgram`, 장치별 스크립트, HSM 플래시 | TC27x와 TC36x의 PFLASH. 같은 섹터는 건너뛰고, 지워진 페이지는 ECC로 판별. 지우기 전 섹터 백업 옵션 |
| OS 인식 | AUTOSAR Classic(ORTI)의 태스크·ISR·runnable, ARTI 프로파일링 | ORTI 2.1·2.2의 태스크·ISR 표(앱 B OS 탭, 멈춤·실행 중 0.5초, 2026-10-25, probe-rs `d8954dd`). 캐스트·산술 식, runnable, ARTI는 없음 |
| 캐시·오버레이·MPU | 캐시 라인 보기, 데이터 오버레이(OVC), 코드 오버레이, MPU 디버깅 | 없음 |
| 커버리지·인증 | 객체 커버리지부터 MC/DC까지, 도구 인정 키트(TQSK, ISO 26262) | 없음 |
| IDE | PowerView. VS Code는 `t32debugadapter`(Remote API 프록시, R.2024.09 이후)로 연결하고, 변수는 정지했을 때만 갱신 | VS Code 네이티브 DAP. 실행 중 Live Watch, SVD 주변장치 읽기·쓰기, GDB 서버 |
| 자동화 | PRACTICE 스크립트(`.cmm`), Remote API(RCL), Python `lauterbach.trace32.rcl`(3.6+) | probe-rs CLI, Rust 라이브러리, probe-rs RPC |
| 시뮬레이터 | 명령어 시뮬레이터, 가상 타깃(MCD API, GDB) | 없음 |
| 비용·소스 | 상용, 비공개. 하드웨어와 라이선스(예: TriCore Debug License LA-7756A) | 무료, 오픈소스 |

출처: [TriCore 매뉴얼](https://www2.lauterbach.com/pdf/debugger_tricore.pdf) 27–63쪽, [제품 페이지](https://www.lauterbach.com/tricore), [ARTI 보도자료](https://www.lauterbach.com/press-releases/arti-trace-support-for-infineon-aurix-production-devices), [VS Code 연동 문서](https://support.lauterbach.com/kb/articles/vscode-as-frontend-for-trace32-using-dap), [Python API 매뉴얼](https://www2.lauterbach.com/pdf/app_python.pdf).

## aurix_debugger가 나은 점

aurix_debugger가 앞서는 점은 비용, VS Code에서의 사용성, 직접 고칠 수 있다는 점이다.

- **비용과 하드웨어**: 전용 프로브와 라이선스가 필요 없다. ShieldBuddy처럼 보드에 있는 DAS로 충분하다.
- **VS Code 사용성**: DAP를 직접 구현해 실행 중에 변수를 갱신한다(Live Watch, 요청당 약 50 ms). TRACE32의 VS Code 연동은 정지했을 때만 변수를 갱신하고, 실시간 보기는 PowerView가 필요하다. SVD 주변장치 보기와 쓰기도 VS Code 안에서 된다.
- **직접 고칠 수 있음**: 백엔드가 모두 공개돼 있다. 동시 정지, SW 브레이크포인트, 진입 함수 정지를 필요할 때 직접 더했다.
- **원격과 CI**: TAS가 TCP라 다른 PC의 TasServer에 붙는다. `download`, `read`, `mcds` 같은 CLI를 스크립트와 CI에서 쓸 수 있다.
- **GDB 서버**: `probe-rs gdb`로 GDB에서 바로 디버깅한다. TRACE32 제품 페이지에 나온 GDB 연동은 가상 타깃용이다.

## 보완 가능한 점

1–4번은 2026-10-23에 구현했다(맨 오른쪽 열은 처음 검토할 때의 조건이다). 5–8번은 하드웨어나 시험 대상 앱이 더 필요하다. 9번은 2026-10-24에 구현했다. 10–11번은 범위가 커서 필요할 때 다룬다. 크기는 소(며칠), 중(1–2주), 대(그 이상)로 잡았다.

| # | 항목 | 지금 | 메우는 방법 | 크기 | 조건 |
| --- | --- | --- | --- | --- | --- |
| 1 | 데이터 BP(읽기·쓰기 감시) — **완료** `6826341` | 검토 때: 없음. probe-rs DAP와 GDB 서버 모두 데이터 BP를 받지 않았다 | 트리거 유닛을 데이터 주소용으로 쓴다(`TRnEVT`의 `ALD`·`AST`, 범위는 `RNG`). DAP `dataBreakpointInfo`·`setDataBreakpoints`와 GDB `Z2`–`Z4`를 더한다. 프로그램 BP와 유닛 8개를 나눠 쓴다 | 중 | 없음 |
| 2 | step 중 인터럽트 막기 — **완료** `79dd08d` | 검토 때: 옵션 없음. 인터럽트가 켜진 코드에서 step이 ISR로 들어가는지 확인하지 않았다(확인 결과 들어갈 수 있었다) | 먼저 인터럽트가 도는 앱에서 동작을 확인한다. 필요하면 step하는 동안 `ICR.IE`를 끄고 되돌린다(TRACE32 `IMASKASM`·`IMASKHLL`과 같은 옵션) | 소 | 없음 |
| 3 | 성능 카운터 — **완료** `f0d9b30` | 검토 때: MCDS 타임스탬프 CLI만 | `CCTRL`·`CCNT`·`ICNT`·`M1`–`M3CNT`를 레지스터 뷰나 Live Watch에 보이고, 두 BP 사이 구간을 재는 명령을 더한다 | 소–중 | 없음 |
| 4 | 앱이 일으킨 리셋 처리 — **완료** `fbf362f` | 검토 때: 다른 세션의 리셋만 버틴다. 워치독·소프트 리셋 뒤에는 동작을 고를 수 없다 | 리셋을 감지(`SCU_RSTSTAT`, 부트 정지 상태)하고, 멈춤 또는 BP 복원 뒤 실행을 고르는 설정을 더한다. 플래시 쓰기 직후 리셋의 0x44 오류도 같이 다룬다 | 중 | 없음 |
| 5 | MCDS 트레이스 검증과 VS Code 표시 | CLI만 있고 기록(T-15)은 미검증. 2026-10-24에 Infineon의 MCDS 기록 라이브러리로도 이 어댑터에서는 맵 1을 읽지 못함을 확인했다([DAS 분석](DAS_DLL_ANALYSIS.md) 부록 D) | 맵 1을 여는 어댑터로 기록을 확인한다. 그다음 프로그램 흐름을 소스 줄로 풀어 VS Code에 보인다 | 대 | DAP miniWiggler 등 맵 1 어댑터 |
| 6 | 칩·플래시 범위(TC3xx, DFLASH, UCB) — **TC36x 완료** `e1f1952` | TC27x와 TC36x의 PFLASH, TC27x의 DFLASH(DF0). TC36x DFLASH, UCB, 다른 TC3xx(TC37x 등)는 없음 | TC36x는 타깃 정의와 TC3xx 플래시 로더로 구현했다. TC27x DFLASH는 로더의 8 B 페이지 쓰기와 8 KiB 섹터 맵으로 더했다. 나머지 TC3xx는 칩별 타깃 정의를 더하고(WP-9), TC3xx DFLASH·UCB는 따로 만든다 | 대 | TC37x 등 해당 보드(TC36x는 TriBoard에서 확인) |
| 7 | AUTOSAR OS 인식(ORTI) | 태스크·ISR 표 됨(2026-10-25, 직접 만든 데모 OS로 시험) | 실제 AUTOSAR OS의 ORTI 파일로 확인, 캐스트·산술 식 | 작음 | ORTI를 내는 OS의 앱 |
| 8 | DAP 스트리밍·ARTI — **조사 완료**(2026-10-24) | 없음 | 경로는 있다. TAS의 트레이스 채널(`CTasClientTrc`의 `subscribe`·`get_trace`, TasServer V2.0 이상)로 서버가 트레이스를 보내고, miniMCDS는 연속 트레이스를 지원하지 않는다. 이 어댑터는 연속 트레이스 등급 8(불안정, 대역폭 0 kB/s)이고 맵 1을 막아 쓸 수 없다. 구현하려면 DAP 기반 툴 하드웨어(등급 12)에서 트레이스 클라이언트를 더한다([DAS 분석](DAS_DLL_ANALYSIS.md) 부록 D) | 대 | DAP 기반 어댑터 |
| 9 | 한 세션 멀티코어(SMP 보기) — **완료** `763c12a`–`84a45aa` | 검토 때: 코어별 세션 3개 | DAP 서버 하나가 `coreConfigs`의 코어를 모두 맡고 코어마다 스레드 하나로 보인다. 요청은 스레드·프레임·변수로 그 코어에 가고, 소스 BP 하나가 코어마다 걸리며, 상태 확인은 TAS 요청 하나다. VS Code에서 확인했다([한 세션 멀티코어 PRD](ONE_SESSION_MULTICORE_PRD.md)) | 대 | 없음 |
| 10 | 캐시·데이터 오버레이(OVC)·MPU 인식 | 없음 | OVC 설정을 읽어 메모리 읽기를 CPU 주소 기준으로 바로잡는다 | 중 | 오버레이를 쓰는 앱 |
| 11 | 보조 코어, 커버리지, 도구 인증 | 없음 | GTM·HSM·SCR은 다른 아키텍처다. 커버리지는 트레이스가 있어야 하고, ISO 26262 인증은 범위 밖이다 | 대 | — |

## 권장 순서

지금 장비로 할 수 있고 일상 디버깅에 바로 쓰이는 것부터 한다. 1–4는 이 순서대로 끝냈다(2026-10-23).

1. ~~**step 중 인터럽트 확인 (2번)**~~ 완료: 인터럽트를 대기시키면 step이 ISR로 빠졌고, 기본값으로 막는다.
2. ~~**데이터 BP (1번)**~~ 완료: VS Code의 변수 우클릭 "Break on Value Change"가 동작한다.
3. ~~**성능 카운터 (3번)**~~ 완료: 레지스터 뷰와 `perfWindow` 구간 측정.
4. ~~**리셋 처리 (4번)**~~ 완료: 감지·원인 알림·`resetBehavior`.
5. **하드웨어가 생기면**: MCDS 트레이스(5번)와 DAP 스트리밍(8번)은 맵 1을 열고 연속 트레이스를 지원하는 DAP 기반 어댑터, 나머지 칩 범위(6번, TC37x 등)는 해당 보드가 있을 때 한다. ORTI(7번)는 AUTOSAR 앱이 생기면 한다.

트레이스 기반 분석(커버리지, ARTI)과 ISO 26262 인증이 필요한 일에는 TRACE32를 쓰는 편이 현실적이다.

## 출처

- [TriCore Debugger and Trace](https://www2.lauterbach.com/pdf/debugger_tricore.pdf), Lauterbach, Release 02.2026: 멀티코어(19–24쪽), 브레이크포인트(27쪽), step(29쪽), 플래시(30쪽), 벤치마크 카운터(33쪽), 리셋(51쪽), 워치독·오버레이(56쪽), MCDS 트레이스(63쪽)
- [TriCore Debugger & Trace 제품 페이지](https://www.lauterbach.com/tricore), Lauterbach: 지원 칩, 접속, 보조 코어, 트레이스, 프로브·라이선스 번호
- [ARTI trace support for Infineon AURIX production devices](https://www.lauterbach.com/press-releases/arti-trace-support-for-infineon-aurix-production-devices), Lauterbach, 2023-12-05
- [VS Code as frontend for TRACE32 using DAP](https://support.lauterbach.com/kb/articles/vscode-as-frontend-for-trace32-using-dap), Lauterbach 지원 문서
- [Controlling TRACE32 via Python 3](https://www2.lauterbach.com/pdf/app_python.pdf), Lauterbach, Release 02.2026
- TriCore TC1.6P & TC1.6E Core Architecture User Manual Volume 1, Infineon, V1.0 2012-02: 트리거 이벤트(12.3, `TRxEVT`), 성능 카운터(12.10)
- [najari/aurix_debugger](https://github.com/najari/aurix_debugger)와 [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore): 기능과 실기 시험 기록. 자세한 내용은 [PRD](PROBE_RS_TRICORE_PRD.md)와 [aurix-tcf 비교 문서](PROBE_RS_VS_AURIX_TCF.md)에 있다
