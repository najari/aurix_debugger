# AURIX 디버거 전체 코드 리뷰

2026-10-26 · 김동규

probe-rs 포크(tricore 브랜치)와 aurix_debugger(main)를 네 영역으로 나눠 검토했고, 실제 사용 중에 생길 수 있는 결함 7건을 포함해 21건을 찾았습니다. 가장 먼저 고칠 것은 스텝할 때 인터럽트가 꺼진 채 남는 문제, allStop과 suspendPeripherals를 함께 쓸 때 STM이 멈추지 않는 문제, 한글 조건식에서 dap-server가 패닉하는 문제입니다.

## 결함 목록

높음 7건, 중간 8건, 낮음 7건입니다. 상태 열은 고치면서 바꿔 주세요. 경로의 `tricore/`는 probe-rs의 `probe-rs/src/architecture/tricore/`, `dap_server/`는 `probe-rs-tools/src/bin/probe-rs/cmd/dap_server/`입니다.

| # | 심각도 | 영역 | 결함 | 위치 | 상태 |
| --- | --- | --- | --- | --- | --- |
| 1 | 높음 | TriCore | 스텝할 때 인터럽트를 끄는 기능이 MFCR ICR, SYSCALL, 트랩을 놓쳐 인터럽트가 꺼진 채로 남음 | `tricore/mod.rs:240-266`, `1841-1862` | 수정됨 |
| 2 | 높음 | DAP·TriCore | allStop과 suspendPeripherals를 함께 쓰면 정지 그룹 검사가 HALT 필드를 지워 STM이 멈추지 않음 | `tricore/mod.rs:1616-1628`, `2162-2166` | 수정됨 |
| 3 | 높음 | 플래시 | 로더의 `wdt_disable`이 CPU0·세이프티 와치독의 `CON1.DR`를 되돌리지 않음 | `loader/m25_flash_loader.c:175-187` | 수정됨 |
| 4 | 높음 | 플래시 | `stop_loader`, `prepare_flash` 오류 경로에서 RAM·레지스터·코어 복원이 빠짐 | probe-rs-infineon `flash.rs` | 수정됨 |
| 5 | 높음 | 플래시 | `--restore-unwritten`이 0만 있는 페이지를 건너뛰고, 읽기 오류를 지워진 페이지로 봄 | probe-rs-infineon `flash.rs` (`read_pages`) | 수정됨 |
| 6 | 높음 | GUI | 요청이 스레드풀 태스크마다 따로 나가 순서가 바뀜 (브레이크포인트 유실, 캐시 측정 왜곡) | `app/src/debugger.rs:386-416` | 수정됨 |
| 7 | 높음 | DAP·스크립트 | 조건식·`expect`에 ASCII가 아닌 문자가 있으면 패닉 | `dap_server/server/condition.rs:68-77`, `crates/script/src/compare.rs:97` | 수정됨 |
| 8 | 중간 | TriCore | 정지 그룹 안에서 프로그램의 DEBUG 명령에 멈추면 계속 실행·스텝이 안 됨 | `tricore/mod.rs:1710-1712`, `1913`, `1944` | 수정됨 |
| 9 | 중간 | TriCore | `on_device_reset`이 `SWEVT`를 다시 쓰지 않아 소프트웨어 브레이크포인트가 안 걸림 | `tricore/mod.rs:1438-1471` | 수정됨 |
| 10 | 중간 | TriCore | 스텝 중 오류가 나면 스텝용 트리거 창이 장치에 남음 | `tricore/mod.rs:1866-1933` | 수정됨 |
| 11 | 중간 | DAP | 비정상 종료 때 `clean_up`이 실행되지 않아 suspend 레지스터가 복원되지 않음 | `dap_server/server/peripheral_suspend.rs`, `session_data.rs` | 수정됨 |
| 12 | 중간 | GUI | 서버 연결이 끊겨도 알아채지 못함 | `crates/dap-client/src/lib.rs:168`, `app/src/debugger.rs:441-475` | 수정됨 |
| 13 | 중간 | GUI | GTM 탭에서 응답 전에 MCS를 바꾸면 언더플로 패닉(debug) 또는 잘못된 데이터(release) | `crates/debug-model/src/gtm.rs:536` | 수정됨 |
| 14 | 중간 | 플래시 | 섹터 맵 불일치는 경고만 하고, TC27x BMHD0 섹터 지우기를 막지 않음 | probe-rs-infineon 플래시 | 수정됨 |
| 15 | 낮음 | DAP | C 식 파서에 재귀 깊이 제한이 없음 (스택 오버플로) | `dap_server` C 식 파서 | 수정됨 |
| 16 | 낮음 | DAP | 타입 있는 포인터 산술에 원소 크기를 곱하지 않음 | `dap_server` C 식 파서 | 수정됨 |
| 17 | 낮음 | DAP | 정렬 안 된 레지스터에 SVD 필드를 쓰면 옆 레지스터를 건드림 | `svd.rs:219-241` | 수정됨 |
| 18 | 낮음 | DAP | `readRanges`의 count 합이 넘칠 수 있음 | `adapter.rs:280` | 수정됨 |
| 19 | 낮음 | GUI | 메모리 탭에 `u64` 끝 근처 주소를 넣으면 패닉 | `crates/debug-model/src/memory.rs:39`, `95`, `194` | 수정됨 |
| 20 | 낮음 | GUI·스크립트 | sw_trace 오류 메시지를 만들다 ASCII가 아닌 문자에서 패닉 | `crates/debug-model/src/sw_trace.rs:35` | 수정됨 |
| 21 | 낮음 | TriCore | 저널 복구가 성능 카운터의 `CCTRL`을 되돌리지 않음 | `tricore/mod.rs:1217-1228` | 수정됨 |
| 22 | 중간 | 플래시 | `--restore-unwritten`이 TC3xx에서 지워진 페이지가 있는 섹터면 항상 실패 (지우기 전에 멈춰 데이터는 안전) | probe-rs `flashing/host_flasher.rs` `fill_unwritten` | 수정됨 |

