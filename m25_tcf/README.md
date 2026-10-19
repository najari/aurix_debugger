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
patches/
  cmake-tcf-lib.patch   업스트림 cmake-tcf-lib.txt 수정 (aurix glob + context-win32 제외 + ENABLE_*)
  main-options.patch    업스트림 main.c 수정 (aurix-debugger 호환 옵션 수용)
M25_1_TCF_AGENT_LIVE.md    마일스톤 보고서 (설계 규명·구현·검증 전문)
M25_TCF_AGENT_BLUEPRINT.md 설계도 (갱신본)
```

## 적용 방법 (업스트림 TCF 에이전트 클론 기준)

1. `agent/system/aurix/tcf/`, `agent/machine/aurix/tcf/` 디렉터리를 복사
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

## 검증 도구

- `tcf_test_client.js` — TCF 프로토콜 스모크 (컨텍스트·레지스터·메모리·suspend)
- `ext_client_test.js` — VS Code 확장 자체 클라이언트(out/tcfclient.js)로 조회
  (위 파일들은 `C:\work\tcf_agent\`에 있음 — 필요 시 이 디렉터리로 이동)
