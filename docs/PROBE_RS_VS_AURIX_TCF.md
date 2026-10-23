# probe-rs TriCore vs aurix-tcf 확장 기능 비교

2026-10-23 · 원본: [Claude 문서](https://claude.ai/artifact/H9kLK73bCb8QuqCmJYgzdQ)

## 결론

소스 디버깅의 기본 흐름(브레이크포인트, step, 콜스택, 변수, 디스어셈블리, 주변장치 읽기)은 두 도구가 비슷하다. probe-rs TriCore에 남은 큰 차이는 지원 칩 범위(TC2x·TC3x·TC4x 전체 vs TC27x와 TC36x)다. 모든 코어를 함께 멈추는 동시 정지(`allStop`), Live Watch, 주변장치 레지스터 쓰기, 진입 함수 정지, RAM 코드의 SW 브레이크포인트는 구현해 실기로 확인했고, 시험하지 않았던 DAP·GDB 기능 7가지도 모두 실기로 확인했다. 멀티코어는 여전히 코어별 세션 3개를 묶는 방식이다(aurix-tcf는 세션 하나).

- 비교 대상: [ZeZhan/vscode-aurix-tcf-debugger](https://github.com/ZeZhan/vscode-aurix-tcf-debugger) `main`(README, `package.json`, `src/`)과 [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore)(커밋 `ad00ecc`)
- probe-rs 쪽 내용은 TC275 ShieldBuddy 실기 시험 결과 기준이다. aurix-tcf 쪽 내용은 저장소 문서와 코드를 기준으로 했고, 실기로 다시 확인하지는 않았다.

## 부족한 점

중요도 순서다. 1–3은 디버깅 방식과 지원 범위의 차이고, 4–10은 편의 기능의 차이다. 1번(동시 정지, 세션 하나)과 4·5·6·7번은 구현을 마쳤다.

| # | 항목 | aurix-tcf 확장 | probe-rs TriCore (현재) | 메우는 방법 |
| --- | --- | --- | --- | --- |
| 1 | 멀티코어 동작 | 세션 하나에 모든 코어가 스레드로 나옴. 한 코어가 멈추면 모든 코어가 멈춤(all-stop) | **세션 하나 구현함:** `coreConfigs`에 코어 3개를 주면 DAP 서버 하나가 코어마다 스레드 하나로 보이고, 요청을 스레드·프레임·변수로 그 코어에 보낸다. 소스 BP 하나가 세 코어에 걸리고, 상태 확인은 TAS 요청 하나. 코어별 세션 3개(compound)도 그대로 된다. **동시 정지:** `allStop`을 켜면 OCDS 트리거 스위치로 한 코어가 멈출 때 나머지도 멈추고(이유 `external`), continue하면 함께 돈다. 제한: 자기 BP에 멈춘 코어는 그 코어에서 continue해야 함. 그룹에 코어가 둘 이상이면 앱 리셋 뒤 장치가 잠긴다(`--connect-under-reset`로 복구) | 완료: 동시 정지 `84c21eb`, 세션 하나 `763c12a`–`0e7761c`([한 세션 멀티코어 PRD](ONE_SESSION_MULTICORE_PRD.md)), VS Code에서 확인 |
| 2 | 지원 칩 | TC2x, TC3x, TC4x 전체 | TC27x(TC275)와 TC36x(2코어, TriBoard TC3X5에서 확인) | 나머지 TC3xx(TC37x 등): 칩별 타깃 정의(WP-9), CPU5 CSFR 창 처리 |
| 3 | 플래시 | 모든 칩, PFLASH와 DFLASH, PPU/CDSP HEX | TC27x와 TC36x의 PFLASH(TC3xx 로더는 `e1f1952`). DFLASH와 UCB 없음 | 칩 계열별 로더와 DFLASH 시퀀스 |
| 4 | Live Watch | 실행 중에 전역 변수를 주기적으로 갱신(최소 200 ms, `aurix-tcf/liveWatchEvaluate`) | **구현함.** VS Code `Live Watch` 뷰(확장 `najari.probe-rs-live-watch`)가 DAP `liveWatch` 요청으로 실행 중에 갱신. 코어를 멈추지 않고 읽으며 요청당 약 50 ms. 세션 하나로 여러 코어를 볼 때는 코어마다 읽고 **Select Cores**로 고른다(확장 0.2.0, `liveWatch`의 `core` 인자) | 완료(`2561449`, 코어 선택은 확장 0.2.0) |
| 5 | 주변장치 레지스터 쓰기 | Peripheral Viewer 확장(`mcu-debug.peripheral-viewer`)으로 읽기와 쓰기 | **구현함.** `Peripherals`에서 레지스터는 통째로, 필드는 read-modify-write로 씀. 값은 0x/0b/10진수, 폭을 넘으면 거부. 읽으면 값이 바뀌는 레지스터의 필드는 거부. 실기: STM2 `CMCON.MSTART1`, `CMP1` | 완료(`d7b3b30`) |
| 6 | 리셋 후 진입 함수에서 정지 | `entryFunction`(예: `core0_main`)까지 실행해 멈춤. 리셋 뒤 모든 코어 정지 | **구현함.** 코어 설정 `entryFunction`으로 리셋 뒤(`launch`·`restart`) 진입 함수까지 실행해 멈춤(이유 `entry`). 임시 브레이크포인트는 멈추면 지움. 멈추는 것은 해당 코어뿐. 실기: 리셋으로 시작하는 `core0_main` 이미지로 확인(PF0의 리셋 점프를 잠시 바꾸고 백업으로 복원) | 완료(`68a5f6a`) |
| 7 | 브레이크포인트 개수 | TCF 에이전트가 관리(RAM 코드에는 SW 브레이크포인트) | **구현함.** 코어 자신의 PSPR(RAM 코드)에 있는 BP는 `debug` 명령을 심는 SW 브레이크포인트라 8개 제한이 없다(슬롯 40개). 메모리 뷰와 디스어셈블리는 원래 코드를 보여 준다. PFLASH 등 다른 곳은 코어당 HW 8개. 실기: RAM 데모에서 BP 12개, 동시 정지와 함께 사용 | 완료(`ad00ecc`). 캐시가 있는 LMU와 다른 코어의 PSPR은 HW만 쓴다 |
| 8 | 식 평가 | TCF Expressions 서비스(C 식) | probe-rs 평가기(변수 이름 위주, 연산 제한) | probe-rs 업스트림 기능에 의존 |
| 9 | PPU, CDSP, SCR 코어 | ELF를 붙여 디버깅 | 없음 | TriCore가 아닌 아키텍처라 범위가 큼 |
| 10 | 설정 UI | 설정 패널, 장치 검출 버튼, SVD 선택 명령 | `launch.json` 직접 작성. 장치와 SVD는 자동 매칭 | 필요하면 간단한 명령 추가 |

## TriCore 실기 확인 결과

probe-rs DAP 서버가 지원한다고 알리지만 TriCore에서 돌려 보지 않았던 기능 7가지를 2026-10-23에 TC275 ShieldBuddy에서 확인했다. 모두 통과했다. DAP와 GDB 프로토콜을 직접 보내는 Node 스크립트로 시험했다.

| 기능 | 결과 | 확인 내용 |
| --- | --- | --- |
| 명령어 단위 스텝(`supportsSteppingGranularity`) | 통과 | `0x8021C008` → `C00C` → `C00E` → `C024`(`ret` 다음은 호출한 함수로 돌아감) |
| 명령어 브레이크포인트(`supportsInstructionBreakpoints`) | 통과 | `0x8021C008`(`square`)에서 정지 |
| Memory Inspector(`readMemory`, `writeMemory`) | 통과 | `g_result` 46 읽기, 12345 쓰기 후 메모리와 Live Watch로 확인, 원래 값 복구 |
| 변수 값 바꾸기(`setVariable`) | 통과 | `scale`의 로컬 변수 `k` 3 → 5, 복구 |
| Restart(리셋) | 통과 | 재시작 후에도 세션이 계속 동작 |
| `launch`와 `flashingEnabled` 조합 | 통과 | 9.3 s. PF1 시험 섹터만 대상, 끝난 뒤 PF1 내용이 데모 이미지와 같음. 리셋 후 `0xA0000020`에서 정지, 이어서 실행 |
| GDB 서버(`probe-rs gdb`) | 통과(11항목) | `qSupported`, `target.xml`(레지스터 40개, `pc` = 32번), 코어 3개가 스레드로 나옴, `g`·`m` 읽기, `Z1` HW 브레이크포인트와 `hwbreak` 정지, `s` 스텝, 인터럽트(0x03), detach |

GDB 서버는 처음에 `[::1]:1337`(IPv6)에서만 대기했다(업스트림 동작). 주소마다 리스너를 열도록 고쳐(`68a5f6a`) 이제 `127.0.0.1:1337`로도 접속되고, 같은 11항목이 통과했다.

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

동시 정지(1번), Live Watch(4번), 주변장치 쓰기(5번), 진입 함수 정지(6번, 실제 `core0_main` 리셋 이미지 포함), SW 브레이크포인트(7번), 미시험 기능 확인, GDB 서버 IPv4 대기는 끝났다. 남은 것은 **칩과 플래시 범위 확대(2·3번)**로, TC36x는 더했고 TC37x 등 나머지 칩은 해당 보드가 있어야 한다.

8–10번(식 평가, PPU·CDSP·SCR, 설정 UI)은 필요가 생기면 다룬다.

## 출처

- [ZeZhan/vscode-aurix-tcf-debugger](https://github.com/ZeZhan/vscode-aurix-tcf-debugger): `README.md`, `package.json`(명령, 뷰, 설정), `src/debugProvider.ts`(DAP 기능 플래그, `entryFunction`, 전체 코어 정지), `src/liveWatchView.ts`, `src/svdCatalog.ts`
- [najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore): DAP 기능 플래그(`adapter.rs`, `debugger.rs`), `rpc/functions/debug_vars.rs`(변수와 SVD), 실기 시험 기록
- [probe-rs TriCore · MCD · MCDS 연동 개발지시서 (PRD)](https://claude.ai/artifact/GqYnnvgJjCWxkZUUsqJSzq): 작업 패키지와 시험 결과