1~22번 모두 고쳤습니다. 8~10번, 13~17번, 19~21번은 단위 테스트로, 나머지는 TC36x 보드나 앱에서 확인했습니다.

- **1번:** 32비트 RFE 예외 오류도 함께 고쳤습니다. 코드가 op2 `0x06`을 RFE로 보았는데, 매뉴얼상 `0x06`은 RET이고 RFE는 `0x07`입니다.
- **3번:** 예전 로더로 플래시하면 `WDTS_CON1.DR`이 1로 남고(와치독 꺼짐), 새 로더는 0을 되돌려 놓습니다. 호스트도 끝난 뒤 `DR`을 확인해 다르면 오류를 냅니다. TC27x 로더는 빌드만 했고 보드에서는 시험하지 않았습니다.
- **22번(새 항목):** probe-rs의 `fill_unwritten`이 지워진 페이지를 읽다 ECC 오류(`0x8d`)를 만나 실패했습니다. probe-rs 플래시 시퀀스에 `read_unwritten`을 두고, AURIX는 페이지 단위로 읽어 지워진 페이지를 0(지워진 값)으로 돌려주도록 고쳤습니다. TC36x 데모 섹터에서 데모 코드, 0 페이지, 지워진 페이지가 모두 그대로 남는 것을 확인했습니다. 5번의 0 페이지 규칙은 이제 `--restore-unwritten`일 때만 적용됩니다(일반 다운로드는 나머지를 지움).
- **11번:** 요청 처리 중 오류로 끝나거나 세션 시작이 `apply` 뒤에 실패하면 suspend 레지스터를 되돌립니다. dap-server 프로세스가 강제 종료되면, 코어 저널에 남긴 값(TRCn과 STM OCS는 코어 n, TL1ST는 첫 코어)을 다음 세션이 되돌립니다. TRCn은 그룹 항목보다 먼저 복구되어 둘 다 돌아옵니다. 보드에서 서버를 죽여 확인했습니다.
- **14번:** BMHD 위치 중 BMHD0(`0xA0000000`)만 AP32566으로 확인했고, BMHD1~3은 일반적으로 알려진 TC27x 주소입니다. 주소가 틀려도 거부가 줄어들 뿐 잘못 거부하지는 않습니다.
- **HSM(추가 검토):** HSM 부팅이 켜진 TC3xx에서 PF0 S0~S39의 HSM 부트 코드를 지우면 장치가 영구히 잠길 수 있습니다(AP32555, AP32349). 이제 플래셔가 `DMU_SP_PROCONHSMCFG/CBS/CX0/CX1`과 `RPRO`를 읽어, HSM_exclusive 섹터와 읽기 보호된 PFLASH는 항상, HSM이 부팅하면 S0~S39는 `PROBE_RS_TRICORE_ALLOW_HSM_CODE_ERASE` 없이는 지우기 전에 거부합니다. 로더 오류는 비트 이름(PROER 등)으로, HSM 리셋(RSTSTAT bit 26/27)과 잠긴 디버그 인터페이스·SSWWAIT는 안내 문구로 나옵니다. 이 보드는 HSM 설정이 모두 0이라 단위 테스트로 확인했고, 보드에서는 정상 다운로드가 막히지 않는 것만 확인했습니다.

