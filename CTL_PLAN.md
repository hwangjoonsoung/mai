# `mai ctl ask` 구현 계획 — AI 끼리 검증하기 (macOS 우선)

2026-11-11 작성. 결정 사항은 메모리 «mai 검증 방향과 스타일»(방식 B) 와 Codex 의
Windows 샌드박스 검증 결과를 따른다. **스타일(색·기호·들여쓰기)은 이 계획에서
다루지 않는다.** 기능 검증이 끝난 뒤 별도로 적용한다.

---

## 0. 한 줄 요약

칸 안에서 도는 AI 가 셸 명령 `mai ctl ask <에이전트> "<질문>"` 을 실행하면,
실행 중인 mai 가 **옆에 검증 칸을 열어** 그 에이전트에게 질문을 넣고, 답이 끝나면
그 답을 **명령의 표준 출력으로 돌려준다.** 사람은 아무 키도 누르지 않는다.

```
┌ Claude (요청자) ──────────┐ ┌ Codex ← Claude (검증자) ───┐
│ > mai ctl ask codex "…"   │ │ [Claude 의 검증 요청]       │
│   (답을 기다리는 중)        │ │ """ … """                   │
│                           │ │ … Codex 가 답하는 중 …      │
│ ← 답이 stdout 으로 돌아옴  │ │ 답 끝 → mai 가 회수해 전달  │
└───────────────────────────┘ └─────────────────────────────┘
```

---

## 1. 지금 코드에서 확인한 사실 (설계의 근거)

| 사실 | 위치 | 설계에 미치는 영향 |
|---|---|---|
| 새 칸은 늘 **셸**이다. 에이전트는 그 셸에서 사용자가 `claude`·`codex` 를 쳐서 띄운다 | `app.rs` `spawn_terminal`, `model.rs` `Session::terminal` | 요청자 칸은 `agent_id` 가 빈 문자열이다. 요청자가 누구인지는 **환경변수**로 알아야 한다 |
| `PtySession::spawn_in(agent, …)` 이 이미 있다. `resolve_agent` 가 실행 파일을 찾고 codex 에는 `--no-alt-screen` 을 붙인다 | `pty.rs:43`, `pty.rs:340` | 검증 칸은 이걸로 **에이전트를 직접** 띄운다. 셸을 거치지 않는다 |
| 자식 환경변수는 `CommandBuilder::env` 로 넣는다. 지금은 `TERM`·`COLORTERM`·(셸이면) `PATH` 뿐이다 | `pty.rs:70-82` | `MAI_CTL`·`MAI_PANE`·`MAI_ROLE` 을 같은 자리에서 넣는다. 셸 → 에이전트 → 에이전트가 실행한 셸까지 상속된다 |
| 세션은 `Vec<Session>` 의 **인덱스**로만 구분한다. 칸을 닫으면 뒤 인덱스가 당겨진다 | `app.rs:290-310` `close_session` | 인덱스를 환경변수로 넘기면 칸을 닫는 순간 어긋난다. **고정 id** 를 새로 둔다 |
| 프롬프트 주입은 `pending` 에 걸어 두고 `tick()` 이 «출력 뒤 잠잠해짐 + 대화상자 없음 + 사용자 타이핑 없음» 일 때 넣는다 | `app.rs:372-404`, `model.rs:159` `ready_to_inject`, `modal.rs` | 검증 질문도 **같은 길**로 넣는다. 신뢰 대화상자가 떠 있으면 사람이 답할 때까지 기다리는 규칙이 그대로 적용된다 |
| 여러 줄 본문은 브래킷 붙여넣기(`ESC[200~ … ESC[201~`)로 감싸야 첫 줄바꿈에서 전송되지 않는다 | `relay.rs` `compose` | 질문 본문도 같은 방식으로 감싼다 |
| 마지막 답변은 1순위 세션 기록(JSONL), 2순위 화면 버퍼에서 꺼낸다. claude 는 `~/.claude/projects/<cwd>/` 의 **가장 최근 파일**, codex 는 `~/.codex/sessions/**` 중 첫 줄 cwd 가 맞는 **가장 최근 파일**. agy 는 화면뿐 | `relay.rs` `last_answer`, `claude_last`, `codex_last` | 답 회수에 재사용한다. 단, «가장 최근 파일» 이 요청자 것일 수 있다(같은 cwd 에 Claude 둘). **검증 칸이 뜬 뒤 생긴 파일**로 좁혀야 한다 |
| `modal::open` 의 `to interrupt` 마커는 «작업 중» 표시다 | `modal.rs:32` | 턴 종료 판정에 쓴다. 이 글자가 보이는 동안은 아직 답하는 중이다 |
| `scripts/mai` 는 **첫 인자가 `-` 로 시작하지 않으면 공간 경로로 본다** | `scripts/mai:38-41` | `mai ctl ask …` 를 치면 `cd ctl` 로 죽는다. 런처에 `ctl` 예외를 넣어야 한다 |
| 바이너리는 `--converge`·`--quote` 처럼 `--플래그` 로 하위 명령을 가른다 | `main.rs:67-220` | `ctl` 서브커맨드를 같은 match 에 붙인다 |
| Unix domain socket 은 Rust 표준 라이브러리(`std::os::unix::net`)로 된다 | — | **새 의존성 없음.** Windows 는 나중에 `windows-sys` 추가가 필요하다(별도 합의) |

