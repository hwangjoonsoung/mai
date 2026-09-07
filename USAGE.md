# USAGE — multi_ai_cli 사용법

Claude · Codex · Gemini(agy) 세 AI를 한 채팅방에서 쓰는 방법.

> 이 문서는 **실제로 돌려서 확인한 것만** 적었다. 설계 배경은 [INTENT.md](INTENT.md), 스펙은 [SPEC.md](SPEC.md)에 있다.

---

## 1. 처음 한 번만 — 환경 점검

세 CLI가 설치·로그인돼 있어야 한다. API 키는 필요 없고 **각 CLI의 구독 인증을 그대로 쓴다.**

**Windows**

```powershell
cd C:\Users\HJS\Desktop\multi_ai
.\scripts\doctor.ps1
```

**macOS**

```bash
cd ~/multi_ai
./scripts/doctor.sh
```

정상이면 이렇게 나온다.

```
== CLI 설치 확인 ==
  claude   C:\Users\HJS\.local\bin\claude.exe
           2.1.259 (Claude Code)
  codex    C:\Users\HJS\AppData\Roaming\npm\codex
           codex-cli 0.152.1
  agy      C:\Users\HJS\AppData\Local\agy\bin\agy.exe
           1.1.25

== Codex 실행 경로 (SPEC §6.3) ==
  tier 1  네이티브: ...\codex-win32-x64\vendor\...\bin\codex.exe

== agy 사용 가능 모델 (SPEC D13) ==
  gemini-3.8-flash-high
  gemini-3.1-pro-high
  ...
```

`MISSING` 이 뜨면 그 AI는 **비활성으로 처리되고 나머지로 진행한다.** 앱이 멈추지 않는다.

---

## 2. 빌드와 실행

빌드는 `javac` 하나뿐이다. Gradle·Maven 없고 외부 라이브러리도 없다.

**Windows**

```powershell
.\scripts\compile.ps1      # out\ 에 클래스 생성
.\scripts\run.ps1          # 현재 디렉터리를 워크스페이스로 실행
```

**macOS**

```bash
./scripts/compile.sh
./scripts/run.sh
```

`run` 은 빌드 산출물이 없으면 알아서 컴파일부터 한다.

### 워크스페이스 지정

AI들이 읽을 대상 프로젝트다. **생략하면 현재 디렉터리**가 된다.

```powershell
.\scripts\run.ps1 -Workspace C:\Users\HJS\IdeaProjects\aikconf
```

```bash
./scripts/run.sh --workspace ~/IdeaProjects/aikconf
```

워크스페이스는 **방을 만들 때 한 번 정해지면 그 방에 고정**된다. 라운드마다 바뀌지 않는다.

### 이전 방 이어서 열기

```powershell
.\scripts\run.ps1 -Room 20260903-210657
```

방 ID는 `/rooms` 로 확인한다.

---

## 3. 기본 사용 — 그냥 말하면 셋 다 답한다

`run` 하면 먼저 이 화면이 뜬다.

```
                            multi_ai_cli
                  Claude  ·  Codex  ·  Gemini via agy

        ┌────────────────────────────────────────────────┐
        │              질문을 입력하세요                 │
        └────────────────────────────────────────────────┘

                  /help 명령 목록   ·   /exit 종료
```

질문을 던지면 **참여자마다 자기 칸을 갖는 화면**으로 바뀐다.

```
┌──────────────────────────────────────────────────────────────────────────┐
│ 로그인 설계에서 빠진 위험을 찾아줘                                       │
├──────────────────────────────────────────────────────────────────────────┤
┌ dir ──────────┐┌ Claude 5.8s [완료] ┐┌ Codex 6.4s [완료] ┐┌ Gemini ───┐
│ C:/Users/HJS/ ││ 세션 고정 위험이   ││ 1. 토큰 만료 처리 ││ 우선순위  │
│ IdeaProjects  ││ 있습니다...        ││ 가 없습니다...    ││ 는...     │
└───────────────┘│                    ││                   ││           │
┌ session ──────┐│                    ││                   ││           │
│ 20260904-0930 ││                    ││                   ││           │
│ 라운드 1      ││                    ││                   ││           │
│ 메시지 4      ││ … 12줄 더 (/v 1)   ││                   ││           │
└───────────────┘└────────────────────┘└───────────────────┘└───────────┘
 /v <번호|이름> 한 칸 크게 · /s <번호> <줄> 스크롤 · /n 새 질문 · /exit
```

