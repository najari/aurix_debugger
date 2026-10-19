# m25_tcf — 자체 AURIX TCF 에이전트 (M25)

`aurix-debugger.exe`(Infineon)의 "missing 10%"에 해당하는 AURIX 백엔드를
오픈 Eclipse TCF 에이전트 위에 구현한 소스 모음입니다.

**M25-1 완료 (2026-10-18)**: VS Code에서 ShieldBuddy TC275ED 3코어
디버깅(레지스터/메모리/스택프레임/run/pause) 라이브 성공.

## 구성

```
agent/system/aurix/tcf/
  context-aurix.c    네이티브 컨텍스트 백엔드 (원본의 context-aurix.cpp 대응)
  aurix-tas.c/h      TAS 와이어 클라이언트 (mcd_tas에서 이식)
agent/machine/aurix/tcf/
  cpudefs-mdep.c/h   TriCore 레지스터 정의 (EABI DWARF 번호, CSFR 주소, BREAK_INST)
  stack-crawl-tricore.c/h   CSA 체인 스택 크롤 (디버그 정보가 없을 때)
  disassembler-aurix-tricore.c/h   TriCore 디스어셈블러
agent/tcf/                 업스트림 파일을 대체하는 수정본 (같은 경로에 덮어씀)
  main/main.c              aurix-debugger 호환 옵션
  services/disassembly.c   ISA 이름 대소문자 무시
  services/dwarfframe.c    TriCore: 호출자 upper context를 CSA에서 복원
  services/stacktrace-ext.h  복귀 주소 0에서 스택 추적 종료
  services/runctrl.c       스텝 판정의 프레임 식별자 = CSA 주소 (PCXI가 있는 타깃)
  framework/cpudefs.c      PCXI가 0이면 CSA를 읽지 않음
flasher/                   자체 PFLASH 플래시 로더 (TC27x / TC37x 빌드) + TC27x 셀프테스트 이미지
tools/m25_4b_flash.js      플래셔 호스트 드라이버 (장치 자동 판별, 페이지 단위 백업·복원)
tools/m25_4b_flash_selftest.ps1  플래시 셀프테스트 (M25_4B_FLASHER.md 12절)
patches/
  cmake-tcf-lib.patch   업스트림 cmake-tcf-lib.txt 수정 (aurix glob + context-win32 제외 + ENABLE_*)
  main-options.patch    업스트림 main.c 수정 (aurix-debugger 호환 옵션 수용)
M25_1_TCF_AGENT_LIVE.md    마일스톤 보고서 (설계 규명·구현·검증 전문)
M25_TCF_AGENT_BLUEPRINT.md 설계도 (갱신본)
```

## 적용 방법 (업스트림 TCF 에이전트 클론 기준)

1. `agent/system/aurix/tcf/`, `agent/machine/aurix/tcf/` 디렉터리를 복사하고,
   `agent/tcf/` 아래 파일은 업스트림의 같은 경로 파일에 덮어씀 (기준: 업스트림 c441453)
2. `git apply patches/cmake-tcf-lib.patch` (또는 수동 반영)
3. `git apply patches/main-options.patch`
4. 빌드: `cmake -B build -G "Visual Studio 17 2022" -A x64 -DTCF_MACHINE=aurix`
   → `cmake --build build --config Release`
5. 실행: `tcf-agent.exe -s TCP:127.0.0.1:1534 -p "<디바이스 이름>"`
   (tas_server가 24817에서 실행 중이어야 함)

## 수정 (2026-10-20) — VS Code 테스트에서 찾은 결함 3건

1. **Step Out/Step Over 프레임 판정** (`stack-crawl-tricore.c`): FP(CFA)로 SP(A10)를 써서, 스택을 쓰지 않는
   TriCore 함수 체인에서는 모든 프레임 FP가 같았음 → 프레임워크(runctrl.c의 FP 비교)가 바깥 함수의 Step Out을
   엉뚱한 지점에서 "완료"로 판정. FP = 프레임 PCXI가 가리키는 **CSA 주소**(CALL마다 새로 할당, 호출 중 불변)로 변경
   → 바깥 함수 Step Out은 복귀할 때까지 실행, 안쪽 함수는 호출자(0x8003a264)에서 정지, CALL에서 Step Over 정상