---

## 2. 구성 요소

### 2.1 제어 통로 (`rust/src/ctl/`) — 새 모듈

```
ctl/
  mod.rs        요청·응답 타입, JSON 직렬화 (serde — 이미 의존성)
  server.rs     mai 쪽. 소켓을 열고 수신 스레드가 요청을 mpsc 로 앱에 넘긴다
  client.rs     `mai ctl ask` 쪽. 연결 → 요청 한 줄 → 응답 한 줄 대기
```

- **주소**: `$TMPDIR/mai-<pid>/ctl.sock`. 디렉터리는 `0700` 으로 만든다(기존
  `CodexShim` 과 같은 방식). 다른 사용자는 파일 권한으로 막히므로 **macOS 에서는
  인증값이 필요 없다.** 토큰은 Everyone ACL 을 써야 하는 Windows 몫이다.
- **환경변수** (칸을 띄울 때 자식에게 넣는다):
  - `MAI_CTL` — 소켓 경로
  - `MAI_PANE` — 칸의 고정 id
  - `MAI_ROLE` — `main` 또는 `verifier`
- **프로토콜**: 연결당 요청 한 줄, 응답 한 줄. JSON.
  ```json
  → {"op":"ask","pane":7,"role":"main","agent":"codex","question":"…","timeout_secs":null}
  ← {"ok":true,"answer":"…","origin":"세션 기록"}
  ← {"ok":false,"error":"검증자 칸에서는 다시 검증을 요청할 수 없다"}
  ```
  비공개 내부 규약이다. 버전 필드는 두지 않는다 — 클라이언트와 서버가 같은
  바이너리다.
- **수신 스레드**: `accept` 블로킹 → 요청 파싱 → `(요청, UnixStream)` 을
  `mpsc::Sender` 로 앱에 넘긴다. 응답 쓰기는 앱 스레드가 그 `UnixStream` 에 한다.
  클라이언트가 중간에 끊으면(Ctrl+C) 쓰기 실패로 알게 되고, 요청은 버린다.
- **정리**: `Drop` 에서 소켓 파일과 디렉터리를 지운다. 비정상 종료로 남은 소켓은
  다음 기동과 무관하다(pid 가 다르다).

### 2.2 칸 식별 (`model.rs`)

- `Session` 에 `id: u64` 추가. 앱이 들고 있는 단조 증가 카운터로 붙인다.
- `Session` 에 `verifier_of: Option<u64>` 추가. 검증 칸이면 요청자 칸의 id.
- `App` 에 `by_id(id) -> Option<usize>` 추가. 인덱스가 밀려도 id 로 찾는다.

### 2.3 서버 쪽 처리 (`app.rs` — `tick()` 에서 요청을 꺼낸다)

`tick()` 머리에서 `ctl` 수신 채널을 `try_recv` 로 비우고, 요청마다:

1. **요청자 확인** — `pane` 으로 세션을 찾는다. 없으면 오류 응답.
2. **재귀 차단** — `role == verifier` 면 오류 응답. (검증자가 또 검증을 부르면
   칸이 무한히 는다. 지시문에도 적지만 서버가 막는 것이 확실하다.)
3. **중복 흡수** — 같은 요청자의 요청이 이미 진행 중이고 대상 에이전트가 같으면
   새 연결을 그 요청의 **대기자 목록에 붙인다.** 답이 오면 전부에게 같은 답을 쓴다.
   Codex 는 긴 명령을 기다리다 같은 명령을 다시 실행하는 일이 있다(Codex 답변 §장시간
   대기). 대상이 다르면 오류 응답("이미 진행 중").