## 높음: 실제 사용 중 생기는 결함

7건 모두 코드를 다시 읽어 확인했습니다. 7번은 리뷰 에이전트가 재현까지 했습니다.

### 1. 스텝하면 인터럽트가 꺼진 채로 남을 수 있음

스텝 중 `ICR.IE`를 잠시 끄는 기능이 기본으로 켜져 있습니다(`mask_step_interrupts: true`). 이 기능은 `IE`를 쓰는 명령(ENABLE, DISABLE, RESTORE, BISR, RFE, RFM, MTCR ICR)만 예외로 둡니다. `IE`를 읽는 명령과 트랩에 들어가는 명령은 예외가 아닙니다.

- **MFCR:** iLLD `IfxCpu_disableInterrupts()`는 `__mfcr(CPU_ICR)`로 상태를 저장합니다. 이 명령을 스텝하면 `enabled = false`가 저장되고, 임계 구역이 끝나도 인터럽트가 다시 켜지지 않습니다.
- **SYSCALL과 트랩:** 트랩 진입 때 `PCXI.PIE = ICR.IE`(0)가 저장됩니다. 핸들러의 RFE가 돌아오면 `IE = 0`이 됩니다.
- `complete_watchpoint_access`도 같은 마스킹을 씁니다.

### 2. allStop과 suspendPeripherals를 함께 쓰면 STM이 멈추지 않음

최근 추가한 `suspendPeripherals`의 버그입니다. 정지 그룹 검사 `in_halt_group`은 `CBS_TRCn & 0x00F000FF`를 비교하는데, 이 마스크에 HALT 필드(bit 3:0)가 들어 있습니다.

1. `debugger.rs`가 먼저 정지 그룹에 넣어 `0x00700070`을 씁니다.
2. `peripheral_suspend::apply`가 HALT=1을 넣어 `0x00700071`을 씁니다.
3. 다음 상태 확인에서 `check_group`이 "그룹이 아님"으로 보고 `write_group_routing`으로 `0x00700070`을 다시 씁니다. 트리거 라인 1로 가는 경로가 지워집니다.

그룹에서 나갈 때와 `finish_reset`도 HALT를 그룹에 들어가기 전 값으로 되돌립니다. 지난번 2코어 보드 시험은 스크립트만 확인했고, STM이 실제로 멈추는지는 보지 않았습니다.

### 3. 플래시 로더가 와치독을 끈 채로 둠

`wdt_disable`이 CPU0 와치독과 세이프티 와치독의 `CON1.DR`(bit 3)를 켭니다. `stop_loader`는 `CON0`만 확인하고 `CON1`은 되돌리지 않습니다. 플래시 후 리셋 없이 계속 실행하면 와치독이 꺼진 채로 돕니다.

### 4. 로더 오류 경로에서 복원이 빠짐

- `stop_loader`는 저장해 둔 값을 `saved.take()`로 먼저 꺼낸 뒤 `?`로 오류를 돌려줍니다. 중간에 오류가 나면 RAM, 레지스터, 코어가 복원되지 않습니다.
- 로더가 시작된 뒤 `prepare_flash`가 실패해도 `finish_flash`를 건너뜁니다.