- **왼쪽** — 작업 디렉터리(`dir`)와 방 정보(`session`)
- **오른쪽** — 참여자별 답변 칸. 완료되는 대로 각 칸이 채워진다
- 칸에 다 안 들어가면 `… n줄 더` 로 알린다

### 한 칸을 크게 보기

```
/v 1           /v claude      번호로도 이름으로도
/v             분할 화면으로 복귀
/s claude 40   그 칸을 40번째 줄부터 표시
/n             질문 전 화면으로
```

`/v <이름>` 은 그 답변만 **전체 화면**으로 펼친다. 긴 답을 읽을 때 쓴다.

> 터미널이 ANSI 를 못 다루면 `config.properties` 에 `ui.mode=plain` 을 넣는다. 예전 방식(순서대로 출력)으로 돌아간다.

### 한 명만 지목하기

```
@claude 이 코드의 동시성 문제를 봐줘
@codex 위 지적을 반박해봐
@claude @gemini 둘 다 의견 줘
```

멘션은 문장 맨 앞에, 여러 개 붙일 수 있다.

### 협업이 일어나는 방식

**같은 라운드에서는 서로의 답을 못 본다.** 다음 라운드부터 직전 라운드의 답이 공동 문맥에 들어간다. 먼저 답한 AI에 동조하는 걸 막으려는 설계다.

```
1턴: "이 설계 어때?"        → 셋이 각자 독립적으로 답함
2턴: "서로 의견 보고 다시"   → 이제 서로의 1턴 답을 읽고 답함
```

다른 AI의 stderr·추론 과정·툴 호출 로그는 **절대 넘기지 않는다.** 사용자에게 공개된 최종 답변만 넘어간다.

---

## 4. 명령어 전체

| 입력 | 동작 |
|---|---|
| 일반 문장 | 전 참여자 동시 호출 |
| `@claude` / `@codex` / `@gemini <질문>` | 지목 호출 |
| `/run @<참여자> [--write] <프롬프트>` | 지정 권한으로 **1회** 실행 |
| `/preset list` | 저장된 프리셋 목록 |
| `/preset save <이름> [@참여자...] <프롬프트>` | 프리셋 저장 |
| `/preset run <이름>` | 프리셋 실행 |
| `/preset rm <이름>` | 프리셋 삭제 |
| `/v <번호\|이름>` | **한 칸을 전체 화면으로.** 인자 없으면 분할 화면 복귀 |
| `/s <번호\|이름> <줄>` | 그 칸을 지정 줄부터 표시 (스크롤) |
| `/n` | 질문 전 화면으로 |
| `/context [n]` | 프롬프트에 실을 이전 대화 수. **토큰 조절** |
| `/converge [@수렴자] <안건>` | **구조화 교차검증 → REPORT.md** |
| `/status` | 참여자 경로·방 상태 |
| `/status auth` | 버전·인증·사용 가능 모델까지 확인 |
| `/rooms` | 저장된 방 목록 |
| `/open <방 ID>` | 기존 방 열기 |
| `/new [이름]` | 새 방 시작 |
| `/cancel [참여자]` | 실행 중 프로세스 종료 |
| `/help` | 명령 목록 |
| `/exit` | 저장 후 종료 |

---

## 5. 파일을 고치게 하려면 — `/run --write`

**기본은 전원 읽기 전용이다.** 아무리 시켜도 파일을 못 고친다. 쓰기는 명시적으로 열어야 한다.

```
/run @claude --write src/main/java 아래 NPE 가능성만 방어 코드로 막아줘
```

### 규칙

- **딱 그 호출 한 번만** 쓰기가 열린다. 세션에 남지 않는다.
- **한 명만 지목할 수 있다.** 여러 AI가 같은 워크스페이스를 동시에 고치면 충돌한다.

```
/run @codex @claude --write 뭔가
  ! --write 는 참여자 하나만 지목할 수 있다. 같은 워크스페이스에 여러 AI 를
    동시에 쓰기 모드로 실행하지 않는다 (SPEC §7.5).
```