4. **검증 칸 확보** — 요청자 id + 에이전트로 살아 있는 검증 칸을 찾는다.
   있으면 재사용, 없으면 `PtySession::spawn_in(agent, …)` 으로 **요청자와 같은
   공간·같은 cwd** 에서 띄운다. `agent_id = agent`, `verifier_of = Some(요청자)`,
   제목은 `"{에이전트 표시명} ← {요청자 제목}"`. 환경변수는 `MAI_ROLE=verifier`.
   - 공간이 셋을 넘으면 기존 규칙대로 탭으로 바뀐다. 건드리지 않는다.
   - 포커스는 **옮기지 않는다.** 사용자는 요청자 칸을 보고 있을 수 있다.
5. **기준선 기록** — 주입 전에 `relay::last_answer` 로 지금 마지막 답을 떠 둔다.
   검증 칸을 새로 띄운 경우 기준선은 None 이고, **띄운 시각**을 적어 둔다.
6. **질문 주입** — `pending` 에 본문을 건다. 본문은
   ```
   [{요청자 제목} 의 검증 요청]
   """
   {question}
   """
   ```
   를 브래킷 붙여넣기로 감싼 것. 기존 주입 규칙(대화상자 대기 등)이 그대로 적용된다.
7. **대기 등록** — `App.asks: Vec<Ask>` 에 넣는다.
   ```rust
   struct Ask {
       requester: u64, verifier: u64, agent: String,
       baseline: Option<String>,        // 주입 전 마지막 답
       spawned_at: Option<SystemTime>,  // 새로 띄웠으면 그 시각
       injected_at: Option<Instant>,    // pending 이 실제로 들어간 시각
       deadline: Option<Instant>,
       waiters: Vec<UnixStream>,
   }
   ```

### 2.4 턴 종료 판정과 답 회수 (`app.rs` + `relay.rs`)

매 `tick()` 마다(500ms 에 한 번만) 진행 중인 `Ask` 를 본다.

- **아직 주입 전**(`pending` 이 남아 있음) → 기다린다.
- **주입됨** → 다음 셋이 **모두** 참이면 답이 끝난 것으로 본다.
  1. 검증 칸에 **2.5초 이상 새 출력이 없다** (`seen.1.elapsed()`). 에이전트는
     작업 중에 스피너·경과 시간을 계속 그리므로 출력이 멈추면 끝난 것이다.
  2. 화면에 `to interrupt` 가 없다 (`modal::open` 의 작업 중 마커).
  3. `relay::last_answer_since(agent, cwd, spawned_at)` 가 **기준선과 다른** 답을
     돌려준다. 새 함수다 — 후보 파일을 `spawned_at` 이후에 **생성된** 것으로
     좁혀 같은 cwd 의 요청자 기록을 집지 않게 한다. 재사용 칸은 `spawned_at` 을
     처음 띄운 시각으로 유지한다.
  - agy 는 기록 위치가 없다. 1·2 만으로 판정하고 화면 버퍼(`from_screen`)를
    답으로 준다. 응답의 `origin` 에 `화면` 이라고 적어 요청자가 알게 한다.
- **실패 조건** → 오류 응답하고 `Ask` 를 지운다.
  - 검증 칸이 끝났거나 닫혔다 (`finished()` / 세션 없음).
  - `deadline` 이 지났다 (클라이언트가 `--timeout` 을 줬을 때만).
- 답이 나오면 모든 `waiters` 에 응답을 쓰고 닫는다. **검증 칸은 닫지 않는다.**
  사용자가 과정을 볼 수 있어야 하고, 다음 요청에 재사용한다.
- 임계값(2.5초)은 상수로 두고 실측 뒤 조정한다.

### 2.5 클라이언트 (`main.rs` `Some("ctl")` 분기 + `ctl/client.rs`)

```
mai ctl ask <에이전트> "<질문>"          질문을 인자로
mai ctl ask <에이전트> -                 질문을 stdin 으로 (긴 질문)
mai ctl ask --timeout 600 <에이전트> …  초 단위 상한 (기본 무제한)
mai ctl instructions                    에이전트에 넣을 지시문을 출력
```

- `MAI_CTL`·`MAI_PANE` 이 없으면 stderr 에 «mai 안에서 실행해야 한다» 를 적고
  **종료 코드 2.** 서버 오류 응답은 메시지를 stderr 에 적고 **종료 코드 1.**
  성공은 답만 stdout 에 쓰고 **0.** 에이전트는 종료 코드와 stdout 만 본다.
- 에이전트 이름은 `claude`·`codex`·`agy` 와 표시명(`Claude`·`Gemini`)을 받는다.
  `main.rs` 의 `AGENTS` 표로 정규화한다.