2. **리셋 후 코어 상태 고착** (`context-aurix.c` 모니터): "정지" 컨텍스트를 폴링하지 않아, 리셋 후 펌웨어가 CPU1/2를
   시작해도 SUSPENDED로 남고 Pause가 무효 → 정지로 알고 있는데 하드웨어가 실행 중이면 started 이벤트 보고
3. **브레이크포인트 오귀속** (`context-aurix.c`): TRIG_ACC 비트만 보고 그 유닛의 BP에 정지를 귀속 → 스텝 트랩(TR0)과
   부트 소프트웨어의 halt-after-reset 트리거(TR0, 0x1022@0xAFFFD418)가 BP 적중으로 보고되고 HitCount 증가.
   명령 BP는 정지 PC가 BP 주소일 때만 귀속, 스텝 후 TRIG_ACC 폐기, 리셋 후 모든 코어의 트리거 유닛을
   모델대로 재기록(부트 소프트웨어가 덮어쓴 TR0의 사용자 BP 복구)

## 수정 (2026-10-20, 2차) — 호출 스택·디스어셈블리·스텝

GCC(`-O0 -g3`)로 빌드한 RAM 데모(`main → compute → accumulate → scale → square`)를
VS Code에서 디버깅하며 찾은 결함입니다.

4. **디스어셈블리가 CPU0에서만 동작** (`context-aurix.c`): TriCore 디스어셈블러를 첫 코어의 CPU 그룹에만
   등록 → CPU1/2에서는 `.half`만 나옴. 코어마다 등록
5. **호출 스택이 2단계에서 끊김** (`dwarfframe.c`): 디버그 정보가 있으면 DWARF CFI로 스택을 푸는데, GCC CFI에는
   CALL이 CSA에 저장한 upper context(A10, A11, A12-A15, D8-D15, PSW, PCXI) 복원 규칙이 없음 → 두 번째 프레임부터
   A11/A14/PCXI를 몰라 중단. TriCore에서는 이 레지스터들을 PCXI가 가리키는 CSA에서 읽음 → 5단계 모두 표시
6. **스택 끝 처리**: 복귀 주소(A11)가 0인 프레임은 체인의 끝으로 처리 (`stacktrace-ext.h`), PCXI가 0이면 CSA를
   읽지 않음 — 이전에는 주소 0x0~0x3C 읽기 실패가 정지마다 16번 발생 (`cpudefs.c`), CSA 크롤이 A11=0인
   호출자 프레임까지 버리던 문제 (`stack-crawl-tricore.c`)
7. **Step Over가 호출된 함수 안에서 멈춤** (`runctrl.c`): GCC의 CFA는 함수 진입 시 SP라서 스택 프레임이 없는
   함수(compute)는 호출자와 FP가 같음 → 프레임워크가 CALL 뒤를 "같은 프레임"으로 보고 accumulate 진입점에서 정지.
   스텝 판정의 프레임 식별자를 PCXI가 가리키는 CSA 주소로 변경 (VS Code에 보이는 FP와 지역 변수 계산은 그대로 CFA)
8. **스텝이 줄 중간에서 "Suspended"로 끊김** (`context-aurix.c`): 단일 스텝은 started를 보내고 정지 보고를
   이벤트로 미루는데, 그 사이 상태 모니터가 정지한 코어를 비동기 정지로 보고하며 intercept를 걸었음(경쟁 상태).
   정지 보고가 예약된 코어는 모니터가 건너뜀
9. **Step Over가 복귀 지점에서 "Breakpoint"로 멈춤** (`context-aurix.c`): 하드웨어 BP 정지마다 intercept를 강제해
   프레임워크 내부 스텝 BP에서도 클라이언트가 멈춤. BP 정지의 intercept는 Breakpoints 서비스가 결정하도록 변경
   (사용자 BP는 그대로 정지, HitCount 1)

검증 (VS Code, 데모 프로젝트 `C:\work\aurix_tcf_demo`): 호출 스택 5단계 + 프레임별 지역 변수, 디스어셈블리,
Step Over(호출 포함, 긴 줄 포함), Step Into(프롤로그 건너뜀), Step Out(FP가 같은 프레임 포함), 줄 BP — 모두 정상.

## 검증 도구

- `tcf_test_client.js` — TCF 프로토콜 스모크 (컨텍스트·레지스터·메모리·suspend)
- `ext_client_test.js` — VS Code 확장 자체 클라이언트(out/tcfclient.js)로 조회
  (위 파일들은 `C:\work\tcf_agent\`에 있음 — 필요 시 이 디렉터리로 이동)