### 실행 후 변경 목록이 나온다

```
== 워크스페이스 변경 (GIT) ==
   M src/main/java/io/multiai/cli/Main.java
  ?? src/main/java/io/multiai/cli/Foo.java

  · 커밋·푸시는 수행하지 않는다. 필요하면 직접 실행하라.
```

Git이면 `git status --short`, SVN이면 `svn status` 를 **읽기 전용으로** 돌린다. **커밋·푸시는 어떤 경우에도 하지 않는다.**

---

## 6. 프리셋 — 자주 쓰는 프롬프트 저장

```
/preset save 리뷰 @codex @claude 이 변경의 위험을 심각도 순으로 짚어라
/preset list

  리뷰            [codex,claude]      이 변경의 위험을 심각도 순으로 짚어라

/preset run 리뷰
```

**프리셋은 권한을 저장하지 않는다.** 실행해도 언제나 읽기 전용이다. 쓰기가 필요하면 그때마다 `/run --write` 를 직접 쳐야 한다.

역할(`planner`/`reviewer` 같은 것)은 제품에 내장돼 있지 않다. 프리셋은 **프롬프트 텍스트와 대상 멘션만** 저장한다. 역할은 사용자가 프롬프트로 정한다.

---

## 7. 핵심 기능 — `/converge` 교차검증

**이 프로젝트를 만든 이유다.** 혼자 판단하기 어려운 설계 결정을 셋에게 던져서 쟁점을 좁힌다.

```
/converge transcript 손상 복구를 best-effort 로 두는 것이 타당한가?
```

### 진행 과정

```
  · 검토자: [Claude, Codex, Gemini via agy]
  · 수렴자: 규칙 기반 (모델 호출 없음)
  · 1라운드 — 3명 독립 검토
  [Codex] CONCERNS · 지적 3건
  [Claude] CONCERNS · 지적 4건
  [Gemini via agy] CONCERNS · 지적 2건
  · 2라운드 — critical·major 단독 지적이 있다
  [Codex] CONCERNS · 지적 3건
  [Gemini via agy] CONCERNS · 지적 3건

== 판정 요약 ==
  Claude            CONCERNS
  Codex             CONCERNS
  Gemini via agy    CONCERNS

  합의 2 · 이견 1 · 단독 지적 4 · 미해결 3

== 사용자 결정 필요 ==
  - Codex: 복구된 transcript에 손상·결측 상태가 명시적으로 표시되는가?
  ...

  · 보고서: ...\runs\r0001\converge\REPORT.md
```

1. **1라운드** — 셋이 서로 못 본 상태에서 같은 JSON 스키마로 답한다
2. **분류** — 합의 / 이견 / 단독 지적 / 미해결
3. **2라운드** — 이견이나 critical·major 단독 지적이 있을 때만. **상대 의견을 첨부해** 다시 묻는다. 누가 말했는지는 안 알려준다
4. **보고서** — `REPORT.md` 생성

**최대 2라운드까지만이다.** 3라운드는 없다.

### 수렴자 지목

```
/converge @claude 이 설계의 위험은?
```

지목하면 **그 AI는 검토에서 빠지고** 나머지 둘의 답을 분류한다. 자기 답을 자기가 채점하면 안 되기 때문이다.

- 지목 **안 하면** — 셋 다 검토, 규칙 기반으로 분류 (모델 호출 없음)
- 지목 **하면** — 둘이 검토, 지목된 AI가 분류

> 참여자가 2명뿐일 때는 지목하지 마라. 검토자가 1명만 남아 거부된다.

### 터미널과 보고서의 차이

터미널에는 **판정 요약과 미해결만** 나온다. 전문은 `REPORT.md` 에 있다.

`REPORT.md` 는 **판정하지 않는다.** 어느 쪽이 옳은지 단정하지 않고 분류해서 사용자 앞에 놓는 것까지가 역할이다. 기각한 지적도 이유와 함께 남긴다.

---

## 8. 실행 중 취소 — `/cancel`

라운드가 도는 중에도 칠 수 있다.