- 블로킹 읽기. 몇 분이 걸려도 그대로 기다린다(Codex 쪽 125초 대기 정상 확인).

### 2.6 런처 (`scripts/mai`)

첫 인자가 `ctl` 이면 공간 경로로 보지 않고 그대로 넘긴다. 빌드 최신성 검사는
그대로 둔다 — 소스가 바뀐 상태에서 AI 가 `mai ctl` 을 부르면 빌드가 먼저 도는데,
그 편이 옛 바이너리로 엉뚱하게 도는 것보다 낫다.

### 2.7 지시문 (`AGENTS.md`, `CLAUDE.md` 추가분, `mai ctl instructions`)

에이전트가 «답을 낸 뒤 스스로 검증을 요청» 하게 만드는 글. 세 CLI 가 읽는 파일이
다르다 — Codex 는 `AGENTS.md`, Claude 는 `CLAUDE.md`, Gemini 는 `GEMINI.md`.
같은 본문을 한 곳(`prompts/verify.md`)에 두고 각 파일에서 참조하거나 복사한다.
`mai ctl instructions` 는 그 본문을 출력해 다른 프로젝트에 붙이기 쉽게 한다.

본문 요지(Codex 답변 §상시 검증 지시를 다듬은 것):

- 답변 초안이나 구현이 준비되면 최종 응답 전에 `mai ctl ask <다른 에이전트> "<검증할 내용>"` 을 실행한다.
- 명령이 끝나기 전에 같은 명령을 다시 실행하지 않는다. 몇 분 걸릴 수 있다.
- 돌아온 검증 결과를 검토해 필요한 수정을 반영하고, 이견이 남으면 사용자에게 **결정 필요**로 적는다.
- 종료 코드가 0 이 아니면 «검증하지 못했다» 고 명시한다. 추측으로 메우지 않는다.
- `MAI_ROLE=verifier` 로 떠 있으면 자신은 검증자다. 검증 요청을 보내지 않는다.

이 저장소 자체를 첫 실험대로 삼아 루트 `AGENTS.md` 를 만든다.

---

## 3. 바꾸는 파일

| 파일 | 변경 |
|---|---|
| `rust/src/ctl/mod.rs` **(신규)** | `Request`·`Response` 타입, JSON 한 줄 읽기/쓰기 |
| `rust/src/ctl/server.rs` **(신규, `cfg(unix)`)** | 소켓 생성·수신 스레드·정리 |
| `rust/src/ctl/client.rs` **(신규, `cfg(unix)`)** | 연결·요청·응답 대기 |
| `rust/src/main.rs` | `mod ctl;` · `Some("ctl")` 분기 · 사용법 주석 한 줄 |
| `rust/src/model.rs` | `Session.id`, `Session.verifier_of` |
| `rust/src/pty.rs` | `spawn_exe` 에 환경변수 인자(`&[(&str, OsString)]`) 추가. `spawn_in`·`spawn_shell_in` 시그니처에 전달 |
| `rust/src/app.rs` | `ctl` 서버 보유 · id 카운터 · `by_id` · `tick()` 에서 요청 처리와 답 회수 · `spawn_verifier` · `spawn_terminal` 에 환경변수 전달 |
| `rust/src/relay.rs` | `last_answer_since(agent, cwd, since)` — 생성 시각으로 후보를 좁힌 변형. 기존 `last_answer` 는 그대로 |
| `scripts/mai` | `ctl` 서브커맨드 통과 |
| `AGENTS.md` **(신규)** · `prompts/verify.md` **(신규)** | 지시문 |
| `README.md` | 기능 한 절, 키 표는 손대지 않음 |

Windows 쪽 `cfg(windows)` 는 이번엔 «지원하지 않는다» 는 오류만 돌려준다.
서버·클라이언트가 별도 파일이라 named pipe 구현은 파일을 추가하는 것으로 끝난다.

---

## 4. 구현 순서와 확인 기준

