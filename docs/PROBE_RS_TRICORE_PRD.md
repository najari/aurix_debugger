# probe-rs TriCore · MCD · MCDS 연동 개발지시서 (PRD)

2026-10-23 · 원본(다이어그램 포함): [Claude 문서](https://claude.ai/artifact/GqYnnvgJjCWxkZUUsqJSzq) · 구현 현황 갱신 2026-10-23

## 0. 구현 현황 (2026-10-23)

이 보드(ShieldBuddy TC275, DAS JTAG AURDUINO)로 검증할 수 있는 작업은 모두 끝났다. 남은 것은 TC37x 보드가 있어야 하는 WP-9와, 맵 1을 여는 어댑터가 있어야 하는 T-15뿐이다. WP-0(업스트림 설계 제안)은 하지 않기로 했다. PRD 범위 밖으로 코어별 세션, SVD·장치 파일 매칭, Live Watch, 주변장치 쓰기, 진입 함수 정지, GDB 서버 수정, 동시 정지, SW 브레이크포인트를 더했다(0.6). TRACE32 비교에서 나온 보완 항목(step 중 인터럽트 막기, 데이터 BP, 성능 카운터, 리셋 처리)은 [보완 개발지시서](AURIX_DEBUGGER_IMPROVEMENT_PRD.md)대로 구현했다(0.6).

- 코드: [najari/probe-rs `tricore` 브랜치](https://github.com/najari/probe-rs/tree/tricore) (probe-rs/probe-rs master `f4d1902` 기반, 커밋 22개, 업스트림 PR 없음)
- 빌드: `cargo build -p probe-rs-tools` (MCD 프로브는 `--features mcd`)

### 0.1 작업 패키지

| WP | 상태 | 커밋 | 내용 |
| --- | --- | --- | --- |
| WP-0 | 하지 않음 | — | 2026-10-23 결정. 셀렉터 VID/PID는 자리표시 값으로 남는다 |
| WP-1 | 완료 | `bc08400` | `Architecture` · `CoreType` · `InstructionSet` · `CoreAccessOptions`의 TriCore 변형, `TriCoreBusAccess`, `TC27x.yaml`, JTAG ID 칩 감지 |
| WP-2 | 완료 | `fa3fdf6` | `TasProbe` · `TasFactory`, 가짜 TAS 서버 테스트, 참조 클라이언트 바이트와 대조한 프레임 테스트 |
| WP-3 | 완료 | `bc08400`, `d8f9b7c` | TriCore 코어(halt · run · step · HW BP 8개 · 레지스터 · 메모리 · 리셋), `ArchitectureInterface::TriCore`, 가짜 CSFR 테스트, 하드웨어 스모크 예제 |
| WP-4 | 완료 | `03842c9` | `TriCoreExceptionHandler`(CSA 언와인드), GCC 11 DWARF 5 단위의 line program v3 처리 |
| WP-5 | 완료 | `da9fc4c`, `c056d07` | `Tc27xFlash`(`DebugFlashSequence`), 로더 소스 · 빌드 스크립트 · 바이너리(946 B, SHA-256 검사), 호스트 측 지우기 |
| WP-6 | 완료 | `51a3bd5` | DAP 경로 수정 2건, `attach` 방식 `launch.json` |
| WP-7 | 완료 | `5bf7ee8` | `McdProbe`(feature `mcd`, `libloading`), `probe-rs info`의 TriCore 항목 |
| WP-8 | 구현 완료, T-15 미검증 | `ef0f17e`, `1ae7058` | `probe-rs mcds record`(FR-S1, S2), `probe-rs mcds timestamp`(FR-S3) |
| WP-9 | 남음 | — | TC37x 보드가 있어야 한다 |

`b42e067`은 rustfmt · clippy 정리다.

### 0.2 하드웨어 시험 결과

| ID | 결과 | 확인한 내용 |
| --- | --- | --- |
| T-01 | 통과 | `probe-rs list`에 TAS 대상 표시. `probe-rs info --verbose`가 JTAG ID 0x401DA083 · SCU\_CHIPID 0x47427060 · 코어 3개(CPU\_ID 0x00B7C002 / 0x00C0C012 ×2) 보고 |
| T-02 | 통과 | DSPR0에 2500 B를 +3 비정렬로 쓰고 읽어 일치, 4 KB 원래 내용 복원. 로컬 DSPR 별칭(0xD000A008)과 전역 주소 값 일치 |
| T-03 | 통과 | 맵 1(0xF00E0080) 읽기가 0이 아니라 `TransferFailed`(코드 0x8D)로 실패 |
| T-04 | 통과 | CPU0–2 각각 halt, CPU0 run → Running → halt → Halted(Request). 다른 코어 무영향은 따로 측정하지 않음 |
| T-05 | 부분 | `main`부터 step 20회, 매번 PC가 바뀌고 정지 이유 = Step. 기준인 100회 · objdump 주소 대조는 하지 않음 |
| T-06 | 통과 | HW BP 8개 설정, 9번째 거부. `square`의 BP에서 Halted(Breakpoint(Hardware)), BP에서 다시 run하면 다음 호출에서 다시 정지 |
| T-07 | 부분 | D0–D15 쓰기 · 읽기 · 복원 일치. PC · A10 · A11 · PCXI · ICR 쓰기는 데모 시작에 사용. A0–A15 전체 대조는 하지 않음 |
| T-08 | 통과 | reset\_and\_halt 후 CPU0가 리셋 벡터 0xA0000020에서 정지, 리셋 후 실행 |
| T-09 | 통과 | `square ← scale ← accumulate ← compute ← main` |
| T-10 | 부분 | DAP에서 `scale`의 지역 변수 `v=0, k=3, s=0` 표시. aurix-tcf 확장 값과 직접 대조는 하지 않음 |
| T-11 | 통과 | PF1 0xA021C000에 쓰기 · 검증(같은 이미지는 건너뜀, 512 B 패턴 16페이지, 데모 복원 6페이지). 앱 섹터 12개가 백업과 일치, 로더 후 ENDINIT 확인 |
| T-12 | 통과 | 비정렬 지우기, PFLASH 밖 주소, 알 수 없는 JTAG ID 등 4건 모두 쓰기 없이 거부 |
| T-13 | 통과 | VS Code probe-rs 확장: `demo.c:17` BP 정지, 콜스택 5단, 변수, Step Over · Into · Out, Continue. 화면 기록은 남기지 않음 |
| T-14 | 통과 | `mcd_tas.dll`로 T-01–T-09(+T-03)이 TAS와 같은 결과(스모크 16/16). MCD 경유 PFLASH 비교(앱 섹터 12개 일치)와 DAP 시험도 통과 |
| T-15 | 미검증 | 이 어댑터는 맵 1을 막는다. `mcds record`가 OCS 읽기(0xF00E6004, 0x8D)에서 "The MCDS registers (EEC address map 1) are not accessible through this probe"로 멈추고, 아무것도 쓰지 않으며 파일도 만들지 않는 것까지 확인 |

플래시 시험은 승인받은 PF1 시험 섹터에서만 했다. 마지막 상태는 PF1에 `demo_flash.elf`, 나머지 섹터는 백업 `C:\work\aurix_flash_backup\2026-10-20_pflash`와 같고, 보드는 리셋돼 앱이 3개 코어에서 돈다.

FR-S3(MCDS 타임스탬프)은 이 어댑터에서도 열린 맵 15로 확인했다. 카운터는 약 1 MHz로 세고, SCU\_CHIPID를 카운터 두 번 사이에 약 1,000 tick 안에 읽는다.

### 0.3 하드웨어 없는 테스트

| 크레이트 | 테스트 | 내용 |
| --- | --- | --- |
| `probe-rs` (`tricore`) | 52개 | 전송 분할, 레지스터 표, 가짜 CSFR로 halt · run · step · BP · 정지 이유, 코어별 세션의 트리거 소유와 묶음 요청, 정지 그룹(라우팅 · 복원, 함께 실행, 리셋 뒤 재설정, 재부팅 중 상태 확인, 놓친 정지 보완), SW 브레이크포인트(`debug` 명령 심기 · 복원, 읽기 가림, 넘어가기, 트리거 유닛과 섞어 쓰기), step 중 인터럽트 막기(IE 판별, 복원), 데이터 BP(단일 · 범위 유닛, 정지 이유, 프로그램 BP와 나눠 쓰기), 성능 카운터 구간, 앱 리셋 감지(BP 복원, 정지, 알림) |
| `probe-rs-debug` (`tricore`) | 3개 | CSA 체인 언와인드 |
| `probe-rs-infineon` | 54개 (`--features mcd`) | TAS 프레임 · 가짜 서버, 섹터 맵 54개 · 로더 해시 · 페이지 패치, MCD 프로브(가짜 MCD 함수 · 구조체 레이아웃), MCDS 기록 · 타임스탬프(TC27x ED 버스 모델), SVD · AURIX Flasher 장치 파일 매칭 |
| `probe-rs-tools` | 197개 | 기존 테스트 회귀 없음. SVD `.svd.gz` 파싱, SVD 값 파싱 · 필드 병합, 진입 함수 심볼 검색 추가 |

`cargo fmt`, `cargo clippy -D warnings`가 `mcd` 기능을 켠 상태와 끈 상태 모두에서 통과한다. probe-rs `smoke-tester`의 TriCore 케이스는 만들지 않았고, 대신 `probe-rs-infineon/examples/`(`hw_smoke`, `pflash_check`, `flash_refusals`, `start_in_flash`, `tas_info`, `trigger_switch`, `bus_read`, `step_interrupts`, `perf_counters`, `sw_reset`)로 하드웨어를 시험한다. `step_interrupts`는 step이 끝난 곳과 `ICR`을, `perf_counters`는 실행 중 카운터와 구간 측정을, `sw_reset`은 앱처럼 SW 리셋을 요청해 남는 디버그 설정을 보인다. `start_in_flash`는 RAM 세그먼트를 써 넣어 RAM 데모도 시작하고, `bus_read`는 코어를 멈추거나 다시 실행시키지 않고 메모리를 읽는다(`probe-rs read`는 끝날 때 모든 코어를 다시 실행시킨다). `PROBE_RS_PROBE`로 프로브를 바꿀 수 있다.

### 0.4 이 문서와 달라진 점

| 항목 | 문서 | 구현 |
| --- | --- | --- |
| `CoreType` (8.2) | `Tc16E`, `Tc16P` | `TriCore` 하나. 1.6E와 1.6P는 디버그 방식이 같다 |
| 칩 감지 (8.2, 8.3) | `ChipDetectionMethod::Infineon { jtag_id, chip_id, variants }` | 벤더 훅 `try_detect_chip_from_probe`가 JTAG ID(제조사 0x41, 부품 번호 0x01DA)로 `TC275`를 고른다. CHIPID로 변형을 나누지 않는다 |
| `ArchitectureInterface` (8.2) | `TriCore(Box<dyn TriCoreBusAccess>)` | `TriCore(Probe)`. 버스는 `DebugProbe::try_as_tricore_bus()`로 얻는다 |
| `BusTransfer` (8.1) | Read / Write enum | 구조체 `{ map, address, kind, data, status: Option<TransferStatus> }`. 프로브는 첫 실패 전송에서 멈춘다 |
| 오류 (8.5) | `Pl0Status`, `AccessDenied` 등 | `TriCoreError::TransferFailed { map, address, len, code }`(0x8D 포함), `TransferNotExecuted`, `CoreDidNotHalt`, `NotHalted`, `Timeout` 등. 플래시 오류는 플래시 시퀀스가 낸다 |
| PFLASH 영역 (8.3) | 0xA000\_0000 주, 0x8000\_0000 별칭 | 0x8000\_0000(캐시, ELF 링크 주소)이 주 영역이고 0xA000\_0000이 `is_alias`. 플래셔가 0xA000\_0000으로 바꿔 쓴다. 섹터 크기별로 영역 10개씩 |
| TAS 셀렉터 (8.4) | `<VID>:<PID>:<host>[:port][/<target>]` | `058b:0000[:[host[:port]/]target]` (예: `058b:0000:DAS JTAG AURDUINO 46YPEADW`) |
| MCD 셀렉터 (8.4) | `--probe mcd:<dll 경로>` | probe-rs 셀렉터는 `VID:PID`만 받으므로 `058b:0001:<dll 경로>[\|<서버 설정>]`, 또는 `PROBE_RS_MCD_DLL` · `PROBE_RS_MCD_CONFIG` |
| MCD 헤더 (NFR-5) | SPRINT 헤더 없이 FFI 타입 직접 정의 | 헤더 파일은 넣지 않았다. Rust로 옮긴 `ffi.rs`에 SPRINT 라이선스가 요구하는 저작권 고지와 변경 고지를 넣었다 |
| DAP (UC-4) | `launch` | probe-rs DAP의 `launch`는 항상 리셋하므로 `attach`를 쓴다. 리셋으로 시작하는 이미지는 `launch`와 `entryFunction`으로 진입 함수까지 실행할 수 있다(0.6) |
| MCDS 설정 입력 (UC-7) | `--config <cfg>` | Infineon 설정기가 만든 `.mcds` 파일(`START_OF_MCDS_CONFIG`) 또는 `주소 값` 줄 목록. 정리 설정은 `--cleanup`, HAR 기록은 `--reset` |

### 0.5 남은 것과 알려진 제한

- **WP-9 (TC37x)**: TC37x 보드가 필요하다.
- **T-15 (MCDS 기록)**: 이 보드는 EMEM이 읽히는 TC27x ED다. 맵 1을 여는 어댑터(예: DAP miniWiggler)나 TriBoard가 있으면 검증할 수 있다.
- **FR-P1 유휴 PING**: `Connection::ping()`은 있지만 유휴 중에 보내지 않는다.
- **FR-C5 (SW 브레이크포인트, Could)**: 코어 자신의 PSPR에서만 쓴다(0.6). 다른 메모리(PFLASH, LMU, 다른 코어의 PSPR)의 BP는 트리거 유닛 8개를 쓴다. 세션이 `disconnect` 없이 끝나면(디버거 강제 종료) `debug` 명령이 RAM에 남으므로 프로그램을 다시 올려야 한다. **FR-V3 (주변장치 뷰, Could)**는 Infineon 레지스터 파일 대신 Infineon PD SVD로 구현했다(0.6).
- **FR-F5**: 쓰기 전에 섹터를 읽어 같으면 건너뛰고, 이웃 섹터 무변화는 `pflash_check`로 검증한다. 백업 파일 저장 옵션은 없다.
- **`probe-rs erase`**: 호스트 측 시퀀스로 PFLASH 전체를 섹터 단위로 지운다. 시험 보드에서 실행하지 말 것.
- **Step Into**: probe-rs의 GCC 프롤로그 처리 때문에 함수 첫 줄에서 멈춘다.
- **MCD 프로브**: 서버의 첫 장치 · 첫 코어만 연다. 맵 0만 지원한다(맵 1 · 15는 0x8D로 거부). 원본 Infineon `mcdxdas.dll`로는 시험하지 않았다.
- **원격 TAS 서버와 DAP**: TAS 목록은 로컬 서버만 나열하므로, DAP · VS Code에서 원격 TAS 셀렉터는 아직 열리지 않는다.
- **VS Code 설정**: 코어별 `launch.json`(CPU0–CPU2와 compound) · `tasks.json`을 이 저장소의 `m25_tcf/examples/aurix_tcf_demo`에 옮겼다(`8e849cc`).
- **동시 정지 제한**: `allStop`(0.6)을 켠 코어끼리만 함께 멈춘다. 자기 BP에 멈춘 코어는 다른 코어를 continue해도 남는다(그 코어의 세션에서 continue). 그룹 코어 하나가 멈춘 채 남아 있으면 다른 코어의 BP는 나머지를 하드웨어로 멈추지 못하고, 그 코어의 세션이 정지를 확인할 때(폴링, 수십 ms 안) 멈춘다. 세션이 attach하는 동안 이미 돌던 그룹 코어가 잠깐 멈췄다가 다시 돈다.
- **보완 기능 제한**: 데이터 BP는 주소만 비교한다(DAP `condition`은 거부). Watch 표현식에서는 데이터 BP를 걸 수 없고, 변수 뷰나 주소로 건다. 성능 카운터는 DAP 레지스터 뷰에서 읽기만 되고, 쓰기는 GDB나 probe-rs API로 한다. 리셋은 상태를 확인할 때(폴링) STM0 타이머가 줄어든 것으로 감지하므로, 리셋 직후 확인 전에 시작 코드가 BP 자리를 지나가면 그 BP는 놓친다(PFLASH의 트리거 유닛은 리셋에도 남아 놓치지 않는다). 플래시 쓰기 직후 리셋의 0x44 오류는 PF0에 리셋 이미지가 없으면 재현되지 않아 재시도로만 막았다.

### 0.6 PRD 이후 추가한 기능

VS Code 디버깅을 aurix-tcf 확장 수준에 맞추려고 더한 기능이다. 모두 TC275 ShieldBuddy에서 확인했고, 비교는 [probe-rs TriCore vs aurix-tcf 확장 기능 비교](PROBE_RS_VS_AURIX_TCF.md)에 있다.

| 커밋 | 기능 | 실기 확인 |
| --- | --- | --- |
| `54fe074` | 코어별 DAP 세션. 세션은 첫 step·BP 변경 때 그 코어의 트리거 유닛을 맡고, 닫을 때 맡은 코어만 정리한다. 스택 추적은 요청한 코어만 멈춘다 | CPU0–CPU2 세션 3개를 compound로 묶어 코어별 정지. 다른 세션을 열고 닫아도 BP 유지 |
| `ff7ddc0` | Infineon PD SVD 번들과 칩별 자동 매칭(TC275 → TC27XDPD, `.svd.gz`). AURIX Flasher 장치 파일을 설치된 aurix-tcf 확장에서 실행 중에 읽어 PFLASH 맵·금지 섹터를 확인(배포하지 않음) | DAP `Peripherals`에 TC27XDPD 주변장치, `probe-rs info`에 장치 파일·SVD 표시 |
| `2561449` | DAP `liveWatch` 요청과 VS Code 확장 `najari.probe-rs-live-watch`. 실행 중에 전역 변수를 읽는다 | `g_ticks`가 실행 중 증가, 요청당 약 50 ms, VS Code 뷰 표시 |
| `d7b3b30` | `Peripherals`의 레지스터·필드 쓰기. 필드는 read-modify-write, 폭 검사 | STM2 `CMCON.MSTART1`·`CMP1` 쓰기와 복원, 폭을 넘는 값 거부 |
| `68a5f6a` | `entryFunction`: 리셋 뒤 진입 함수까지 실행(이유 `entry`). GDB 서버가 `[::1]`과 `127.0.0.1` 모두에서 대기 | 시험용 ELF로 launch·restart 정지와 BP 제거 확인. GDB 11항목 통과. 리셋으로 시작하는 `core0_main` 이미지(`reset_demo.elf`, PF0의 리셋 점프를 잠시 바꿈)로 launch·restart 정지 9항목 통과, PF0는 백업으로 되돌려 전 섹터 일치 확인 |
| `84c21eb` | 동시 정지(DAP `allStop`). OCDS 트리거 스위치의 라인 7에 각 코어의 HALT와 BRKIN을 연결(`CBS_TRCn`, `EXEVT`)해 한 코어가 멈추면 나머지도 하드웨어로 멈춘다(이유 `external`). continue는 그룹이 멈춘 코어를 함께 실행. 다른 세션이 리셋해도 세션이 끊기지 않고 라우팅을 다시 쓴다 | 세션 3개 시나리오 14항목(BP·pause·step·continue, 세션 종료 후 복원)과 리셋 시나리오 6항목 통과. 정지 이벤트는 폴링 때문에 40–60 ms 안에 도착 |
| `ad00ecc` | SW 브레이크포인트(FR-C5). 코어 자신의 PSPR에 있는 BP는 `debug` 명령(16·32비트)을 심고, 다른 곳은 트리거 유닛을 쓴다. 슬롯은 40개(트리거 8 + SW 32). 읽기는 원래 코드를 보여 주고, continue·step은 원래 명령을 한 번 실행한 뒤 다시 심는다. 동시 정지 보완: 다른 코어를 먼저 실행, 놓친 정지는 세션이 확인할 때 멈춤 | RAM 데모로 BP 12개를 실행 순서대로 적중, 16·32비트 명령에서 step, 메모리·디스어셈블리 표시, 해제·`disconnect` 뒤 코드 복원 13항목. 세션 3개로 SW BP 동시 정지 5항목. 기존 동시 정지 20항목, DAP 10항목, GDB 11항목 회귀 없음 |
| `79dd08d` | step 중 인터럽트 막기(FR-I). step하는 동안 `ICR.IE`를 끄고 되돌린다. IE를 바꾸는 명령(`enable`, `disable`, `bisr`, `rfe`, `rfm`, `ICR`에 쓰는 `mtcr`)에서는 막지 않는다. DAP 코어 설정 `stepInterrupts`(`maskInstruction` 기본, `allow`) | T-20: 앱만으로는 step 50·200회 동안 ISR 진입 0회. T-21: SRPN 60을 매 step 대기시키면 `allow`는 10/10 ISR, 기본값은 0회이고 IE 복원. T-22: IE를 바꾸는 명령에서 IE 0, 1, 1, 0, 0 |
| `6826341` | 데이터 BP(FR-D). 트리거 유닛을 데이터 주소용으로 쓴다(읽기·쓰기·둘 다, 1바이트는 유닛 하나, 그 이상은 짝수·홀수 유닛 범위). 정지 이유 `data breakpoint`. 코어 API `set_watchpoint`·`clear_watchpoint`, RPC `core/watchpoint`, DAP `dataBreakpointInfo`·`setDataBreakpoints`, GDB `Z2`–`Z4` | T-23–T-28: `g_result` 쓰기 BP가 `st.w` 다음에서 6회 정지, `g_ticks` 읽기 BP, 해제 뒤 정지 없음, 유닛 부족 거부, `allStop`, GDB `watch`·`rwatch`·`awatch` |
| `f0d9b30` | 성능 카운터(FR-P). `CCTRL`·`CCNT`·`ICNT`·`M1CNT`–`M3CNT`를 레지스터 뷰 "Performance"에 보이고, 두 주소 사이 구간만 센다(DAP 코어 설정 `perfWindow {start, stop}`, 남는 유닛 2개 사용) | T-29: `compute` 실행 동안 `ICNT` +24(정확). T-30: `square` 창에서 `compute` 1회당 24.0 명령(30만 회 평균). T-31: 실행 중 읽기 |
| `fbf362f` | 앱 리셋 처리(FR-R). 상태 확인 때 STM0 타이머가 줄면 리셋으로 보고 원인(`SCU_RSTSTAT`)을 DAP 콘솔에 알린다(RPC `core/notices`). 트리거 유닛·SW BP를 다시 쓰고, DAP 코어 설정 `resetBehavior`(`restore` 기본, `halt`)대로 계속 실행하거나 멈춘다. 리셋 직후 실패하는 접근은 재시도한다 | T-32: SW 리셋은 디버그 설정을 그대로 둔다. T-33–T-35: 콘솔 알림, 시작 코드 BP 유지·적중, `halt` 정지. T-36: PF1 재기록+리셋 10회 오류 0 |

그 밖에 명령어 단위 스텝, 명령어 BP, Memory Inspector, `setVariable`, Restart, `launch`와 플래시 쓰기, GDB 서버를 실기로 확인했다.

## 1. 개요

probe-rs에 Infineon AURIX TriCore(1차 대상 TC27x) 아키텍처를 추가하고, TAS 서버를 첫 프로브로, MCD를 선택 플러그인으로, MCDS를 트레이스 기록 기능으로 붙인다. 이 문서는 구현자가 그대로 작업할 수 있도록 요구사항, 인터페이스, 작업 순서, 수용 기준을 정한다.

**한 줄 목표**: TC275 ShieldBuddy에서 `probe-rs download`로 ELF를 PFLASH에 쓰고, VS Code(probe-rs DAP)에서 소스 브레이크포인트·스텝·콜스택·변수를 쓸 수 있다.

**성공의 정의**

| 항목 | 기준 |
| --- | --- |
| 디버그 | halt / run / step / HW 브레이크포인트 / 레지스터 / 메모리 / CSA 콜스택이 TC275 CPU0–2에서 동작 |
| 플래시 | PF1 시험 섹터(0xA021C000)에 쓰기·검증 통과, 그 밖의 섹터는 바이트 단위로 변화 없음 |
| VS Code | probe-rs 확장으로 `demo_flash.elf` 소스 디버깅 (현재 aurix-tcf 확장에서 확인한 것과 같은 범위) |
| MCDS | 기록을 시작·정지하고 `.mcds` 파일로 저장 (디코드는 Infineon 도구) |
| 업스트림 | probe-rs 코드 규칙(edition 2024, MSRV 1.95, MIT OR Apache-2.0)과 CI 통과 |

**관련 문서**: [probe-rs × MCD / MCDS 연동 기술 검토](PROBE_RS_TRICORE_REVIEW.md) — 방식 비교와 근거는 그 문서에 있고, 이 문서는 결정된 방식의 구현 지시만 담는다.

## 2. 배경과 문제

TC275 디버그에 필요한 프로토콜과 레지스터 시퀀스는 이미 이 저장소에서 하드웨어로 검증됐다. 문제는 그것이 C/C++/JavaScript 도구 네 개에 흩어져 있고, 범용 디버그 도구인 probe-rs에는 TriCore가 없다는 점이다.

**재사용할 현재 자산** ([najari/aurix\_debugger](https://github.com/najari/aurix_debugger))

| 자산 | 검증된 내용 | probe-rs에서 쓰는 곳 |
| --- | --- | --- |
| `mcd_tas/src/tas_client.*` | TAS PL2/PL0 프레이밍, SERVER\_CONNECT · GET\_TARGETS · SESSION\_START · DEVICE\_CONNECT, PL0 실행기 | TAS 프로브 드라이버(Rust로 재작성) |
| `m25_tcf/agent/.../context-aurix.c` | halt / run / step / 리셋, TR0–7 트리거, `debug` 명령(0x00A0) | TriCore `CoreInterface` |
| `m25_tcf/.../cpudefs-mdep.c` | CSFR 오프셋, DWARF 레지스터 번호 | 레지스터 파일 정의 |
| `m25_tcf/.../dwarfframe.c`, `stack-crawl-tricore.c` | CSA upper context에서 호출자 복원 | probe-rs-debug TriCore 언와인더 |
| `m25_tcf/flasher/m25_flash_loader.c` + `tools/m25_4b_flash.js` | TC27x PFLASH 지우기·쓰기·검증, 54개 논리 섹터 맵, ECC 인식 읽기 | `DebugFlashSequence` 구현 |
| `mcd_tas` (MCD API 54개 함수) | SPRINT MCD v1.1, 원본 DLL과 교차 캘리브레이션 | 선택 플러그인 프로브의 상대편 |
| `mcdsrl` (22개 export) | MCDS 기록 시퀀스, `.mcds` 파일 형식 | MCDS 기록 모듈 |

**probe-rs 쪽 제약** (master `f4d1902`, 0.32.0 기준)

- `Architecture` · `CoreType` · `InstructionSet`은 Arm / Riscv / Xtensa만 가진 닫힌 enum이다.
- `DebugProbe`는 아키텍처별 훅(`try_get_arm/riscv/xtensa_interface`)만 있고, TAS처럼 메모리 맵으로만 접근하는 프로브를 담을 자리가 없다.
- `Flasher::call_function`은 ARM 호출 규약(인자 레지스터 0–3, 복귀 주소)을 전제한다. 반면 `DebugFlashSequence`(호스트 측 플래시)는 아키텍처와 무관하다.
- 업스트림 이슈 [#2796 Infineon Tricore architecture support](https://github.com/probe-rs/probe-rs/issues/2796)는 2024-09-03부터 열려 있고, 구현 PR은 아직 없다.

## 3. 범위

1차 릴리스는 TC27x + TAS TCP 프로브의 디버그·플래시·DAP까지다. MCD 플러그인과 MCDS 기록은 2차, 네이티브 JTAG은 범위 밖이다.

| 구분 | 항목 | 단계 |
| --- | --- | --- |
| 포함 | `Architecture::TriCore`, TC1.6P/TC1.6E 코어 (TC275 CPU0–2) | 1차 |
| 포함 | TAS TCP 프로브 (`tas`), 주소 맵 0 / 1 / 15, 리셋 | 1차 |
| 포함 | halt, run, step, HW 브레이크포인트 8개(TR0–7), 레지스터, 메모리 | 1차 |
| 포함 | CSA 기반 콜스택과 DWARF 변수 (probe-rs-debug) | 1차 |
| 포함 | TC27x PFLASH 쓰기 (`DebugFlashSequence` + RAM 로더) | 1차 |
| 포함 | DAP 서버를 통한 VS Code 디버깅 | 1차 |
| 포함 | MCD 플러그인 프로브 (`mcd`, 외부 DLL 동적 로드) | 2차 |
| 포함 | MCDS 기록 시작·정지·`.mcds` 저장 | 2차 |
| 포함 | TC37x 타깃 (플래시 레지스터·섹터 맵 차이만 추가) | 2차 |
| 제외 | MCDS 트레이스 디코드 (Infineon 도구에 맡김) | — |
| 제외 | FTDI 등 네이티브 JTAG/DAP 프로브 (TAS 없이 직접) | 장기 |
| 제외 | UCB · DFLASH 쓰기, 보안(잠금) 해제 | — |
| 제외 | RTT, SWO 같은 ARM 트레이스 기능의 TriCore 대응 | — |

**전제 조건**

- 대상 보드: ShieldBuddy TC275 (CBS\_JTAGID `0x401DA083`, SCU\_CHIPID `0x47427060`, PFLASH 4 MB), 어댑터 `DAS JTAG AURDUINO 46YPEADW`.
- 호스트에 Infineon DAS / TasServer가 설치돼 TCP 24817에서 대기한다. TasServer는 비공개 Windows 바이너리다.
- 이 어댑터에서 주소 맵 1(EEC/MCDS)은 PL0 `0x8D`로 막혀 있다. MCDS 기록을 실제로 검증하려면 ED 칩과 맵 1을 허용하는 환경이 따로 필요하다.
- 툴체인: Rust stable(MSRV 1.95), 로더·테스트 이미지용 `tricore-elf-gcc`.

## 4. 사용자와 사용 시나리오

주 사용자는 AURIX 펌웨어 개발자이고, 보조 사용자는 CI에서 하드웨어 테스트를 돌리는 담당자다. 아래 일곱 시나리오가 수용 테스트(10절)의 기준이 된다.

| ID | 시나리오 | 명령 / 동작 | 단계 |
| --- | --- | --- | --- |
| UC-1 | 프로브와 코어 확인 | `probe-rs list` → TAS 타깃 표시, `probe-rs info --chip TC275` → JTAG ID·코어 3개 | 1차 |
| UC-2 | PFLASH 쓰기 | `probe-rs download --chip TC275 build/demo_flash.elf` → 백업·지우기·쓰기·검증 | 1차 |
| UC-3 | 쓰고 실행 | `probe-rs run --chip TC275 <elf>` → 리셋 후 실행, Ctrl+C에 halt + 콜스택 출력 | 1차 |
| UC-4 | VS Code 소스 디버깅 | probe-rs 확장 + `launch.json`(`chip: TC275`, `coreIndex: 0`) → BP, Step Over/Into/Out, 콜스택, 지역 변수 | 1차 |
| UC-5 | 멀티코어 | 같은 세션에서 CPU1·CPU2를 따로 halt / run, 로컬 SPR 별칭 주소 변환 | 1차 |
| UC-6 | MCD 도구 사용 | `--probe mcd:<dll 경로>` → 같은 디버그 기능을 MCD DLL 경유로 | 2차 |
| UC-7 | MCDS 기록 | `probe-rs mcds record --config <cfg> --out run.mcds` → 기록 시작·정지·저장 | 2차 |

UC-4의 기준선은 현재 aurix-tcf 확장으로 확인한 동작이다(`m25_tcf/examples/aurix_tcf_demo`: `main → compute → accumulate → scale → square`).

## 5. 아키텍처 개요

새 코드는 probe-rs 본체(TriCore 코어·언와인더·타입)와 새 벤더 크레이트 `probe-rs-infineon` 두 곳에 들어간다. 코어는 `TriCoreBusAccess`만 알기 때문에 TAS든 MCD든 같은 코어 코드를 쓴다.

> 다이어그램: probe-rs TriCore 구성 · 새 크레이트 1개, 새 트레이트 1개 — [원본 문서](https://claude.ai/artifact/GqYnnvgJjCWxkZUUsqJSzq)에서 볼 수 있다.

플래시는 기존 `HostSideFlasher` 경로를 그대로 타고, TC27x 시퀀스는 `DebugFlashSequence`로 넘겨진다. MCDS 기록도 같은 버스 트레이트의 맵 1을 쓴다.

## 6. 기능 요구사항

1차 릴리스는 Must 21개를 모두 충족해야 한다. 각 행의 근거는 이 저장소에서 하드웨어로 검증한 시퀀스이며, 수치는 부록 A의 레지스터 맵을 따른다.

| ID | 영역 | 요구사항 | 근거 / 검증 원본 | 우선순위 |
| --- | --- | --- | --- | --- |
| FR-P1 | TAS 프로브 | SERVER\_CONNECT(0x80) → GET\_TARGETS(0x86) → SESSION\_START(0xA1) → SET\_DEVICE\_KEY(0xAD) → DEVICE\_CONNECT(0xA8)로 연결하고, 유휴 시 PING(0xA2)을 보낸다 | `mcd_tas/src/tas_client.cpp` | Must |
| FR-P2 | TAS 프로브 | 8/16/32비트·블록 읽기/쓰기. PL0 프레임당 트랜잭션 32개, 블록당 1 KB를 넘지 않게 나눈다 | `tas_client.cpp` PL0 실행기 | Must |
| FR-P3 | TAS 프로브 | 접근마다 주소 맵을 고른다: 0 = 타깃, 1 = EEC/MCDS, 15 = 서버 내부 | `mcdsrl` 접근 매트릭스(`map_probe.cpp`) | Must |
| FR-P4 | TAS 프로브 | 리셋은 DEVICE\_CONNECT 옵션 0x0003으로 하고, 다시 연결해 세션을 이어 간다 | `mcd_tas` M17 | Must |
| FR-P5 | TAS 프로브 | PL0 상태 코드를 probe-rs 오류로 바꾼다 (0x8D = 접근 거부 등). 실패한 읽기를 0으로 채워 돌려주지 않는다 | M25 12절 결함 1 | Must |
| FR-C1 | TriCore 코어 | halt: DBGSR ← 0x6, DBGSR.HALT = 01b로 정지 확인 | `context-aurix.c` | Must |
| FR-C2 | TriCore 코어 | run: DBGSR ← 0x4 | `context-aurix.c` | Must |
| FR-C3 | TriCore 코어 | 한 명령 step: 디버그 초기화 → CBS 0xF0000494 ← 0x8000003E → DBGSR ← 0x4 → 0494 ← 0x8000003F | `context-aurix.c`, `runctrl.c` | Must |
| FR-C4 | TriCore 코어 | HW 브레이크포인트 8개: TRnEVT(F000 + 8n) ← 0x101A, TRnADR ← 주소. `available_breakpoint_units()` = 8 | `context-aurix.c` | Must |
| FR-C5 | TriCore 코어 | RAM 코드의 SW 브레이크포인트: `debug` 명령(0x00A0) 삽입·복원 | `cpudefs-mdep.c` BREAK\_INST | Could |
| FR-C6 | TriCore 코어 | 정지 이유: DBGSR.EVTSRC = 16 + n → TRn 브레이크포인트, 그 밖은 요청 halt / step | `context-aurix.c` | Must |
| FR-C7 | TriCore 코어 | 레지스터 읽기/쓰기: D0–15(+0xFF00), A0–15(+0xFF80), PCXI(+0xFE00), PSW(+0xFE04), PC(+0xFE08), ICR | `cpudefs-mdep.c` | Must |
| FR-C8 | TriCore 코어 | 코어 n의 CSFR 창 = 0xF8810000 + n × 0x20000. 로컬 별칭 0xD0000000 → 0x70000000 − n × 0x10000000 변환 | `docs/TC27X_DEBUG_REGISTERS.md` | Must |
| FR-C9 | TriCore 코어 | `reset_and_halt`: 리셋 후 리셋 벡터에서 정지 (TR 트리거 또는 BOOT halt) | `mcd_tas` M17, M25-4a | Must |
| FR-U1 | 스택·변수 | CSA 체인 언와인드: CSA = ((PCXI & 0xF0000) << 12) \| ((PCXI & 0xFFFF) << 6), upper context에서 PCXI·PSW·A10·A11·D8–15·A12–15 복원 | `dwarfframe.c`, `stack-crawl-tricore.c` | Must |
| FR-U2 | 스택·변수 | TriCore EABI DWARF 레지스터 번호로 변수 위치를 해석한다 | `cpudefs-mdep.c` | Must |
| FR-U3 | 스택·변수 | 디스어셈블리 뷰 (capstone의 TriCore 지원 여부에 따름, 11절 미결 사항) | `disassembler-aurix-tricore.c` | Should |
| FR-F1 | 플래시 | `DebugFlashSequence` 구현: RAM 로더를 PSPR 0x70100000에 올리고 DSPR 메일박스 0x7000F000로 명령한다 | `m25_flash_loader.c`, `m25_4b_flash.js` | Must |
| FR-F2 | 플래시 | TC27x 논리 섹터 54개 맵 (16K×8, 32K×8, 64K×4, 128K×3, 256K×4, 뱅크 2개), 32 B 페이지 | M25 12절, `TC27x_D_step.json` | Must |
| FR-F3 | 플래시 | 지워진 페이지의 ECC 에러(FLASH0\_FSR PFDBER)를 "지워짐"으로 판정한다. 0으로 읽지 않는다 | M25 12절 결함 1 | Must |
| FR-F4 | 플래시 | CPU0·Safety WDT의 ENDINIT를 iLLD 순서(비밀번호 접근 → 수정 접근)로 내리고 다시 올린다 | M25 12절 결함 3, 4 | Must |
| FR-F5 | 플래시 | 쓰기 전 대상 섹터 백업 옵션, 지운 뒤 다시 쓰기, 이웃 섹터 무변화 검증 | `m25_4b_flash.js` backup | Should |
| FR-V1 | DAP / VS Code | probe-rs DAP 서버가 TriCore 세션에서 BP 설정, Step Over/Into/Out, 콜스택, 지역 변수를 제공한다 | aurix-tcf 확장 기준선 | Must |
| FR-V2 | DAP / VS Code | 코어 선택(`coreIndex` 0–2)과 코어별 정지·재개 | UC-5 | Must |
| FR-V3 | DAP / VS Code | SVD 대신 Infineon 레지스터 파일을 쓰는 주변장치 뷰 | — | Could |
| FR-M1 | MCD 플러그인 | 외부 MCD DLL을 실행 시점에 불러온다 (cargo feature `mcd`, 기본 꺼짐) | `mcd_tas` drop-in 방식 | Should |
| FR-M2 | MCD 플러그인 | MCD 함수는 `mcd_execute_txlist_f`와 `mcd_rst_f`만 쓴다. 나머지 코어 로직은 TriCore 백엔드가 그대로 맡는다 | 기술 검토 문서 ‘연동 방식 비교’ | Should |
| FR-S1 | MCDS | 주소 맵 1로 MCDS 설정 쓰기 → 기록 시작 → 정지 → 트레이스 메모리 읽기 | `mcdsrl_impl.cpp` start/stop\_recording | Should |
| FR-S2 | MCDS | `.mcds` 파일 저장: 텍스트 헤더 + `START_OF_MCDS_CONFIG` (addr, value) 쌍 + `START_OF_TDR` + raw | `mcdsrl` M23-2 | Should |
| FR-S3 | MCDS | 기록 중 타임스탬프(맵 15, 0x50000090) 읽기 | `mcdsrl` read\_timestamped | Could |

## 7. 비기능 요구사항

가장 중요한 비기능 요구는 플래시 안전이다. 잘못된 쓰기는 보드의 앱을 지우고, 지워진 PFLASH를 잘못 읽으면 백업이 깨진다(M25 12절).

| ID | 분류 | 요구사항 |
| --- | --- | --- |
| NFR-1 | 안전 | 알 수 없는 JTAG ID, 섹터 시작이 아닌 지우기, PFLASH 밖 주소, 로더 미실행 상태의 명령은 모두 거부한다 |
| NFR-2 | 안전 | 플래시 작업은 항상 명시적 명령(`download`, `run`, DAP `flashingEnabled`)으로만 시작한다. 테스트 기본 대상은 PF1 0xA021C000 한 섹터다 |
| NFR-3 | 안전 | 로더 실행 후 WDT CON0의 ENDINIT = 1로 복구됐는지 확인하고, 아니면 오류로 끝낸다 |
| NFR-4 | 성능 | PL0 프레임 하나에 트랜잭션을 최대한 묶는다(레지스터 읽기, 트리거 설정). 목표치는 Phase 1에서 측정해 정한다 (11절 미결) |
| NFR-5 | 라이선스 | 모든 새 코드는 MIT OR Apache-2.0. SPRINT `mcd_api.h`는 저장소에 넣지 않고 FFI 타입을 직접 정의한다. 플래시 시퀀스는 iLLD(Boost 1.0) 파생으로 표기한다 |
| NFR-6 | 라이선스 | TasServer와 MCD DLL은 배포물에 넣지 않는다. 사용자가 설치한 것에 연결만 한다 |
| NFR-7 | 빌드 | edition 2024, MSRV 1.95, `cargo fmt` · `clippy -D warnings` 통과. 기본 빌드에 새 필수 의존성을 더하지 않는다(`libloading`은 `mcd` feature 뒤) |
| NFR-8 | 플랫폼 | TAS 프로브는 순수 TCP라 Linux·macOS에서도 원격 Windows 호스트의 TasServer에 붙는다. MCD 플러그인은 DLL이 있는 Windows에서만 제공한다 |
| NFR-9 | 테스트 가능성 | TAS 와이어를 흉내 내는 가짜 서버로 하드웨어 없이 CI에서 프로토콜·코어 로직을 검증한다 |
| NFR-10 | 업스트림 | PR은 작업 패키지 단위(9절)로 나누고, 각 PR은 혼자 빌드·테스트가 통과해야 한다. 착수 전 #2796에 설계를 공유한다 |

## 8. 인터페이스 명세

아키텍처 enum이 닫혀 있어 TriCore 코어는 `probe-rs`와 `probe-rs-target` 본체에 넣고, TAS 프로브·타깃·플래시 시퀀스는 `probe-rs-espressif`처럼 별도 벤더 크레이트 `probe-rs-infineon`에 둔다. 벤더 크레이트는 `plugin::register_plugin`으로 `vendors` · `targets` · `probe_drivers`를 등록한다. 아래 시그니처는 제안이며, 이름은 업스트림 리뷰에서 바뀔 수 있다.

### 8.1 버스 접근 트레이트 (`probe-rs/src/architecture/tricore/bus.rs`)

```rust
/// TAS · MCD처럼 주소 맵 단위 메모리 접근만 제공하는 프로브가 구현한다.
pub trait TriCoreBusAccess: Send + std::fmt::Debug {
    /// 접근 목록을 한 번에 실행한다 (TAS: PL0 프레임 하나, 최대 32개).
    fn execute(&mut self, list: &mut [BusTransfer]) -> Result<(), TriCoreError>;

    fn read_block(&mut self, map: AddressMap, addr: u32, buf: &mut [u8]) -> Result<(), TriCoreError>;
    fn write_block(&mut self, map: AddressMap, addr: u32, data: &[u8]) -> Result<(), TriCoreError>;
    fn read_u32(&mut self, map: AddressMap, addr: u32) -> Result<u32, TriCoreError>;
    fn write_u32(&mut self, map: AddressMap, addr: u32, value: u32) -> Result<(), TriCoreError>;

    fn reset(&mut self, kind: ResetKind) -> Result<(), TriCoreError>;
    fn jtag_id(&mut self) -> Result<u32, TriCoreError>;
    /// 프레임당 최대 트랜잭션 수와 블록 크기 (TAS: 32, 1024).
    fn limits(&self) -> BusLimits;
}

#[repr(u8)]
pub enum AddressMap { Target = 0, Eec = 1, Server = 15 }

pub enum BusTransfer {
    Read  { map: AddressMap, addr: u32, width: Width, data: Vec<u8>, status: Option<u8> },
    Write { map: AddressMap, addr: u32, width: Width, data: Vec<u8>, status: Option<u8> },
}
```

### 8.2 기존 타입에 더하는 것

| 위치 | 변경 |
| --- | --- |
| `probe-rs-target` `Architecture` | `TriCore` 추가 |
| `probe-rs-target` `CoreType` | `Tc16E`, `Tc16P` 추가 (TC275: CPU0 = 1.6E, CPU1–2 = 1.6P) |
| `probe-rs-target` `InstructionSet` | `TriCore` 추가 |
| `probe-rs-target` `CoreAccessOptions` | `TriCore(TriCoreCoreAccessOptions { core_index, csfr_base })` 추가 |
| `probe-rs-target` `ChipDetectionMethod` | `Infineon { jtag_id, chip_id, variants }` 추가 |
| `probe-rs` `DebugProbe` | `has_tricore_interface()`, `try_get_tricore_interface()` 추가 (기본 구현 = 미지원, 기존 xtensa 훅과 같은 모양) |
| `probe-rs` `session.rs` `ArchitectureInterface` | `TriCore(Box<dyn TriCoreBusAccess>)` 변형 추가 |
| `probe-rs` `architecture::tricore` | `TriCore` 구조체가 `CoreInterface` + `MemoryInterface` 구현 |
| `probe-rs-debug` | TriCore `ExceptionInterface` + CSA 언와인더 |

### 8.3 타깃 YAML (`probe-rs-infineon/targets/TC27x.yaml`, 초안)

```yaml
name: TC27x
manufacturer:
  id: 0x41        # JEP106 Infineon (JTAG ID 0x401DA083의 [11:1])
  cc: 0x0
chip_detection:
- !Infineon
  jtag_id: 0x401DA083
  chip_id: 0x47427060
  variants: { 0x47427060: TC275 }
variants:
- name: TC275
  cores:
  - { name: cpu0, type: tc16e, core_access_options: !TriCore { core_index: 0, csfr_base: 0xF8810000 } }
  - { name: cpu1, type: tc16p, core_access_options: !TriCore { core_index: 1, csfr_base: 0xF8830000 } }
  - { name: cpu2, type: tc16p, core_access_options: !TriCore { core_index: 2, csfr_base: 0xF8850000 } }
  memory_map:
  - !Nvm   { name: PFLASH,   range: { start: 0xA0000000, end: 0xA0400000 }, cores: [cpu0, cpu1, cpu2] }
  - !Nvm   { name: PFLASH-c, range: { start: 0x80000000, end: 0x80400000 }, is_alias: true }  # 캐시 별칭
  - !Ram   { name: DSPR0,    range: { start: 0x70000000, end: 0x7001C000 }, cores: [cpu0] }
  - !Ram   { name: PSPR0,    range: { start: 0x70100000, end: 0x70106000 }, cores: [cpu0] }
```

ELF는 보통 캐시 주소(0x8021C000)로 링크되므로, 플래셔는 0x8xxx\_xxxx를 0xAxxx\_xxxx로 바꿔 쓴다. RAM 크기와 `is_alias`의 정확한 의미는 WP-1에서 데이터시트·probe-rs 코드로 확인한다.

### 8.4 프로브 선택

- `probe-rs list`: `TasFactory::list_probes()`가 127.0.0.1:24817에 GET\_TARGETS를 보내 대상을 나열한다. 서버가 없으면 빈 목록.
- `--probe <VID>:<PID>:<host>[:port][/<target>]`: XVC(`0000:0000:host:port`)와 같은 방식. TAS용 자리표시 VID/PID는 XVC와 겹치지 않게 정하며 값은 메인테이너와 합의한다. `<target>`은 GET\_TARGETS가 돌려준 이름(예: `DAS JTAG AURDUINO 46YPEADW`).
- MCD: `--probe mcd:<dll 경로>` 또는 환경 변수 `PROBE_RS_MCD_DLL` (2차).

### 8.5 오류

| `TriCoreError` | 뜻 | probe-rs 대응 |
| --- | --- | --- |
| `Pl0Status(u8)` | PL0 트랜잭션 실패 코드 | `DebugProbeError::Other` |
| `AccessDenied { map }` | 주소 맵 접근 거부 (0x8D) | `Error::NotImplemented` 계열 |
| `NotHalted` | 정지가 필요한 작업을 실행 중에 요청 | `Error::CoreNotHalted`류 |
| `Timeout` | halt · step · 로더 응답 시간 초과 | `DebugProbeError::Timeout` |
| `UnknownDevice(u32)` | 타깃 목록에 없는 JTAG ID | `Error::ChipNotFound` |
| `LoaderNotRunning` | 플래시 로더 magic 불일치 | 플래시 오류 |
| `EndinitStuck` | 로더 후 ENDINIT 복구 실패 | 플래시 오류 |

## 9. 작업 분해와 마일스톤

작업은 열 개 패키지(WP)로 나누고, WP 하나가 업스트림 PR 하나가 되게 한다. WP-1과 WP-2, WP-4와 WP-5는 동시에 진행할 수 있다. 날짜는 WP-0 합의 후에 정한다.

> 다이어그램: 작업 패키지 선행 관계 · WP-0–WP-9, 관문 1개 — [원본 문서](https://claude.ai/artifact/GqYnnvgJjCWxkZUUsqJSzq)에서 볼 수 있다.

| WP | 산출물 (PR) | 주요 위치 | 완료 조건 |
| --- | --- | --- | --- |
| WP-0 | 설계 제안서 (이 문서 요약 + 8절 시그니처) | #2796 코멘트 또는 Discussion | 메인테이너가 방향에 동의, 셀렉터·크레이트 이름 확정 |
| WP-1 | `Architecture::TriCore` 등 타입 추가, `TriCoreBusAccess` 트레이트, `TC27x.yaml`, 칩 감지 | `probe-rs-target/src/`, `probe-rs/src/architecture/tricore/bus.rs`, `probe-rs-infineon/targets/` | 기존 테스트 통과, YAML 스키마 검사, 섹터 54개 일치 |
| WP-2 | `TasProbe`, `TasFactory`, 가짜 TAS 서버 | `probe-rs-infineon/src/tas/` | 가짜 서버 테스트 통과, T-01–T-03 통과 |
| WP-3 | `TriCore` 코어, `ArchitectureInterface::TriCore`, `DebugProbe`의 TriCore 훅 | `probe-rs/src/architecture/tricore/`, `session.rs`, `probe.rs` | 가짜 CSFR 테스트 통과, T-04–T-08 통과 |
| WP-4 | TriCore `ExceptionInterface`, CSA 언와인더 | `probe-rs-debug/src/` | 덤프 기반 테스트 통과, T-09–T-10 통과 |
| WP-5 | TC27x `DebugFlashSequence`, 로더 ELF + 소스 + 빌드 스크립트 | `probe-rs-infineon/src/flash/`, `probe-rs-infineon/loader/` | T-11–T-12 통과, 앱 섹터 12개 백업과 일치 |
| WP-6 | DAP 경로의 TriCore 분기 보완, `launch.json` 예제 | `probe-rs-tools/src/bin/probe-rs/cmd/dap_server/` | T-13 통과 (화면 기록 첨부) |
| WP-7 | `McdProbe` (feature `mcd`) | `probe-rs-infineon/src/mcd/` | `mcd_tas.dll`로 T-14 통과 |
| WP-8 | `probe-rs mcds record` 서브커맨드 | `probe-rs-infineon/src/mcds/`, `probe-rs-tools` | 맵 1 허용 환경에서 T-15 통과 |
| WP-9 | `TC37x.yaml`, TC37x 플래시 레지스터 분기 | `probe-rs-infineon/` | TC37x 보드에서 T-01–T-12 통과 |

## 10. 수용 기준과 테스트 계획

1차 릴리스는 T-01–T-13을 TC275 ShieldBuddy에서 모두 통과하고, 하드웨어 없는 CI 테스트가 녹색이면 완료다. T-14–T-15는 2차 기준이다.

### 10.1 하드웨어 테스트

| ID | 시험 | 합격 기준 | 요구사항 |
| --- | --- | --- | --- |
| T-01 | 연결과 식별 | `probe-rs list`에 TAS 대상이 나오고, `info`가 JTAG ID 0x401DA083 · CHIPID 0x47427060 · 코어 3개를 보고 | FR-P1 |
| T-02 | 메모리 | DSPR0 4 KB 쓰기 → 읽기 일치 (1 KB 블록·트랜잭션 32개 분할 경계 포함), 시험 후 원래 내용 복원 | FR-P2 |
| T-03 | 접근 거부 | 이 어댑터에서 맵 1 읽기가 0이 아니라 `AccessDenied`를 돌려줌 | FR-P3, FR-P5 |
| T-04 | halt / run | CPU0–2 각각 halt → 상태 Halted → run → Running, 나머지 코어는 영향 없음 | FR-C1, FR-C2, FR-V2 |
| T-05 | step | `main`에서 step 100회, 매번 PC가 다음 명령 주소(objdump 기준)와 일치 | FR-C3 |
| T-06 | HW 브레이크포인트 | 8개 설정 성공, 9번째 거부. 각각 도달 시 정지 이유 = 해당 TRn | FR-C4, FR-C6 |
| T-07 | 레지스터 | D0–15 · A0–15 · PC 쓰기 → 읽기 일치, 시험 후 원래 값 복원 | FR-C7, FR-C8 |
| T-08 | reset\_and\_halt | 리셋 후 CPU0가 리셋 벡터에서 정지 | FR-C9, FR-P4 |
| T-09 | 콜스택 | demo `square`의 BP에서 프레임 5개 `square ← scale ← accumulate ← compute ← main` | FR-U1 |
| T-10 | 변수 | 같은 정지 지점에서 지역 변수 값이 aurix-tcf 확장이 보여 주는 값과 같음 | FR-U2 |
| T-11 | 플래시 | `demo_flash.elf`를 PF1 0xA021C000에 쓰기·검증. 앱 섹터 12개가 `C:\work\aurix_flash_backup\2026-10-20_pflash`와 일치, WDT CON0 ENDINIT = 1 | FR-F1–F5, NFR-3 |
| T-12 | 플래시 거부 | 비정렬 지우기, PFLASH 밖 주소, 알 수 없는 JTAG ID가 모두 쓰기 없이 거부됨 | NFR-1 |
| T-13 | VS Code | probe-rs 확장으로 UC-4: BP, Step Over/Into/Out, 콜스택, 지역 변수 | FR-V1 |
| T-14 | MCD 플러그인 | `mcd_tas.dll`을 프로브로 T-01–T-09 재실행, 결과 동일 | FR-M1, FR-M2 |
| T-15 | MCDS | 맵 1이 허용되는 ED 환경에서 기록 → `.mcds` 저장, Infineon 디코더가 파일을 연다 | FR-S1–S3 |

플래시를 바꾸는 T-11·T-12는 실행할 때마다 담당자 승인을 받는다. 시험 전 전체 PFLASH 백업(`backupall`)을 떠 둔다.

### 10.2 하드웨어 없는 테스트 (CI)

- **가짜 TAS 서버**: `mcd_tas` 스모크 로그(`smoke_attach_ok.txt`, `probe_run.txt`)의 바이트 교환을 재생해 FR-P1–P5를 검증한다.
- **가짜 CSFR**: DBGSR · TRnEVT를 흉내 내는 메모리 모델로 halt / run / step / BP 시퀀스의 쓰기 순서를 확인한다.
- **CSA 언와인드**: 하드웨어에서 떠 둔 메모리 덤프 + `demo_flash.elf`로 T-09 결과를 재현한다.
- **타깃**: `target-gen`으로 YAML 스키마를 검사하고, 섹터 맵 54개를 공식 JSON과 비교한다.
- **하드웨어 회귀**: probe-rs `smoke-tester`에 TriCore 케이스를 추가하고, 보드가 연결된 self-hosted 러너에서만 수동으로 돌린다.

## 11. 위험, 제약, 미결 사항

가장 큰 위험은 업스트림이 새 아키텍처를 받아들일지다. 그래서 WP-0(설계 합의)을 코드보다 먼저 둔다.

| 위험 / 제약 | 영향 | 대응 |
| --- | --- | --- |
| 메인테이너가 `Architecture::TriCore` 추가나 버스 트레이트를 거부·지연 | 포크 유지 비용 | #2796에 설계 먼저 제안, 본체 변경을 타입 추가로 최소화, 나머지는 벤더 크레이트에 |
| TasServer가 비공개이고 프로토콜 문서가 없다 | 서버 업데이트로 동작이 바뀔 수 있음 | Apache-2.0 [Infineon/tas\_client\_api](https://github.com/Infineon/tas_client_api)를 기준으로 삼고, 가짜 서버 테스트로 회귀를 잡는다 |
| 이 어댑터에서 맵 1(EEC/MCDS)이 막혀 있다 | T-15를 여기서 검증할 수 없음 | MCDS는 2차로 미루고 맵 1이 열린 ED 환경을 따로 확보한다 |
| 플래시 손상 | 보드 앱 손실 | NFR-1–3, 테스트는 PF1 한 섹터만, 전체 백업 보존, 실행마다 승인 |
| 리셋 직후 코어가 정지된 상태에서 첫 지우기가 시간 초과한 적이 있다 (원인 미확인) | 플래시 실패 | 플래시 전 `reset run`으로 우회(현재 방식), WP-5에서 원인 규명 |
| 디버그 중 앱 워치독이 서비스되지 않는다 | 긴 세션에서 리셋 | 데모는 짧게 실행, 필요하면 디버그 중 WDT 비활성화 옵션을 더한다 |
| probe-rs 디스어셈블리가 TriCore를 모를 수 있다 | 디스어셈블리 뷰 없음 | FR-U3를 Should로 두고 미결 사항에서 결정 |
| 플래시 로더는 `tricore-elf-gcc`로만 빌드된다 | probe-rs CI에서 빌드 불가 | 로더 ELF를 소스·빌드 스크립트와 함께 커밋, CI는 해시만 검사 |

**미결 사항**

- [ ] TAS 프로브의 자리표시 VID/PID와 셀렉터 문법 (메인테이너 합의) — WP-0을 하지 않으므로 자리표시 값(`058b:0000` TAS, `058b:0001` MCD)으로 남는다
- [x] 0x8000\_0000 캐시 별칭을 `is_alias`로 둘지, 플래셔에서 주소를 바꿀지 — 0x8000\_0000이 주 영역, 0xA000\_0000이 `is_alias`. 플래셔가 0xA000\_0000으로 바꿔 쓴다 (0.4절)
- [x] probe-rs가 쓰는 capstone 버전의 TriCore 지원 여부 — `arch_tricore` 기능(TriCore 1.6.1)으로 DAP 디스어셈블리 동작 (FR-U3)
- [ ] 성능 목표치 (step 지연, 메모리 처리량): 측정하지 않음. 참고: MCD 경유로 앱 섹터 12개와 PF1 시험 섹터를 ECC 인식 방식으로 읽어 비교하는 데 77초
- [x] MCD FFI 타입을 SPRINT 헤더 없이 직접 정의해도 되는지 라이선스 확인 — 헤더는 넣지 않고 `ffi.rs`에 원문 저작권 고지와 변경 고지를 넣었다. 최종 확인은 담당자 몫
- [ ] MCDS 검증용 ED 보드와 맵 1 허용 어댑터 확보 — 보드는 TC27x ED(EMEM 읽힘)로 확인. 맵 1을 여는 어댑터가 남았다
- [x] TC37x를 2차에 넣을지 — WP-9로 남기고 TC37x 보드가 생기면 진행

## 12. 부록

### A. TC27x 레지스터 요약

코어 n의 CSFR 창 = 0xF8810000 + n × 0x20000. 아래 오프셋은 그 창 기준이다 (원본: `docs/TC27X_DEBUG_REGISTERS.md`, `cpudefs-mdep.c`, `M25_4B_FLASHER.md` 12절).

| 레지스터 | 주소 / 오프셋 | 쓰임 |
| --- | --- | --- |
| TRnEVT / TRnADR | +0xF000 + 8n / +0xF004 + 8n | HW 브레이크포인트 (EVT = 0x101A) |
| DBGSR | +0xFD00 | halt 0x6, run 0x4, HALT 비트, EVTSRC |
| PCXI / PSW / PC | +0xFE00 / +0xFE04 / +0xFE08 | 문맥·상태·프로그램 카운터 |
| D0–D15 | +0xFF00 … +0xFF3C | 데이터 레지스터 |
| A0–A15 | +0xFF80 … +0xFFBC | 주소 레지스터 (A10 = SP, A11 = RA) |
| CBS\_JTAGID | 0xF0000464 | 칩 식별 (0x401DA083) |
| CBS 디버그 제어 | 0xF0000494 | step 시퀀스 (0x8000003E / 0x8000003F) |
| FLASH0\_FSR | 0xF8002010 | 플래시 busy 비트 0–4, OPER/SQER/PROER 11–13, PFDBER |
| CPU0 WDT CON0 / CON1 | 0xF0036100 / 0xF0036104 | ENDINIT, 워치독 비활성화 |
| Safety WDT CON0 / CON1 | 0xF00360F0 / 0xF00360F4 | ENDINIT, 워치독 비활성화 |
| MCDS 타임스탬프 | 맵 15, 0x50000090 | TAS 서버 내부 |

CSA upper context(16워드): w0 PCXI, w1 PSW, w2 A10, w3 A11, w4–7 D8–D11, w8–11 A12–A15, w12–15 D12–D15.

### B. 용어

| 용어 | 뜻 |
| --- | --- |
| TAS | Infineon Tool Access Socket. DAS 어댑터를 TCP 24817로 여는 서버와 그 프로토콜 (PL2 / PL1 / PL0) |
| MCD | Multi-Core Debug API (SPRINT v1.1). 디버거가 DLL로 프로브를 부르는 C API |
| MCDS | Multi-Core Debug Solution. ED 칩의 온칩 트레이스 블록 |
| OCDS | On-Chip Debug Support. TriCore의 디버그 레지스터 묶음 |
| CSA | Context Save Area. CALL마다 upper context를 저장하는 16워드 블록, PCXI로 연결 |
| ED | Emulation Device. MCDS와 트레이스 메모리(EMEM)를 가진 칩 변형 |
| ENDINIT | 보호 레지스터 쓰기를 막는 워치독 비트. 플래시 명령 전에 내리고 끝나면 올린다 |

### C. 출처

- [probe-rs/probe-rs](https://github.com/probe-rs/probe-rs) master `f4d1902` (2026-10-22): `core.rs` `CoreInterface`, `session.rs` `ArchitectureInterface`, `flashing/host_sequence.rs` `DebugFlashSequence`, `probe/xvc/mod.rs`, `plugin.rs`, `probe-rs-espressif`
- [probe-rs 이슈 #2796 — Infineon Tricore architecture support](https://github.com/probe-rs/probe-rs/issues/2796)
- [Infineon/tas\_client\_api](https://github.com/Infineon/tas_client_api) (Apache-2.0)
- [najari/aurix\_debugger](https://github.com/najari/aurix_debugger): `mcd_tas`, `mcdsrl`, `mcdsdl`, `m25_tcf`, `docs/TC27X_DEBUG_REGISTERS.md`
- [probe-rs × MCD / MCDS 연동 기술 검토](PROBE_RS_TRICORE_REVIEW.md)