### 5. --restore-unwritten에서 데이터가 없어질 수 있음

- 값이 모두 0인 페이지를 "쓸 것 없음"으로 건너뜁니다. 그 페이지는 지워진 상태(ECC)로 돌아와 원래의 0이 없어집니다.
- `read_pages`는 읽기 오류가 나면 그 페이지를 지워진 것으로 보고, 다시 쓰지 않습니다.

### 6. GUI가 보낸 요청의 순서가 바뀔 수 있음

요청마다 GPUI 백그라운드 태스크가 따로 만들어지고, Windows에서는 OS 스레드풀(`TrySubmitThreadpoolCallback`)에서 쓰기 잠금을 두고 경쟁합니다.

- **브레이크포인트:** 두 개를 빨리 걸면 `setBreakpoints {A}`와 `{A,B}`가 나갑니다. `{A,B}`가 먼저 도착하면 타깃에는 A만 남는데, 화면에는 B도 걸린 것으로 보입니다.
- **캐시 측정:** `CCTRL` 쓰기와 구간 시작을 읽는 `ReadRanges`가 함께 나가서, 카운터를 새 설정보다 먼저 읽을 수 있습니다.

### 7. 조건식에 한글이 있으면 dap-server가 죽음

`condition.rs`의 `Comparison::parse`는 바이트 위치마다 `text[at..]`로 자릅니다. 멀티바이트 문자 중간에서 자르면 패닉하고 dap-server가 죽습니다. `aurix-run`의 `expect`(`compare.rs:97` `find_operator`)도 같습니다. 나중에 다시 해석되는 `expect ${name} == 1` 같은 줄에서 터지면 disconnect 없이 끝나, 서버에 세션이 남습니다.

## 중간·낮음

중간 항목은 특정 조건에서만 생기거나 사용자가 우회할 수 있고, 낮음 항목은 잘못된 입력이나 데이터에서만 나타납니다.

### 중간

- **8. DEBUG 명령에서 못 빠져나옴:** 정지 그룹 안에서는 자기 break-out 신호가 라인 7로 돌아와 `EVTSRC`를 0으로 덮습니다. `skip_program_debug`는 `EVTSRC == SWEVT`를 요구하므로 DEBUG 명령을 건너뛰지 못합니다. 실행은 `CoreDidNotRun`, 스텝은 PC가 그대로입니다. `PREVSUSP`로 구분할 수 있을 것으로 보입니다.
- **9. 다른 세션의 리셋 뒤 소프트웨어 브레이크포인트가 죽음:** `finish_reset`은 리셋이 `SWEVT`를 지운다고 보고 다시 씁니다. 하지만 `on_device_reset`은 브레이크포인트 자리의 코드가 바뀐 때만 씁니다. 트리거 유닛이 모자라 소프트웨어로 남은 브레이크포인트만 해당합니다.
- **10. 스텝 오류 뒤 트리거 창이 남음:** 실행 요청이나 `step_debug_init(false)`가 실패하면 트리거 유닛이 `STEP_TRIGGERS_ARM`/`INIT`에, `EXEVT`는 0에 머물러 있습니다. 다음 실행이 명령 하나만 돌고 멈추고, 하드웨어 브레이크포인트와 정지 그룹 break-in이 꺼집니다.
- **11. suspend 레지스터 복원 누락:** StdIO 오류, 재시작 실패, `apply` 뒤의 오류로 끝나면 `clean_up`이 실행되지 않습니다. 정지 그룹과 달리 suspend 레지스터는 저널이 없어 다음 세션이 복구하지 못합니다.
- **12. 연결 끊김을 모름:** 리더 스레드는 잘못된 메시지 하나에도 멈추지만 소켓은 닫지 않습니다. `DapClient`는 닫힌 상태를 알려 주지 않고 `try_event()`는 `Disconnected`를 숨깁니다. 상태는 계속 "Attached"이고, 메모리·live watch·OS 폴링 오류가 콘솔에 쌓입니다.
- **13. GTM 탭 MCS 전환 패닉:** `show_mcs`는 뷰를 초기화하지만, 이전 MCS에 보낸 `ReadRanges` 응답이 그대로 도착합니다. `asked[..].address - self.gtm.memory(self.mcs)`가 새 MCS로 계산되어 언더플로가 납니다(`[profile.dev]`는 오버플로 검사가 켜져 있음). 응답에 요청 세대(generation)를 붙이면 됩니다.
- **14. 플래시 보호 장치:** 장치 파일의 섹터 맵이 하드웨어와 달라도 경고만 나옵니다. TC27x의 BMHD0이 들어 있는 섹터를 지우는 것을 막지 않습니다.

