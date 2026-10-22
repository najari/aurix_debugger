# probe-rs × MCD / MCDS 연동 기술 검토

2026-10-23 · 원본(다이어그램 포함): [Claude 문서](https://claude.ai/artifact/TCcayzJFpQsvrX3kYDyAAE)

## 요약

probe-rs에 TriCore 아키텍처를 새로 추가하고, 첫 프로브로 TAS(TCP) 드라이버를 붙이는 방식이 가장 현실적이다. MCD는 같은 백엔드를 쓰는 선택 플러그인으로, MCDS는 녹화·저장만 나중 단계에 붙인다.

- **가능한 이유**: TriCore 디버그는 전부 메모리 맵 레지스터라 프로브는 메모리 읽기·쓰기와 리셋만 제공하면 된다. TAS 프로토콜은 Apache-2.0 공식 클라이언트로 공개돼 있고, 저장소의 mcd\_tas·m25\_tcf가 TC275 실기에서 run/stop/step·브레이크포인트·리셋·CSA 언와인드·플래시를 모두 검증했다.
- **만들 것**: probe-rs 7개 계층 모두에 새 코드가 필요하다. 닫힌 아키텍처 enum, `ArchitectureInterface` 새 변형, `CoreInterface`, CSA 기반 `ExceptionInterface`, M25 로더를 쓰는 `DebugFlashSequence`, TAS 프로브 드라이버다.
- **핵심 위험**: 업스트림 수용이다. 본체 변경이 필요하고 비공개 TasServer(Windows 전용)에 기대며, TriCore 요청 [issue #2796](https://github.com/probe-rs/probe-rs/issues/2796)은 2024-09부터 열려 있다. MCDS는 이 어댑터에서 MCDS 레지스터(주소 맵 1) 접근이 막혀 검증할 수 없다.
- **첫 단계**: issue #2796에 이 설계를 공유하고, 포크에서 TAS 프로브와 `probe-rs read` 검증부터 시작한다.

용어: MCD = Multi-Core Debug API(SPRINT 표준 C API), MCDS = Multi-Core Debug Solution(Infineon ED 칩의 온칩 트레이스), TAS = Tool Access Socket(Infineon 디버그 접근 서버 프로토콜), OCDS = On-Chip Debug Support.

## 배경: 두 스택의 구조

### 현재 AURIX 스택

저장소의 세 구성요소(m25\_tcf, mcd\_tas, mcdsrl)는 모두 자체 TAS 클라이언트로 Infineon TasServer에 붙고, 서버가 DAS 어댑터를 통해 TC275에 접근한다. probe-rs와 맞닿는 지점은 이 TAS 클라이언트 층이다.

> 다이어그램: 현재 AURIX 디버그 스택 · 도구 3종 → TAS → DAS → TC275 — [원본 문서](https://claude.ai/artifact/TCcayzJFpQsvrX3kYDyAAE)에서 볼 수 있다.

- TriCore 디버그는 전부 메모리 맵 레지스터로 이루어진다. 정지·재개는 DBGSR(CSFR+0xFD00) 쓰기, 레지스터는 CSFR+0xFF00(D0–D15)·+0xFF80(A0–A15), 브레이크포인트는 트리거 유닛 TR0–7이다. 그래서 프로브에 필요한 것은 “주소 맵 0 메모리 읽기·쓰기 + 리셋”뿐이다.
- 리셋은 TAS DEVICE\_CONNECT(옵션 0x0003 RESET\_AND\_HALT), 싱글 스텝은 CBS 레지스터 0xF0000494 브래킷 + DBGSR←4 시퀀스다(원본 도구 트래픽 캡처로 확보).
- 공식 TAS 클라이언트 소스([Infineon/tas\_client\_api](https://github.com/Infineon/tas_client_api), Apache-2.0)가 있고, 자체 클라이언트는 그 프로토콜을 캡처로 교차 검증했다. TasServer 자체는 비공개 Windows 바이너리다.
- MCDS 디코드 엔진(원본 디컴파일 174,670줄)은 재구현하지 않았고, 이 어댑터에서는 MCDS 레지스터(맵 1) 접근이 서버에서 거부된다(PL0 0x8D). 원본 recorder도 같은 지점에서 실패한다.

### probe-rs 구조

probe-rs는 프로브 드라이버와 벤더 시퀀스는 외부 플러그인으로 넣을 수 있지만, 아키텍처는 닫힌 enum이라 본체 수정이 필요하다. 아래는 master `f4d1902`(2026-10-22, 0.32.0) 소스를 직접 읽은 결과다.

> 다이어그램: probe-rs 계층별 TriCore 추가 지점 · 7개 계층 — [원본 문서](https://claude.ai/artifact/TCcayzJFpQsvrX3kYDyAAE)에서 볼 수 있다.

- **닫힌 아키텍처 enum**: `Architecture`는 Arm·Riscv·Xtensa, `CoreType`은 Armv6m…Riscv64·Xtensa까지다. `DebugProbe`에는 `try_get_arm_debug_interface`·`try_get_riscv_interface_builder`·`try_get_xtensa_interface`처럼 아키텍처별 메서드만 있고 범용 훅은 없다. Xtensa 추가 때 손댄 파일은 17개다.
- **세션 모델**: `ArchitectureInterface`는 `Arm`·`ArmWithRiscv`·`Jtag`(TAP별 RISC-V/Xtensa) 세 가지다. TAS처럼 “메모리와 리셋만 주는” 프로브를 받을 변형이 없다.
- **플래시**: `Flasher::call_function`은 ARM식 호출(인자 레지스터 4개, r9 static base, 복귀 주소)을 쓴다. TriCore EABI(인자 D4–D7·A4–A7, SP A10, 복귀 A11)와 맞지 않는다. 대신 새로 생긴 `DebugFlashSequence`(`HostSideFlasher`)는 아키텍처와 무관하게 호스트에서 지우기·쓰기·검증을 수행한다.
- **RTT는 아키텍처 무관**이라 `MemoryInterface`만 있으면 동작한다. 트레이스는 ARM 전용(`SwoAccess`, `TraceSink`)이고 범용 추상화가 없다.
- **플러그인·원격**: `plugin::register_plugin`으로 프로브 드라이버·벤더·타깃을 외부 크레이트에서 등록한다(`probe-rs-linux`가 예). TCP 프로브 선례로 XVC와 CMSIS-DAP over TCP가 있고, 주소는 셀렉터에 담는다(`0000:0000:<host>:<port>`). 벤더 동적 라이브러리(libloading) 사용 전례는 없다.
- **기존 논의**: [issue #2796 “Infineon Tricore architecture support”](https://github.com/probe-rs/probe-rs/issues/2796)가 2024-09-03부터 열려 있다. 댓글 하나가 JTAG·DAP 세부가 NDA 대상이라며 실현 가능성을 의심하고 [veecle/tricore-probe](https://github.com/veecle/tricore-probe)(DAS + AURIX Flasher 기반, probe-rs 미사용)를 소개했다. TAS 프로브 방식은 이 NDA 문제를 피한다: 저수준 프로토콜은 TasServer가 처리하고, probe-rs는 공개된 TAS 프로토콜과 공개 매뉴얼의 OCDS 레지스터만 쓴다.
- **라이선스·툴체인**: MIT OR Apache-2.0, edition 2024, MSRV 1.95. CONTRIBUTING은 테스트·문서·changelog 조각을 요구하고, LLM으로 만든 PR은 한 가지 변경만 담으라고 한다.

## 연동 방식 비교

권장안은 probe-rs 안에 TriCore 아키텍처 백엔드를 새로 만들고, 그 아래 버스 접근만 맡는 얇은 트레이트를 두어 TAS 프로브부터 붙이는 방식이다. TriCore 디버그는 전부 메모리 맵 레지스터라서, 프로브는 읽기·쓰기·리셋만 제공하면 되고 코어 로직은 한 벌이면 된다.

> 다이어그램: 연동 구조 · 새 코드 3층(강조) + 프로브 3종 — [원본 문서](https://claude.ai/artifact/TCcayzJFpQsvrX3kYDyAAE)에서 볼 수 있다.

MCD를 쓸 때도 MCD의 run/step/트리거 함수를 쓰지 않고 `mcd_execute_txlist_f`(메모리)와 `mcd_rst_f`(리셋)만 전송 계층으로 쓰면, 세 프로브가 같은 TriCore 백엔드를 공유한다. MCD의 코어 단위 API를 그대로 코어 인터페이스에 연결하는 설계도 가능하지만, 그러면 프로브마다 코어 로직이 따로 생긴다.

| 방식 | 구성 | 외부 의존성 | 플랫폼 | 구현량 | 업스트림 수용 가능성 | 판단 |
| --- | --- | --- | --- | --- | --- | --- |
| A. TAS 프로브 + TriCore 백엔드 | Rust TAS 클라이언트(TCP) | TasServer(비공개, 별도 설치) | 클라이언트는 전 플랫폼, 서버는 Windows | 중 | 높음: 순수 Rust, 서버와는 TCP만 | 1단계 권장 |
| B. MCD 프로브 | MCD DLL 동적 로드 | mcdxdas.dll 등 벤더 DLL, SPRINT 헤더 | Windows(DLL) | 소 | 낮음: 본체에 libloading 선례가 없어 외부 플러그인 크레이트로 | 선택 기능(feature) |
| C. 네이티브 JTAG | 기존 FTDI 드라이버 + Cerberus JTAG | 없음 | 전 플랫폼 | 대: 프로토콜 일부 비공개 | 가장 높음 | 장기 과제 |
| D. 외부 브리지 | probe-rs ↔ m25 TCF 에이전트 | m25 에이전트 | Windows | 소 | 해당 없음 | 비권장: probe-rs의 플래시·RTT·언와인드를 못 씀 |

- B는 Infineon 하드웨어에서는 A와 같은 TasServer를 거치므로 이득이 적다. 가치는 타사 MCD 구현을 붙일 때다(제품별 TriCore MCD 지원 여부는 미확인).
- C는 TasServer·DAS 의존을 없애지만, Cerberus JTAG 명령 체계와 DAP(2핀) 프로토콜을 새로 확보해야 한다. A의 TriCore 백엔드를 그대로 올릴 수 있어 A가 C의 선행 단계가 된다.

## TriCore 코어 설계

`CoreInterface`의 모든 메서드는 CSFR 창(CPU0 `0xF881_0000`, 코어마다 +0x20000) 메모리 접근으로 구현된다. 리셋만 프로브(TAS DEVICE\_CONNECT)가 맡는다. 아래 시퀀스는 모두 mcd\_tas·m25\_tcf에서 TC275 실기로 검증한 것이다.

| probe-rs `CoreInterface` | TriCore(TC27x) 구현 | 저장소 근거 |
| --- | --- | --- |
| `halt` | DBGSR(CSFR+0xFD00) ← 0x6 (HALT=11b, DE=1 필요), HALT=01b 재판독 | mcd\_tas M16 |
| `run` | DBGSR ← 0x4. 디버거 홀드(SUSP) 상태면 CBS 0xF0000494 브래킷 + 0x049C ← 2 | mcd\_tas M22b |
| `step` | 디버그 초기화 → 0494 ← 0x8000003E → DBGSR ← 4 → 0494 ← 0x8000003F → 초기화, 1명령 후 재정지 | mcd\_tas M22 |
| `status` · `core_halted` | DBGSR.HALT, 정지 원인 EVTSRC(16+n = TRn → 브레이크포인트) | TC27X\_DEBUG\_REGISTERS |
| `read_core_reg` · `write_core_reg` | D0–D15 CSFR+0xFF00, A0–A15 +0xFF80, PCXI +0xFE00, PSW +0xFE04, PC +0xFE08 (쓰기는 정지 중만) | m25 cpudefs-mdep.c |
| `set_hw_breakpoint` | TRnEVT(F000+8n) = 0x101A(PC 트리거→Halt), TRnADR = 주소. 8유닛, 범위는 짝 유닛 2개 | mcd\_tas M18 |
| 소프트웨어 브레이크 | `debug` 명령(0x00A0) — RAM에서만 | m25 BREAK\_INST |
| `reset_and_halt` | TAS DEVICE\_CONNECT 옵션 0x0003. 0x0001은 DE=0을 남겨 이후 halt가 무시됨 | mcd\_tas M17b |
| `stack_pointer` · `return_address` · `frame_pointer` | A10 · A11 · A14(GCC) | m25 |
| `spill_registers` | 할 일 없음: CALL이 이미 upper context를 CSA에 저장 | — |
| `MemoryInterface` | 코어 로컬 SPR(0xC/0xD000\_0000)을 전역 별칭(0x7000\_0000 − core×0x1000\_0000)으로 변환 | mcd\_tas M20 |

### 호출 스택: CSA 체인

> 다이어그램: TriCore CSA 체인 · 언와인드 경로 — [원본 문서](https://claude.ai/artifact/TCcayzJFpQsvrX3kYDyAAE)에서 볼 수 있다.

probe-rs에서는 `ExceptionInterface`를 구현해 `calling_frame_registers`가 호출자 레지스터를 CSA에서 채우고, `unwind_without_debuginfo`가 디버그 정보 없이 체인을 따라가게 한다. 인터럽트 진입으로 생긴 lower context CSA(PCXI.UL=0)는 레지스터 배치가 달라 따로 처리해야 한다.

### m25에서 만난 함정 (probe-rs에서도 그대로 나온다)

- **스텝 판정**: GCC의 CFA는 함수 진입 시 SP라서, 스택을 쓰지 않는 함수는 호출자와 CFA가 같다. m25는 프레임 식별자를 CSA 주소로 바꿔 Step Over/Out을 고쳤다. probe-rs의 스텝이 프레임을 어떻게 비교하는지는 확인이 필요하다.
- **스택 끝**: PCXI = 0이면 CSA를 읽지 않고, 복귀 주소(A11) 0에서 멈춘다. 안 그러면 0x0\~0x3C 읽기 실패가 정지마다 16번 났다.
- **브레이크 오귀속**: 부트 소프트웨어가 리셋 후 정지용으로 TR0을 쓴다(0x1022 @ 0xAFFFD418). 정지 PC가 BP 주소일 때만 BP 적중으로 보고하고, 리셋 뒤에는 모든 코어의 트리거를 다시 기록해야 한다.
- **리셋 후 코어 상태**: 리셋 뒤 펌웨어가 CPU1/2를 시작하므로, 정지로 알고 있던 코어도 DBGSR를 다시 읽어야 한다.
- **워치독**: 코어를 오래 멈추거나 다른 코드를 돌리면 앱의 WDT가 리셋을 건다. 플래시 알고리즘은 ENDINIT/비밀번호 처리가 필요하다(M25 로더에서 검증).

## MCDS 트레이스

probe-rs의 트레이스 모델은 ARM 전용이라 MCDS는 별도 경로로 붙여야 하고, 1단계 범위에서는 뺀다. 붙일 때는 녹화와 `.mcds` 저장까지만 probe-rs가 맡고, 디코드는 Infineon 도구에 맡긴다.

> 다이어그램: MCDS 데이터 경로 · probe-rs 구간 5단계 + 외부 디코드 — [원본 문서](https://claude.ai/artifact/TCcayzJFpQsvrX3kYDyAAE)에서 볼 수 있다.

- **녹화 로직**: mcdsrl\_tas(22 export, 원본 recorder 3.3.0과 프레임 대조 완료)를 Rust로 옮긴다. 그래서 버스 접근 트레이트는 주소 맵 번호(0 타깃, 1 EEC, 15 서버 내부)를 인자로 받아야 한다.
- **타임스탬프**: MCDS 64비트 카운터(맵 15 `0x5000_0090`, RD64)는 맵 0 접근과 같은 PL0 프레임에 섞으면 서버가 0x8D로 거부한다. 프레임을 나눠야 한다.
- **하드웨어 블로커**: DAS JTAG AURDUINO에서는 MCDS 레지스터(맵 1, `0xF00E_6xxx`/`8xxx`) 읽기·쓰기가 모두 0x8D로 실패하고, 원본 recorder도 같은 지점에서 실패한다. 다른 어댑터에서 열리는지 확인하기 전에는 이 경로를 실기로 검증할 수 없다.
- **디코드 엔진**: 원본은 함수 2,409개·174,670줄(디컴파일 기준)이고 TriCore 디스어셈블을 포함한다. `.mcds`가 표준 포맷이라 원본 decoder·MCDS Trace Viewer로 분석할 수 있으므로 재구현하지 않는다.
- **probe-rs 접점**: `probe-rs-tools`의 새 하위 명령, 또는 외부 플러그인 크레이트다. `Session::setup_tracing`·`TraceSink`는 SWO·TPIU 개념이라 확장 대상이 아니다.
- **대안**: printf류 출력은 RTT로 충분하다. RTT는 아키텍처 무관이라 TriCore 백엔드만 있으면 동작한다.

## 플래시

AURIX PFLASH는 `DebugFlashSequence`(호스트 플래시 시퀀스)로 굽는 것을 권장한다. TC27x 실기에서 검증한 M25 로더를 그대로 쓰고, ARM식으로 고정된 `Flasher::call_function`을 고칠 필요가 없다.

| 경로 | 타깃 코드 | probe-rs 본체 변경 | 속도 | 위험 |
| --- | --- | --- | --- | --- |
| DebugFlashSequence + M25 로더 (권장) | 946 B 메일박스 로더, 섹터·페이지 단위 명령 | 없음(벤더 시퀀스) | 중: 명령마다 메일박스 왕복 | 낮음: TC27x에서 검증, WDT·ENDINIT 처리 포함 |
| RAM 알고리즘 (`Flasher`) | CMSIS식 ELF(Init·EraseSector·ProgramPage) | `call_function`에 TriCore EABI 분기(D4–D7, A10, A11), 복귀 시 `debug` 명령 정지 | 가장 빠름 | 중: 호출마다 유효한 CSA 목록 필요 |
| 호스트 직접 명령 | 없음: 0xAF00\_xxxx 명령 시퀀스를 TAS로 직접 쓴다 | 없음 | 느림(미측정): 32 B 페이지마다 왕복 | 중: 정지 중 WDT 리셋 가능성 |

> 다이어그램: PFLASH 굽기 시퀀스 · DebugFlashSequence + M25 로더 — [원본 문서](https://claude.ai/artifact/TCcayzJFpQsvrX3kYDyAAE)에서 볼 수 있다.

- **레이아웃**: `HostSideFlasher`도 레이아웃용 `RawFlashAlgorithm` 항목이 필요하다. PF0/PF1 논리 섹터 54개(16K×8, 32K×8, 64K×4, 128K×3, 256K×4 × 2뱅크), 페이지 32 B, 캐시 주소 0x8000\_0000 + 비캐시 별칭 0xA000\_0000(`is_alias`)으로 기술한다.
- **지워진 페이지**: TC27x PFLASH의 지워진 페이지는 0이 아니라 ECC 읽기 오류를 낸다. probe-rs의 blank check·전체 검증은 쓴 페이지만 읽도록 `verify`를 시퀀스에서 구현해야 한다(M25의 페이지 단위 읽기와 같다).
- **보호 영역**: UCB·DFLASH는 타깃 YAML의 NVM 영역에서 빼고 시퀀스가 범위 밖 주소를 거부한다. 잘못된 UCB 쓰기는 칩을 잠글 수 있다.
- **굽기 전 백업**: M25처럼 섹터 백업→지우기→쓰기→검증 순서를 유지하면 `flashrestore`로 되돌릴 수 있다. probe-rs 표준 흐름에는 백업 단계가 없으므로 옵션으로 추가한다.
- **대안**: Windows에서는 확장에 든 Infineon AURIX Flasher도 장치 항목(JTAG ID 0x401DA083)을 추가하면 이 보드를 굽는다(M25\_4B\_FLASHER.md 14절). `DebugFlashSequence`가 외부 도구에 위임하는 것도 허용되지만, 비공개 도구 의존이 늘어난다.

## VS Code / DAP 연동

VS Code 쪽은 probe-rs 공식 확장과 DAP 서버를 그대로 쓰고, 바뀌는 것은 launch.json의 프로브 선택과 칩 이름뿐이다. 지금의 AURIX TCF 확장 + m25 에이전트 + preLaunchTask 굽기 조합을 하나로 대체한다.

> 다이어그램: VS Code 디버그 세션 시퀀스 · launch부터 스택 조회까지 — [원본 문서](https://claude.ai/artifact/TCcayzJFpQsvrX3kYDyAAE)에서 볼 수 있다.

제안하는 launch.json(`probe` 값의 형식은 제안이다. XVC·CMSIS-DAP over TCP처럼 가상 VID:PID 뒤에 주소를 담고, 시리얼 자리에 TAS 타깃 이름을 넣는다):

```json
{
  "type": "probe-rs-debug",
  "request": "launch",
  "name": "TC275 (probe-rs, TAS)",
  "chip": "TC27x",
  "probe": "<TAS VID:PID>:127.0.0.1:24817:DAS JTAG AURDUINO 46YPEADW",
  "flashingConfig": { "flashingEnabled": true },
  "coreConfigs": [
    { "coreIndex": 0, "programBinary": "build/demo_flash.elf", "rttEnabled": true }
  ]
}
```

- **멀티코어**: `coreConfigs`에 core0–2를 각각 넣는다. 코어 3개를 동시에 디버그할 때의 DAP 서버 동작은 확인이 필요하다.
- **디스어셈블리**: DAP 서버는 capstone을 쓰고 Xtensa는 아직 미구현이다. capstone의 TriCore 모드가 TC1.6E/P를 얼마나 풀어내는지 확인하고, 부족하면 m25 디스어셈블러를 옮긴다.
- **현재 대비 이득**: 굽기가 세션 안으로 들어와 preLaunchTask와 `flash_start.js` 같은 보조 단계가 없어지고, RTT·GDB 서버·`probe-rs run`을 같은 백엔드로 쓸 수 있다.
- **원격**: probe-rs 쪽은 TCP 클라이언트라 Linux·macOS에서 Windows의 TasServer에 붙는 구성도 구조상 가능하다. TasServer가 원격 접속을 허용하는지는 확인하지 않았다.

## 위험과 제약

가장 큰 위험은 기술이 아니라 업스트림 수용이다. 아키텍처 추가는 probe-rs 본체 변경이고, 비공개 TasServer에 기대는 프로브에 대한 메인테이너 입장은 확인되지 않았다.

| 위험 | 영향 | 대응 |
| --- | --- | --- |
| 업스트림 수용: `Architecture`·`CoreType`·`ArchitectureInterface`·`DebugProbe` 변경이 필요하고, 비공개 서버 의존 프로브의 선례가 없다 | 포크 유지 비용이 계속 생긴다 | issue #2796에 설계를 먼저 공유하고, 한 가지씩 담은 작은 PR로 나눈다 |
| TasServer 의존: 비공개 Windows 바이너리, DAS 설치 필요 | 하드웨어 없는 CI에서 검증할 수 없다 | tas-sw 캡처(3,662연결)를 재생하는 모의 TAS 서버로 단위 테스트, 실기는 smoke-tester |
| 프로토콜 근거: DAP·JTAG 저수준은 NDA(issue #2796 댓글). 스텝의 CBS 0xF0000494 시퀀스는 도구 트래픽 캡처에서 얻었다 | 기여 코드의 출처 문제가 제기될 수 있다 | TAS는 Apache-2.0 공식 클라이언트, OCDS는 공개 UM에 근거를 두고, 캡처 기반 부분은 UM 근거를 찾거나 메인테이너와 미리 논의한다 |
| MCD 헤더 라이선스(SPRINT)와 벤더 DLL 동적 로드 | 본체 반입이 어렵다 | MCD 프로브는 외부 플러그인 크레이트로 둔다 |
| 성능: PL0 프레임당 트랜잭션 32개·블록 1 KB, TCP 왕복 | 스텝·변수 조회가 느려진다 | `execute_memory_operations`로 묶고, 레지스터는 CSFR 0xFF00–0xFFBC를 블록으로 읽는다 |
| 안전: UCB 쓰기, WDT 리셋, 앱 RAM 상태 손상 | 칩 잠김이나 보드 오동작 | UCB를 NVM 영역에서 제외하고, 섹터 백업을 기본으로 둔다. 로더는 RAM 창을 저장·복원한다 |
| 칩 범위: 실기 검증은 TC275(C-step ED)뿐이고, TC37x 로더는 재검증하지 않았다 | TC3xx에서 주소·코어 수가 달라 바로 동작하지 않을 수 있다 | 칩 패밀리별 YAML과 시퀀스로 분리하고 TC27x부터 낸다 |
| MCDS 블로커: 이 어댑터에서 맵 1 접근 불가 | MCDS 단계를 검증할 수 없다 | 다른 어댑터로 맵 1 접근을 먼저 확인한다 |
| probe-rs 변화 속도: JTAG 계층 개편·플러그인 분리가 master에만 있고 미릴리스 | 작업 중 API가 바뀐다 | master 기준으로 작업하고 본체 접점을 최소화한다 |

## 로드맵과 검증 계획

1단계(TAS 프로브)부터 4단계(플래시)까지가 현재 m25 스택을 대체하는 최소 범위다. 각 관문은 이미 m25·mcd\_tas에서 실기로 통과한 시나리오를 probe-rs로 다시 돌리는 것이다.

> 다이어그램: 단계별 로드맵 · 6단계, 단계마다 관문 1개 — [원본 문서](https://claude.ai/artifact/TCcayzJFpQsvrX3kYDyAAE)에서 볼 수 있다.

- **테스트 기반**: tas-sw의 TAS 캡처(`pkt_capture/`, `mcds/tas_capture/`)를 재생하는 모의 서버로 프로토콜 단위 테스트를 만들고, 실기 회귀는 probe-rs의 smoke-tester에 TC275 항목으로 추가한다.
- **비교 기준**: 각 단계의 결과는 같은 보드에서 mcd\_tas·m25 결과와 대조한다(예: PC·DBGSR 판독값, BP 정지 주소, CSA 체인 프레임 수, 섹터 백업 일치).
- **PR 단위**: CONTRIBUTING의 요구(테스트·문서·changelog 조각, LLM PR은 한 가지 변경)에 맞춰 단계마다 여러 PR로 나눈다. 예: enum 추가 → 버스 트레이트 → TAS 드라이버 → 코어.

## 출처

- [probe-rs/probe-rs](https://github.com/probe-rs/probe-rs) master `f4d1902`(2026-10-22) 소스: `probe-rs/src/probe.rs`, `session.rs`, `core.rs`, `flashing/flasher.rs`, `flashing/host_sequence.rs`, `probe-rs-target/src/chip_family.rs`, `probe-rs-debug/src/exception_handling.rs`
- [probe-rs issue #2796 — Infineon Tricore architecture support](https://github.com/probe-rs/probe-rs/issues/2796)
- [veecle/tricore-probe](https://github.com/veecle/tricore-probe)
- [Infineon/tas\_client\_api](https://github.com/Infineon/tas_client_api) (Apache-2.0)
- [TC27x D-Step User Manual V2.2](https://www.infineon.com/assets/row/public/documents/10/44/infineon-tc27x-d-step-usermanual-en.pdf) · [TriCore TC1.6P/TC1.6E Core Architecture Vol.1](https://www.infineon.com/assets/row/public/documents/10/44/infineon-tc2xx-architecture-vol1-usermanual-en.pdf)
- [najari/aurix\_debugger](https://github.com/najari/aurix_debugger): `mcd_tas/README.md`, `mcdsrl/README.md`, `mcdsdl/README.md`, `m25_tcf/README.md`, `m25_tcf/M25_4B_FLASHER.md`, `docs/TC27X_DEBUG_REGISTERS.md`