```
multi-ai(...)> @gemini 300까지 천천히 세어라
[Gemini via agy · 실행 중]

/cancel gemini
  · 취소 요청: gemini
[Gemini via agy] 취소됨  0.3s
```

- `/cancel` — 전 참여자
- `/cancel gemini` — 그 참여자만

**보장 수준은 best-effort다.** 자식 프로세스부터 역순으로 종료하고, 안 되면 `taskkill /T /F`(Windows) 또는 `kill -9`(macOS)로 넘어간다. 그래도 남는 PID가 있으면 **조용히 넘기지 않고 오류로 표시한다.**

> "생존 PID 없음"은 "전부 종료됐다"가 아니라 **"추적한 범위에서 남은 게 없다"** 는 뜻이다. 이미 분리된 자식 프로세스는 못 잡는다.

라운드 중에는 `/cancel` 외의 입력은 무시된다.

---

## 9. 설정 — `config.properties`

```
Windows : %USERPROFILE%\.multi-ai-cli\config.properties
macOS   : ~/.multi-ai-cli/config.properties
```

```properties
# 프롬프트에 실을 이전 대화 수. 여기가 토큰이 나가는 지점이다.
# 0 이면 이전 대화를 안 넣는다 (매 질문이 독립 질의)
context.messages=12
context.chars=16000

# 화면 분할 UI. ANSI 를 못 다루는 터미널이면 plain
ui.mode=tui

# agy 기본 모델. 검토용이라 추론 강한 pro 를 기본값으로 쓴다
agy.model=gemini-3.1-pro-high

# 계정이 기본 모델을 거부할 때만 지정
codex.model=

# 자동 탐색이 실패할 때 실행 파일 경로를 직접 지정
claude.path=
codex.path=
agy.path=
```

`agy models` 로 쓸 수 있는 모델을 확인할 수 있다. 빠른 응답이 필요하면 `gemini-3.8-flash-high` 로 바꿔도 된다.

---

## 10. 저장 구조

대화와 로그는 **대상 프로젝트가 아니라 홈 디렉터리**에 쌓인다. 워크스페이스를 오염시키지 않는다.

```
%USERPROFILE%\.multi-ai-cli\
  config.properties
  presets.properties
  temp\                        공급자 임시 출력
  rooms\
    20260903-210657\
      room.properties          방 메타데이터
      transcript.md            대화 기록 (SSOT)
      runs\
        r0001\
          claude.stdout.txt    원본 출력
          claude.stderr.txt
          codex.stdout.txt
          ...
          converge\REPORT.md   수렴 보고서
```

- `transcript.md` 는 **사람이 읽을 수 있으면서 프로그램이 정확히 되읽을 수 있다.** 본문 안에 마커처럼 생긴 문자열이 들어와도 안전하다.

> **기록 저장은 토큰을 쓰지 않는다.** 로컬 디스크 쓰기일 뿐이다.
> 토큰이 나가는 곳은 **이전 대화를 매 라운드 프롬프트에 실어 보내는 것**이고, 그건 `/context` 로 조절한다.
> 파일을 안 남기면 `/open` 재개와 `/v` 열람만 잃고 토큰은 그대로다.
- **원본 출력 파일에는 비밀정보가 들어갈 수 있다.** 공유 전에 확인하라.
- 토큰·OAuth 정보·API 키는 저장하지 않는다.

---

## 11. 문제가 생기면

### 특정 AI만 계속 실패한다

```
/status auth
```

버전과 인증 상태가 나온다. agy는 사용 가능한 모델 수까지 확인한다.

실패한 라운드의 stderr 경로가 출력에 찍히므로 그 파일을 보면 원인이 나온다.

```
  ! 실행 실패: 종료 코드 1
  ! stderr: ...\runs\r0001\codex.stderr.txt
```

### Codex가 `model is not supported` 로 실패한다

```
The 'gpt-5.6-sol' model is not supported when using Codex with a ChatGPT account.
```

**구독 상태를 확인하라.** 로그인 자격증명이 남아 있어도 구독이 만료되면 서버가 거부한다. `codex login status` 는 `Logged in` 으로 나오므로 이것만 봐서는 알 수 없다.

복구되면 **아무것도 안 고쳐도 자동으로 3자 구도로 돌아간다.**