### 낮음

- **15.** C 식 파서에 재귀 깊이 제한이 없어, 괄호가 아주 깊은 식에서 스택 오버플로가 납니다.
- **16.** 타입 있는 포인터에 정수를 더할 때 원소 크기를 곱하지 않습니다(`p + 1`이 1바이트 뒤).
- **17.** SVD 필드를 정렬되지 않은 레지스터에 쓰면 옆 레지스터까지 읽고 씁니다.
- **18.** `readRanges`의 count를 더하다 넘칠 수 있습니다.
- **19.** 메모리 탭에 `FFFFFFFFFFFFFF80` 같은 주소를 넣으면 `rows()`의 덧셈에서 패닉합니다. 타깃이 32비트이므로 0xFFFF_FFFF보다 큰 주소는 거부해야 합니다. `scroll`의 `saturating_add`는 시작 주소를 정렬에서 벗어나게 할 수 있습니다.
- **20.** `binary_bytes`의 오류 메시지가 `&rest[..8]`로 자르다 문자 중간에서 패닉합니다. 서버가 잘못된 데이터를 보낼 때만 해당합니다.
- **21.** 죽은 세션의 저널을 복구할 때 성능 카운터 트리거 유닛은 지우지만 `CCTRL`은 되돌리지 않아, 카운터가 task 모드에 남아 세지 않습니다.

## 수정 순서 제안

수정 범위가 작고 영향이 큰 1·2·7번부터 고칩니다.

1. **1·2·7번:** 스텝 인터럽트 마스킹, 정지 그룹 마스크, 문자 경계 처리. 2번은 TC36x 보드에서 CPU0을 세운 뒤 STM0이 멈추는지 직접 확인합니다.
2. **3~5번:** 플래시 로더의 와치독 복원, 오류 경로의 복원, `--restore-unwritten`의 데이터 보존.
3. **6·12·13번:** GUI 요청을 하나의 순서 큐로 보내고, 연결 끊김을 표시하고, GTM 응답에 세대를 붙입니다.
4. **8~11번, 14번:** TriCore·DAP·플래시의 나머지 중간 항목.
5. **15~21번:** 낮음 항목.

## 문제없음으로 확인된 부분

- **RAM 쓰기 검사(`RamMap::writable`):** `checked_add`를 쓰고, RAM 밖으로 넘어가는 쓰기를 거부합니다. 로컬 별칭(0xD/0xC)은 코어를 요구합니다. GUI와 스크립트 모두 보내기 전에 검사합니다.
- **DAP 클라이언트 대기 요청:** 등록과 `closed` 확인이 같은 잠금 아래에 있어, 응답이 올 수 없는 요청을 기다리지 않습니다.
- **파서:** SwTrace는 8바이트 레코드를 읽기 사이에 이어 붙이고 32비트 STM 넘침을 처리합니다. ORTI 파서는 오류 복구가 항상 앞으로 나가 무한 루프가 없습니다. 스크립트 블록, 중첩 `repeat`, include 깊이 제한도 맞습니다.
- **TriCore:** `prefetch_status`·`read_group`의 인덱스 계산, DEBUG 명령과 SYS 명령 인코딩, 트리거 이벤트 비트, 저널 복원 조건(심은 값과 같을 때만 되돌림), 브레이크포인트 슬롯 범위 검사.
- **테스트:** aurix_debugger `crates`의 `cargo test` 97개가 모두 통과합니다.
