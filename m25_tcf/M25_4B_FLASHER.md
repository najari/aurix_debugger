# M25-4b: 자체 플래셔 (PFLASH erase/program) — 1차 구현·검증 (2026-10-19)

> **2026-10-20 갱신 — TC27x 포팅·라이브 검증**: 아래 1~11절은 TC37x 기준 기록입니다. 연결된 보드는
> **TC275** (CBS_JTAGID `0x401DA083`, PFLASH 4MB)라 TC37x 로더를 그대로 쓰면 안 됩니다. 12절을 보세요.

## 1. 참조 구현 정찰

- 확장은 `aurixflasher.exe -gui off -elf <elf> [-hex ...] -start off -ver on -id <device>`로
  굽는다 (`extension.js:624` `runAurixFlasher`) — 별도 도구가 플래시를 담당.
- 이 머신의 AURIX Flasher Software Tool 3.1.0
  (`...\com.ifx.ads2.flash_2.0.24\AurixFlasherSoftwareTool\3.1.0.0\`):
  - `flax_TC37x.hex` = 플래시 알고리즘 (HEX: DSPR 0x70003800 스테이징 +
    PSPR 0x70100000 코드, 엔트리 0x70100008, start-address 레코드)
  - `DeviceConfigs/TC37x_A_step.json`: PFLASH 6MB(0xA0000000), 물리 1MB×6,
    **논리(소거) 섹터 16KB**, UCB 0xAF400000
- 알고리즘의 레지스터 시퀀스는 **iLLD IfxFlash**(TC3xx, Boost Software License 1.0 —
  파생 구현 허용)로 확정:
  - 커맨드 인터페이스: `0xAF000000 | {0x5554, 0xAA50, 0xAA58, 0xAAA8, 0x55F0}`
  - 소거: [AA50]=sectorAddr, [AA58]=1, [AAA8]=0x80, [AAA8]=0x50
  - 페이지(32B): enterPageMode 0x50 → [55F0]에 64비트×4 로드 →
    [AA50]=pageAddr, [AA58]=0, [AAA8]=0xA0, [AAA8]=0xAA
  - 상태: `DMU_HF_STATUS`(0xF8040010) P0BUSY=bit2/P1BUSY=bit3,
    에러 `DMU_HF_ERRSR`(0xF8040034)
  - ENDINIT: SCU `WDTS_CON0`(0xF00362A8) [ENDINIT0/LCK1/PW15:2] 클리어·복원
  - 워치독 비활성화: CON1.DR(bit3) — CPU0(0xF003624C/50), Safety(0xF00362A8/AC)

## 2. 자체 구현 (원본에 없는 신규)

- `flasher/m25_flash_loader.c` (TriCore C, HighTec 툴체인, **858 B**):
  - PSPR(0x70100000)에서 실행, DSPR 메일박스(0x7000F000) 폴링
  - 명령: erase(섹터 N개)/program(32B 페이지 N개)/exit
  - 시작 시 CPU0 + Safety 워치독 비활성화(표준 플래셔 관행), ENDINIT 클리어/복원
- `tools/m25_4b_flash.js` (호스트 드라이버, TCF/TAS 경유):
  - `save`(레지스터 40 + DSPR/PSPR 스크래치 저장 → 불변식),
    `load`(로더 업로드 → PC/SP 설정 → 실행 → magic 확인),
    `erase`/`program`/`read`/`verify`/`restore`, `stop`(ENDINIT 복원 + 상태 복원 + 재개),
    `scan`(섹터 상태), `bp`/`bpclear`

## 3. 라이브 검증 (마지막 논리 섹터 0xA05FC000 — 원래 내용 전부 0x00)

1. backup(16KB) → 2. erase(1섹터) → 3. program(로더 앞 256B 페이로드)
→ 4. **verify: 정확 일치 ✓** → 5. restore(0x00 16KB) → 6. **verify: 일치 ✓**
— **2회 반복 성공**

- 페이로드에 1-비트 포함 → erase 없이는 기록 불가 → **erase 실증** ✓
- 복원 후 대상 섹터 = 원래 값(0x00) ✓, 앱/다른 섹터 무변화 ✓ (앱 정상 실행 확인)
- 로더 기동: "loader ready (magic OK)" ✓

## 4. 알려진 제약 / 다음 단계

- 에이전트의 `-f` 엔트리 BP(M25-4a 파리티 — 앱 `_START`=0x8003A176에서 정지)가
  앱 재부팅 시 코어를 정지시켜 플래셔 운용과 간섭 → **리셋(트리거 클리어) 후 재개**로 해제 ✓
  (운영 플로우는 원본과 동일하게 "플래시 → 리셋 → 실행")
- 다음: ELF 로더블 섹션 자동 추출 + 부분 섹터 read-modify-write + 원본 `-f` 플로우 통합

## 6. M25-4b2 진행 (2026-10-19 후속): ELF 굽기 플로우

**구현 완료** (`tools/m25_4b_flash.js` 확장):
- ELF32 파서(PT_LOAD 추출) → PFLASH 세그먼트(0x80000000-0x805FFFFF) 필터
- 16KB 논리 섹터 단위 그룹핑 + **read-modify-write** 이미지 구성 +
  내용이 같으면 skip
- 쓰기 전 **섹터 백업 자동 저장** (`<elf>.sector_<addr>.bak`) +
  `flashrestore <elf>`(백업에서 복원)
- `flash <elf> [--dry-run]` 명령
- **코어1/2 정지** 후 플래시(앱의 코어 간 모니터 억제) + 3코어 상태 저장/복원
- `reset [run]` 명령(ContextReset 통합 — 리셋이 WDT를 UCB 기본 상태로 되돌림)
- 버그 수정: JS int32 비트연산으로 섹터 주소가 음수가 되던 문제(>>> 0)
- 로더 보강: ENDINIT 대기 **상한**(행 방지), magic 선기록, WDT 비활성화는
  best-effort(앱이 hidden 비밀번호를 쓰면 no-op → **리셋 선행**이 정석)

**현재 블로커**: 리셋 직후 상태에서 로더 기동(코어 재지향)이 되지 않는 문제.

**진단 결과 (2026-10-19 2차)**:
- `reset(suspend=true)` 직후: PC=0xA0000020 ✓, **R0(PC) 쓰기 자체는 정상**
  (0x70100000 쓰고 읽기 확인 ✓)
- 그러나 **resume하면 PC가 무시되고 부팅이 재시작됨** (PC가 0xA0031374/0xA0000160 =
  앱의 언캐시드 별칭 실행) — 즉 **리셋+홀트 후에는 resume = 부팅**이고,
  이 OCDS 상태는 이후의 suspend/resume에도 **고착**됨 (앱이 계속 부팅/실행, 로더 미기동)
- 리셋 이전(정상 디버그 홀트 상태)에서는 동일 플로우가 **2회 성공** ✓
  (save→load→erase/program/verify/restore)
- **복구 방법**: 보드 **전원 재투입**(power-cycle) — OCDS/디버그 래치와 RAM 상태가
  초기화되어 정상 디버그 상태로 복귀. (플래시 내용은 무관 — 무변경 확인 ✓)
- 추가 주의: 리셋/재부팅을 사이에 둔 채 이전 스냅샷을 복원하면 앱 RAM을 덮어쓸 수 있음
  → 운영 플로우에서는 **리셋 없이**(save→load→flash→stop) 수행 권장
- 후속 과제: 에이전트에 "PC 유지 resume" 프리미티브(리셋 상태에서도 PC 반영) 추가
  또는 플로우에서 reset 제거 + ENDINIT 실패 시 명시적 오류 처리(구현됨 ✓)

**타깃 무결**: 테스트 섹터(0x805F0000) 전부 0x00 유지 ✓ (실패 시도 중 쓰기 없음 ✓).

## 7. `-p` 타깃 옵션 구현 + DEVICE_CONNECT 진단 (2026-10-19 3차)

**구현** (원본의 `-p "Board"` 옵션 파리티 — 그동안 "accepted (ignored)"였음):
- `tcf/main/main.c`: `aurix_device_name` 전역 + `-p` 저장(무시 제거)
- `system/aurix/tcf/context-aurix.c`: TAS 타깃 목록(`tas_get_targets`)에서
  `-p` 이름과 매칭(`strcasecmp`/`strstr`) → **그 타깃으로 SESSION_START**,
  목록 전체 로깅 + 미발견 시 명시적 실패
- 검증(에이전트 로그 `-L`):
  ```
  AURIX: target [0] 'DAS JTAG AURDUINO 46YPEADW'   <- selected
  ```

**진단 도구**: `tools/m25_4b2_tas_probe.js` — tas_server(24817)에 직접
SERVER_CONNECT(0x80)+GET_TARGETS(0x86) 질의 (4바이트 LE 길이 프레임,
타깃 엔트리 88B: identifier[64]+type+id[4]+phys+nclient).
결과: 타깃 목록/카운트 정상 ✓, `nclient` = 접속 클라이언트 수(에이전트 유무로 검증 ✓).

**ELF 로드 불가 원인 규명** (에이전트 `-L` 로그):
```
AURIX: DEVICE_CONNECT failed (tas_err=0x20)
```
- 0x20 = `TAS_PL1_ERR_CMD_FAILED` — **DAS↔타깃 JTAG 통신 실패**(하드웨어 측)
- 소프트웨어 경로는 전부 정상 ✓ (서버 접속/타깃 목록/세션/키/`-p` 선택)
- 즉 ELF 심볼 로드(TCF MemoryMap) 이전 단계인 디바이스 접속에서 막힘 →
  단일 USB(COM3+COM4+COM5+debug) 재열거/보드 재부팅 필요

## 5. 산출물

- `flasher/m25_flash_loader.{c,ld,bin}` + `flasher/m25_flash_loader_build.ps1`
- `tools/m25_4b_flash.js`
- 도구 체인: `C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin`

## 8. RUNCMD 값 스윕 + 디바이스 단절 패턴 (2026-10-19 4차)

- **RUNCMD(0xF000049C) 값 1/2 모두**: run → 부팅 (CSFR PC=0xa0031374)
  — 값과 무관하게 **DAS 레벨 run의 의미가 "부팅"** (강제 halt 후에도 동일)
- **step은 PC 존중** ✓ (0x70100000 → 02 → 04, 로더 실행) — 현재 유일한 작동 경로
- **디바이스 단절 패턴**: 유휴 시 96초+ 안정 ✓ /
  **에이전트 하드킬 → 스테일 TAS 세션 → 디바이스 목록 소실(count=0)** ✗
  → tas_server 재시작으로 해소 (운영 규칙: 에이전트 재시작 시 tas_server도 재시작)
- 다음 후보: step 대량 count 기반 실행 / no-reset 플로우(hot attach) /
  DAS run을 대체하는 경로(예: BP 걸고 step 연속)

## 9. DAS API 완전 규명 (2026-10-19 5차, M25-4c)

- `das_api_load(version, *state)`: **v3/v4 성공**, v5 -> state=0x01000000 (버전 오류)
  — 실호출 검증 완료 (PowerShell P/Invoke)
- 반환 = **정보 구조체** (단순 함수 테이블이 아님):
  ```
  [0] -> "DAS Client DLL"   [1] -> "Infineon"   [2] = 0x03040302 (버전)
  [3] -> "Apr 30 2026"      [4] = 0
  [5]..[30] = 약 28개 함수 포인터 (DLL RVA 0x1000 ~ 0x1D50)
  [31]+ = 데이터(문자열/상수)
  ```
- `das.dll` = **Windows 서비스** (익스포트: ServiceMain, SvchostPushServiceGlobals만)
  → das_api.dll = 클라이언트 API, das.dll = DAS 서비스 본체
- tas_server는 DAS DLL을 쓰지 않음 (FTD2XX 자체 구현) — 확인 완료
- **다음**: 함수 포인터 28개의 역할 디스어셈블 -> C 테스트(디바이스 목록 조회)
  -> **"AURDUINO" 인식 판정** (성공 시 에이전트 DAS API 백엔드 구현 착수)

## 10. das_api.dll = 공식 TAS/DAS 클라이언트 API (2026-10-19 6차) - 결정적 발견

- das_api.dll 문자열이 **tas_server와 동일한 TAS API 오류 세트**:
  "ERROR: Wrong TAS API usage", "ERROR: Set acc_mode not supported for %s of addr %llX %s",
  "ERROR: Could not read channel description from device", "ERROR: Target not connected" 등
- 즉 **das_api.dll = 공식 TAS 클라이언트**(우리 aurix-tas.c가 손으로 구현한 와이어와 동일 프로토콜),
  **tas_server = TAS 서버**(DAS 구현: FTD2XX -> AURDUINO 어댑터)
- 아키텍처: `[das_api.dll/TAS 클라이언트] <-> [TAS 서버] <-> FTD2XX <-> AURDUINO <-> 타깃`
- **두 가지 경로**:
  (a) 에이전트가 das_api.dll(공식 클라이언트) 사용 - 여전히 TAS 서버 필요
  (b) **das_api.dll -> 표준 DAS 서비스(das.dll)** - tas_server 제거 가능성
      (표준 DAS가 AURDUINO를 인식하는지가 관건 - 다음 실험)
- 다음: 28개 함수 역할 파악 -> 디바이스 목록 조회 테스트 -> AURDUINO 인식 판정

## 11. M25-4c 7차: 아키텍처 확정 (2026-10-19)

- **정정**: `C:\WINDOWS\system32\das.dll` = Windows "Device Association Service" (무관, 이름 중복)
- **Infineon 스택**: `das_api.dll`(TAS 클라이언트, "Infineon"/TAS_INSTALL_DIR/start_tas_server.bat 문자열)
  + `mcdxdas.dll`(MCD↔DAS 브리지, 2026-05-02 동일 빌드) + `tas_server.exe`(사용자 TAS 서버 = AURDUINO 드라이버)
- **원본 경로**: aurix-debugger -> MCD -> mcdxdas.dll -> das_api.dll -> tas_server -> FTD2XX -> AURDUINO
  (원본 `-p "Board"` = TAS 타깃 선택 - 우리 `-p` 구현과 일치)
- **das_api.dll API**: das_api_load -> 정보 구조체 + 약 28개 함수 포인터;
  함수들은 TAS 세션 오브젝트(매직 0x1B30CCF7 + 버전 3/4) 검증 후 동작;
  다수 엔트리가 동일 스텁(0x1D30)을 가리킴(미지원 함수 패턴)
- **결론**: tas_server는 커스텀 어댑터의 드라이버로 **필수** - 제거하려면 FTDI/DAS 저수준 재구현(대규모).
  실질 대안: 에이전트가 das_api.dll(공식 클라이언트) 사용 (경로 a) - tas_server 유지
- 다음: das_api.dll 함수 역할 매핑(디바이스 목록) 또는 플래셔 블로커(step 기반 run) 우선순위 결정

## 12. TC27x 포팅 + 라이브 검증 (2026-10-20)

**대상 보드 확인**: CBS_JTAGID(0xF0000464) = `0x401DA083` → TC27x (SCU_CHIPID 0x47427060). 플래시 맵과 레지스터는
Infineon AURIX Flasher의 `DeviceConfigs/TC27x_D_step.json`(섹터 맵)과 `flax_TC2xx.hex`(공식 알고리즘, 역어셈블)로 확인.

**TC37x 로더를 TC275에 쓰면 안 되는 이유** (모두 수정):

| 항목 | TC37x (기존) | TC27x |
|---|---|---|
| 상태/에러 | DMU_HF_STATUS 0xF8040010 / ERRSR 0xF8040034 | FLASH0_FSR 0xF8002010 (busy 비트 0-4, OPER/SQER/PROER 11-13) |
| Safety WDT CON0/1 | 0xF00362A8/AC | 0xF00360F0/F4 |
| CPU0 WDT CON0/1 | 0xF003624C/50 | 0xF0036100/04 |
| PFLASH | 6MB, 16KB 균일 | 4MB, 논리 섹터 54개 (16K×8, 32K×8, 64K×4, 128K×3, 256K×4 ×2 뱅크) |

**함께 찾은 결함**:
1. **실패한 읽기가 0으로 보임** (호스트): `memRead`가 TCF 에러를 무시하고 0으로 채운 버퍼를 반환. TC27x는
   **지워진 PFLASH 페이지를 읽으면 0이 아니라 ECC 에러**(FLASH0_FSR PFDBER)가 나는데, 이것이 "0x00 = 비어 있음"으로
   보였음 → 백업이 0으로 저장되고 그 백업을 "복원"하면 섹터 내용이 사라질 수 있었음. 읽기를 엄격하게(에러면 중단) 바꾸고,
   플래시는 32B 페이지 단위로 "데이터 / 지워짐"을 구분해 읽음(4KB → 512B → 32B로 나눠 재시도).
   이전 기록의 "원래 내용 전부 0x00"도 같은 원인이었을 가능성이 큼
2. **백업/복원 형식**: 백업 = 페이지별 데이터 + 지워진 페이지 목록(JSON). 복원·굽기는 지운 뒤 데이터 페이지만 쓰고,
   검증은 데이터 페이지 일치 + 지워진 페이지는 계속 지워진 상태인지 확인
3. **ENDINIT가 다시 서지 않음** (로더, TC37x에도 있던 버그): 잠긴 CON0에 비밀번호 접근 없이 수정 쓰기만 해서 무시됨 →
   로더 실행 후 CPU0/Safety WDT의 ENDINIT=0으로 남았음. iLLD 순서(비밀번호 접근 → 수정 접근)로 수정, 라이브에서 1로 복귀 확인
4. **WDT 비밀번호**: PW[7:2]는 반전되어 읽힘 → `읽은 값 ^ 0x3F` (iLLD/공식 알고리즘과 동일)
5. **메일박스 CMD 경쟁**: CMD를 쓰고 다시 읽어 확인하는 사이 로더가 명령을 가져가 0으로 지워 "verify failed" → CMD는 확인 생략
6. 로더 진입점을 `.text.entry`로 PSPR 맨 앞에 고정, 로더 실행 중 ICR.IE=0, 스택을 페이로드와 겹치지 않는 0x7000E000으로,
   지우기는 TC27x에서 하드웨어 "논리 섹터 범위 지우기" 한 번, 섹터 시작이 아닌 주소·PFLASH 밖·로더 미실행 시 거부,
   알 수 없는 JTAG ID는 거부(`M25_DEVICE`로만 강제)

**라이브 검증** (테스트 섹터 = PF1 `0xA021C000` 16KB, 원래 512 페이지 모두 지워짐, 앱 미사용):

| 단계 | 결과 |
|---|---|
| 섹터 맵 | 공식 JSON 54개 섹터와 주소·크기 전부 일치 |
| 쓰기 (0x00 1KB, 패턴 2KB) | 상태 0, 검증 OK, 데이터 페이지 32 / 64개 |
| 지우기 | 상태 0, 512 페이지 모두 지워짐 = 원래 백업과 일치 |
| ELF 굽기 (기존 데이터 위 64B + 빈 영역 비정렬 40B) | 66 페이지, 패턴 보존·blob 위치·패딩 독립 비교 일치 |
| flashrestore / restore | 굽기 전 상태, 이어서 원래(전부 지워짐) 상태와 일치 |
| 안전 장치 | 비정렬 지우기, 범위 밖, 로더 미실행 → 모두 거부 |
| 셀프테스트 `m25_4b_flash_selftest.ps1` | PASSED (backup → save → load → flash → flashrestore → verifysector → stop) |
| 사후 | 이웃 섹터·앱 섹터(0xA0000000, 데이터 26 페이지) 변화 없음, WDT CON0 = 0xFFFC000F(ENDINIT=1), 3코어 앱 실행 |

TC37x 로더(`m25_flash_loader.bin`)도 3~6번 수정이 들어갔지만 TC37x 보드에서는 다시 검증하지 않았습니다.

## 13. VS Code 연동 (2026-10-20)

**Infineon 플래셔(확장의 `enableFlashing`)는 이 보드에서 동작하지 않음**: 확장에 포함된 AURIX Flasher 3.0.18의
`DeviceConfigs`는 TC27x D-step(JTAG ID `0x501DA083`)만 있음 → `No device found with JTAG ID 0x401DA083`, exit 2
(장치 확인 단계에서 종료, 플래시 무변경 - 전체 백업과 비교로 확인). 연결 시 리셋 후 정지가 일어나므로 이후 core0 재개 필요.
확장은 실패 시 무조건 `-erase all`(플래시 전체 지우기)로 재시도했음 → 설치된 확장의 `out/extension.js`에서 재시도를
제거(원본: `extension.js.orig_erase_all`, 확장 업데이트 시 덮어써짐).

**M25 플래셔 연동**: `tools/m25_flash_task.ps1`을 `launch.json`의 `preLaunchTask`로 실행.

```
F5 → preLaunchTask: m25_flash_task.ps1 -Elf <elf> -Agent <tcf-agent.exe> -Device <이름> [-BackupDir <폴더>]
       1. 에이전트 시작 (이미 포트를 쓰는 에이전트가 있으면 그것을 사용)
       2. flashcheck: PFLASH가 ELF와 같으면 쓰지 않고 코어만 재개
       3. 다르면 save → load → flash(섹터별 백업·지우기·쓰기·검증) → stop → reset run (새 이미지로 부팅)
       4. 시작한 에이전트 종료 → 확장이 평소처럼 자기 에이전트로 디버그 세션 시작
   복원: 같은 스크립트에 -Restore (마지막 굽기 전 섹터 백업으로 되돌림)
   종료 코드가 0이 아니면 VS Code는 세션을 시작하지 않음
```

`m25_4b_flash.js` 추가 명령: `flashcheck <elf>`(쓸 필요 여부만, 종료 코드 0/3), `resumeall`,
`backupall <dir>` / `verifyall <dir>`(데이터가 있는 PFLASH 섹터 전체 백업·비교), `M25_BACKUP_DIR`(굽기 백업 위치).

**검증** (데모 워크스페이스 `C:\work\aurix_tcf_demo`, 구성 "M25 flash PF1 test image + debug"):
- 터미널 실행: 굽기(4 페이지, 검증) → 두 번째 실행은 "already matches"로 건너뜀 → `-Restore`로 원상태
- VS Code F5: preLaunchTask가 굽고 리셋·실행 → 디버그 세션 시작 → 세션 중 flashcheck 일치 확인
- VS Code 작업 "m25: restore PF1 test sector": 512 페이지 모두 지워진 원래 상태로 복원
- 사후: 앱 섹터 12개 = 전체 백업과 일치, 3코어 앱 실행


## 14. Infineon AURIX Flasher(확장의 `enableFlashing`)로 이 보드 굽기 (2026-10-20)

13절의 "동작하지 않음"은 **장치 목록에 없어서**였고, 목록에 추가하면 Aurix Flasher로도 PFLASH에 쓸 수 있다.
두 플래셔의 차이: Infineon 플래셔는 ELF 세그먼트를 **ELF에 적힌 주소**에 쓴다 → RAM 주소로 링크한 ELF
(`demo.elf`, PSPR/DSPR)는 SRAM에만 쓰이고(`Found Data to write/verify in SRAM`) 브레이크·스텝이 동작하지 않는다.
PFLASH 주소(0x8…)로 링크한 ELF를 써야 한다 (`examples/aurix_tcf_demo/demo_flash.ld`).

**장치 추가** (설치된 확장 `…\extensions\zehuazhang.vscode-aurix-tcf-debug-0.1.0\bin\aurixflasher\DeviceConfigs\`):

1. `TC27x_D_step_Extended_ADAS.json`을 `TC27x_C_step_ED.json`으로 복사하고 세 값만 변경
   - `deviceName`: `TC27x C step ED (JTAG ID 0x401DA083, added for ShieldBuddy TC275ED)`
   - `ChipID`: `0x47427060`, `JTAGID`: `0x401DA083` (C-step. D-step은 `0x501DA083`)
   - 메모리·섹터 맵은 그대로 (PD/ED는 ChipID만 다르고, C/D-step은 같은 플래시 구성)
2. `devices.json`의 배열에 `"TC27x_C_step_ED.json"` 한 줄 추가 (원본은 `devices.json.orig_m25`로 보관)
3. VS Code 창 Reload Window

JTAG ID는 `m25_4b_flash.js device` 또는 `No device found with JTAG ID 0x…` 메시지로 확인한다.
**확장을 업데이트하면 이 변경이 사라진다** (다시 적용).

**`-erase all` 재시도 제거**: 확장은 플래싱이 실패하면 `-erase all`(PFLASH 전체 지우기)로 자동 재시도한다.
연결 실패·ELF 오류에서도 전체가 지워지므로 `out/extension.js`의 재시도 블록을 제거하고 실패 메시지만 남겼다
(원본 `extension.js.orig_erase_all`, 확장 업데이트 시 덮어써짐).

**검증** (`examples/aurix_tcf_demo`, 구성 "aurix_tcf_demo (PFLASH, CPU0)", `enableFlashing: true`):
- `aurixflasher.exe -gui off -elf demo_flash.elf -start off -ver on -id 0` → Connected `[TC2x] TC27x`,
  Erasing / Programming / Verifying 모두 Pass, `Flashing completed successfully` (PF1 `0xA021C000`, 8 페이지)
- 세션 시작 후 `node tools/flash_start.js`(플래시 내용 일치 확인, core0 PC = `main`) →
  줄 브레이크 적중("PAUSED ON BREAKPOINT"), 호출 스택 `scale → accumulate → compute → main`(소스 줄 포함),
  지역 변수, 플래시 주소 디스어셈블리, Step Over/Into — 모두 정상
- 사후: `reset run`, 앱 섹터 12개 = 전체 백업과 일치

**주의**: Aurix Flasher 실행 후에는 지워진 페이지가 리셋 전까지 데이터로 읽히고 core0 PC가 플래셔 스텁에 있다
(`-start off`). 디버그 세션의 시작 PC를 지정하려면 `flash_start.js`처럼 PC를 직접 설정하거나 리셋한다.
데모 프로젝트: `examples/aurix_tcf_demo/` (RAM 링크 / PFLASH 링크, 두 플래셔 구성).