### 한글이 깨진다

`run.ps1` / `run.sh` 를 쓰면 콘솔 인코딩이 자동으로 맞춰진다. `java` 를 직접 실행하면 깨질 수 있다.

### 스크립트를 고쳤더니 파서 오류가 난다

`.ps1` 파일은 **UTF-8 BOM 을 유지해야 한다.** Windows PowerShell 5.1 은 BOM 이 없으면 파일을 CP949 로 읽어 한글 주석이 깨지고 파서가 죽는다.

편집기에서 "UTF-8 with BOM" 으로 저장하면 된다. VS Code 라면 하단 인코딩 표시를 눌러 `Save with Encoding` → `UTF-8 with BOM`.

### 토큰을 줄이고 싶다

```
/context        현재 설정 확인
/context 4      최근 4개 메시지만 프롬프트에 싣는다
/context 0      이전 대화를 아예 안 싣는다
```

`config.properties` 의 `context.messages` 로 기본값을 바꿀 수 있다.

> `/context 0` 으로 두면 **"다음 라운드부터 다른 AI 의 답을 읽고 답한다" 는 협업 구조가 꺼진다.** 매 질문이 독립 질의가 되고 `/converge` 의 2라운드 반론도 의미가 약해진다. 단순 질의만 할 거면 0, 교차검증을 쓸 거면 4 이상을 권한다.

### 요청이 거부된다

```
  ! 현재 요청이 상한을 초과한다: 18000자 / 상한 16000자
  · 요청을 나눠서 보내라.
```

문맥 상한은 **최종 프롬프트 전체 16,000자**다. 대화가 길어지면 오래된 기록부터 자동으로 빠진다. 다만 **현재 요청 하나가 상한을 넘으면 잘라 보내지 않고 거부**한다 — 잘린 요청을 보내면 답이 틀리기 때문이다.

> Windows 명령행 한계가 32,767자이고 agy는 프롬프트를 인자로만 받을 수 있어서 생긴 제약이다. 세 AI에게 **같은 양의 문맥**을 줘야 답의 차이가 모델 차이인지 입력 차이인지 구분할 수 있으므로, 가장 제약이 큰 쪽에 맞춘다.

---

## 12. 안 하는 것

설계상 의도적으로 막아둔 것들이다.

| 안 함 | 이유 |
|---|---|
| 자동 커밋·푸시 | 대상 프로젝트 다수가 SVN이라 롤백이 어렵다 |
| 권한 우회 플래그 (`--yolo`, `--dangerously-*`) | 기본은 읽기 전용. 쓰기는 `/run --write` 1회만 |
| 여러 AI 동시 쓰기 | 같은 워크스페이스 충돌 |
| 고정 역할 내장 | 역할은 사용자가 프롬프트로 정한다 |
| 공급자 세션 재개 (`codex resume`, `agy -c`) | 세 세션을 각자 관리하면 동기화가 깨진다. 앱의 기록이 유일한 진실이다 |
| 3라운드 이상 반론 | 최대 2라운드. 안 좁혀지면 사용자가 결정한다 |

---

## 13. 자주 쓰는 흐름

**설계 검토**

```
/converge 이 API 를 동기로 갈지 비동기로 갈지
→ REPORT.md 읽고 미해결만 직접 결정
```

**구현 전 의견 수렴 → 구현 → 검증**

```
이 티켓 어떻게 구현하는 게 좋을까?          (셋 다)
@claude 방금 나온 의견들 중 뭐가 제일 나아?   (지목)
/run @claude --write 그 방향으로 구현해줘     (쓰기 1회)
@codex @gemini 방금 변경 검토해줘             (지목)
```

**빠른 단일 질의**

```
@gemini 이 정규식 뭐 하는 거야?
```

---

## 참고

| 문서 | 내용 |
|---|---|
| [INTENT.md](INTENT.md) | 무엇을 왜 만들었는가. 금지사항 |
| [SPEC.md](SPEC.md) | 구현 스펙. 결정사항 `D1`~`D18` |
| [REVIEW_REPORT.md](REVIEW_REPORT.md) | 5라운드 교차검토 기록 |
| [prompts/](prompts/) | 검토 요청·수렴 절차 프롬프트 |