| 순서 | 단계 | 확인 |
|---|---|---|
| 1 | `ctl/mod.rs` 프로토콜 + 단위 테스트 | 요청/응답 JSON 왕복, 깨진 입력은 오류 |
| 2 | `model.rs` id · `pty.rs` 환경변수 · `scripts/mai` | 기존 테스트 그대로 통과. `mai ctl` 이 런처를 지나 바이너리에 닿는다 |
| 3 | `ctl/server.rs` + `App` 에 보유 · 자식에 `MAI_CTL` 전달 | 칸의 셸에서 `echo $MAI_CTL $MAI_PANE $MAI_ROLE` 이 찍힌다 |
| 4 | `ctl/client.rs` + `main.rs` 분기 | mai 밖에서 `mai ctl ask codex x` → 종료 코드 2 와 안내 |
| 5 | `tick()` 요청 처리 · 검증 칸 띄우기 · 질문 주입 | 셸 칸에서 `mai ctl ask codex "1+1은?"` 을 치면 옆에 Codex 칸이 뜨고 질문이 들어간다 |
| 6 | 답 회수 · `relay::last_answer_since` · 응답 | 위 명령이 Codex 의 답을 stdout 에 찍고 0 으로 끝난다. 두 번째 요청은 같은 칸을 재사용한다 |
| 7 | 재귀 차단 · 중복 흡수 · 시간 초과 · 칸 닫힘 | 검증 칸 안에서 `mai ctl ask` → 오류 1. 같은 요청 두 번 → 둘 다 같은 답. `--timeout 5` → 오류 |
| 8 | 지시문 · README | Claude 칸에 «이 답을 codex 에게 검증받아» 라고 하면 스스로 명령을 실행한다 |
| 9 | Codex 샌드박스에서 실제 실행 | **Codex 칸이 요청자**일 때 되는지. macOS Seatbelt 가 Unix socket 을 막으면 execpolicy `prefix_rule` 폴백으로 |

빌드·테스트는 내가 돌리지 않는다(전역 규칙). 각 단계의 확인은 macOS 에서
사용자가 실행한다. 5·6 은 셸 칸에서 **사람이 직접** `mai ctl ask` 를 치는 것으로
먼저 확인하고, 그다음 AI 가 치게 한다 — 통로 문제와 지시문 문제를 분리하기 위해서다.

---

## 5. 이번에 하지 않는 것

- **스타일** — 에이전트별 색, 상태 기호, 사이드바 들여쓰기. 메모리의 결정대로 기능 검증 뒤.
- **Windows** — named pipe + Everyone ACL + 세션 토큰. `windows-sys` 직접 의존성 추가는 그때 합의한다.
- **헤드리스 폴백** — mai 밖에서 `codex exec` 등으로 대신 답하는 것. 지금은 종료 코드 2 로 끝낸다.
- **답 회수의 요약·가공** — 답은 그대로 넘긴다. 상한은 `relay::MAX_CHARS`(2만 자, 뒤를 남김)를 그대로 쓴다.

---

## 6. 알려진 위험

| 위험 | 대응 |
|---|---|
| 2.5초 «잠잠함» 이 도구 실행 중의 일시 정지와 겹친다 | 3번 조건(기록에 새 답이 있음)이 함께 걸린다. 그래도 중간 텍스트를 답으로 집을 수 있어, 실측 뒤 임계값을 조정한다 |
| JSONL 은 비공개 규약이라 CLI 가 올라가면 깨진다 | 기존 relay 와 같은 태도 — 조용히 화면 버퍼로 내려가고 `origin` 에 표시한다 |
| 검증 칸이 신뢰·MCP 대화상자에서 멈춘다 | 주입이 미뤄지고 요청자는 기다린다. 사용자가 상자에 답하면 이어진다. 상태줄에 «검증 칸이 사용자 확인을 기다린다» 를 적는다 |
| 같은 cwd 에 같은 에이전트가 둘이라 기록 파일이 섞인다 | `last_answer_since` 가 검증 칸 생성 이후 파일만 본다 |
| Codex(요청자)의 macOS 샌드박스가 소켓 연결을 막는다 | 미검증. 9단계에서 실측. 막히면 execpolicy `prefix_rule(pattern=["mai","ctl","ask"])` 로 그 명령만 허용 |
| 요청자가 답을 기다리는 동안 사용자가 요청자 칸을 닫는다 | `waiters` 쓰기가 실패하면 조용히 버린다. 검증 칸은 남는다 |

---

## 7. 결정이 필요한 것

아래는 내 쪽 기본값이다. 다르게 하고 싶으면 알려 달라.

1. 검증 칸은 답을 돌려준 뒤에도 **열어 둔다** (재사용·과정 확인).
2. 검증자가 다시 검증을 요청하는 것은 **서버가 거부한다** (깊이 1 고정).
3. 잠잠함 임계값 **2.5초**, 클라이언트 기본 **시간 제한 없음**.
4. 검증 칸 제목 형식 `"Codex ← Claude"`. 스타일 단계에서 바뀔 수 있다.
