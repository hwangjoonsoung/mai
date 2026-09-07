# BACKGROUND — 기존 솔루션 조사 및 선택 근거

> 이 문서는 **왜 다른 방식을 쓰지 않았는가**를 기록한다. 구현 기준이 아니다.
> 구현 스펙은 `SPEC.md`, 목적과 금지사항은 `INTENT.md`에 있다.
>
> 작성: codex (§2-C·§3·부록 B) · agy (§2-B 주석) · Claude (정리)
> 최종 갱신: 2026-09-03

## 읽는 목적

`INTENT.md` §4의 Non-goals(N1~N8)가 **왜** 폐기됐는지 근거가 필요할 때만 읽는다. 여기 실린 솔루션은 전부 검토를 마쳤고, 채택하지 않기로 결정됐다.

| 절 | 내용 |
|---|---|
| §2-A | PAL MCP Server(`clink`) — 요구사항 최근접이었으나 미채택 |
| §2-B | 순정 MCP 서버 모드 직결 + 스킬 래퍼 — 초기 권장안이었으나 `SPEC.md` §4.2에서 폐기 |
| §2-C | Maestro 등 독립 오케스트레이터 — 발화자 식별 불가로 탈락 |
| §2-D | API 키 과금 기반 프레임워크 — **이중 과금으로 전면 탈락** |
| §3 | 요구사항 대조표 |
| 부록 B | 3자 vs 2자 구도 선택 근거 |
| 부록 C | 참고 자료 링크 |

절 번호는 원본 문서의 상호참조를 보존하기 위해 그대로 유지한다.

---

## 2. 기존 솔루션 조사

### A. PAL MCP Server (구 zen-mcp-server) — `clink` 툴 ★ 요구사항 최근접

**저장소**: https://github.com/BeehiveInnovations/pal-mcp-server

"Provider Abstraction Layer". 원하는 구조를 거의 그대로 구현해 둔 프로젝트입니다.

#### 핵심 기능 — `clink` (CLI + Link)

- Claude Code 세션 안에서 **실제 다른 CLI 바이너리를 서브에이전트로 spawn**
- **API 키가 아니라 각 CLI에 미리 로그인해둔 구독 인증을 그대로 사용** (문서상 "Authenticate CLIs beforehand")
- **컨텍스트 격리** — 서브 CLI는 독립 컨텍스트에서 실행되고 결과만 반환. 메인 세션 컨텍스트 오염 없음
- **역할(role) 프리셋** — `planner`, `codereviewer`, `default` + 커스텀 역할 정의 가능

#### `clink` 파라미터

| 파라미터 | 설명 |
|---|---|
| `prompt` | 작업 내용 (필수) |
| `cli_name` | 호출할 CLI (gemini / claude / codex / 커스텀) |
| `role` | `planner` / `codereviewer` / `default` |
| `files`, `images` | 컨텍스트 파일 참조 |
| `continuation_id` | 이전 대화 재개 |

#### 기본 제공 툴

**기본 활성화**: `chat`(브레인스토밍), `planner`(단계 분해), `consensus`(다중 모델 토론 합의), `codereview`(심각도별 리뷰), `precommit`(커밋 전 검증), `debug`(근본 원인 분석), `thinkdeep`(확장 추론)

**기본 비활성화**: `analyze`, `refactor`, `testgen`, `secaudit`, `docgen`, `tracer`

> `consensus` 툴이 **요구사항 2번**(계획 교차검토 → 의견 수렴)에 정확히 대응합니다. 여러 모델에 동일 안건을 던져 구조화된 찬반 토론 후 합의를 도출하고, 그 전체 컨텍스트를 다음 CLI에 그대로 넘길 수 있습니다.

#### 주의점

1. **권한 우회 플래그로 실행됨** — Gemini `--yolo`, Codex `--dangerously-bypass-approvals-and-sandbox`, Claude `--permission-mode acceptEdits`. 신뢰 워크스페이스에서만 사용하거나 플래그를 직접 제거해야 함
2. **지원 CLI가 구 `gemini` 기준** — `agy`로 교체하려면 커스텀 CLI 등록 작업 필요
3. Python MCP 서버를 별도 프로세스로 띄워야 함 (`./run-server.sh`)
4. `.env`의 API 키를 요구하는 부분이 있음 (외부 모델 직접 호출 툴용). `clink`만 쓴다면 불필요

---

### B. 순정 조합 — MCP 서버 모드 직결 (외부 라이브러리 0개)

외부 프로젝트 없이 Claude Code 순정 기능만으로 구성 가능합니다.

#### B-1. Codex → MCP 서버로 직결 (설정 한 줄)

`codex --help` 실측 확인 결과:

```
mcp-server    Start Codex as an MCP server (stdio)
```

즉 다음 한 줄로 Codex가 Claude Code의 MCP 툴로 등록됩니다.

```bash
claude mcp add codex -- codex mcp-server
```

호출 시 트랜스크립트에 `mcp__codex__*` 형태로 표시되므로 **요구사항 4번(서브에이전트 직관적 식별)이 자동 충족**됩니다.

관련 서브커맨드 구분:

- `codex mcp` — 외부 MCP 서버 **관리** (list / get / add / remove / login / logout)
- `codex mcp-server` — Codex **자신을** MCP 서버로 기동 ← 필요한 것은 이쪽
- `codex app-server`, `codex exec-server` — 실험적 서버 모드

#### B-2. agy → MCP 브리지 (서드파티)

`agy`는 자체 MCP 서버 모드가 없어 브리지가 필요합니다. 이미 여러 구현체가 존재합니다.

| 프로젝트 | 특징 |
|---|---|
| [TurkerYakup/mcp-server-google-antigravity](https://github.com/TurkerYakup/mcp-server-google-antigravity) | 무제한 비동기 잡, 실시간 진행률, 파일시스템 툴, 모델/에이전트/샌드박스 제어. 크로스플랫폼 |
| [SinanTufekci/Claude-Code-Antigravity-CLI-MCP-Server](https://github.com/SinanTufekci/Claude-Code-Antigravity-CLI-MCP-Server) | agy를 Claude Code 서브에이전트로 노출. agy 트랜스크립트 파일을 읽어 헤드리스 stdout 이슈 우회 |
| [a3lab01create-bit/antigravity-mcp-server](https://github.com/a3lab01create-bit/antigravity-mcp-server) | 범용 MCP 클라이언트용 agy 브리지 |
| Agy Headless Bridge | 비-TTY 환경에서의 안정적 헤드리스 실행. CI·Python 스크립트 연동 |

> 참고: 일부 브리지가 우회하려는 "헤드리스 stdout 버그"는 **본 환경(agy 1.1.25 실측)에서도 전혀 재현되지 않았습니다.** `agy -p`가 정상적으로 stdout에 응답(JSON/텍스트)을 즉시 출력하므로, 서드파티 MCP 브리지 없이 Java ProcessBuilder 직접 호출로 안정적으로 동작합니다.
>
> #agy

#### B-3. 스킬(Skill) 래퍼 — 이미 검증된 패턴 보유

현재 환경에 `~/.claude/skills/codex/SKILL.md` (gstack 플러그인)가 이미 설치되어 있고, **정확히 이 방식으로 동작합니다.**

실제 사용 중인 호출 패턴:

```bash
# 신규 세션 (JSONL 스트리밍으로 추론 과정·툴 호출 캡처)
codex exec "<prompt>" -C "$_REPO_ROOT" -s read-only \
  -c 'model_reasoning_effort="high"' --enable web_search_cached --json

# 세션 재개 (연속성 유지)
codex exec resume <session-id> "<prompt>" -C "$_REPO_ROOT" -s read-only \
  -c 'model_reasoning_effort="medium"' --enable web_search_cached --json

# 코드 리뷰 전용 모드
codex review "<instructions>" --base <base-branch> \
  -c 'model_reasoning_effort="high"' --enable web_search_cached
```

타임아웃 래퍼(`_gstack_codex_timeout_wrapper`), 행(hang) 감지, JSONL 파싱까지 구현되어 있습니다.

**→ 이 패턴을 `agy -p`용으로 한 벌 더 만들면 3자 구도가 완성됩니다.**

---

### C. Claude Code 밖의 독립 오케스트레이터

| 프로젝트 | 내용 | 한계 |
|---|---|---|
| [josstei/maestro-orchestrate](https://github.com/josstei/maestro-orchestrate) | Gemini CLI / Claude Code / Codex / Qwen Code 대상 멀티에이전트 플랫폼. 39개 전문 에이전트, 단순 작업용 Express 경로 + 중·고난도용 4단계 표준 워크플로우. 코드리뷰·디버깅·보안·SEO·접근성·컴플라이언스 툴 내장. 단일 `src/` 트리에서 운용 | **Claude Code 안이 아닌 별도 환경.** 요구사항 4번(직관적 표시) 미충족 |
| Parallel Code (johannesjo) | Claude Code / Codex CLI / Gemini CLI를 **git worktree로 격리해 병렬 실행**. 각 에이전트가 독립 브랜치·작업 디렉터리를 가지며 완료 후 머지. 커뮤니티 용어 "agentmaxxing" | 동일 채팅방의 공동 문맥과 참여자별 대화를 제공하는 구조는 아님 |

---

### D. 요구사항에 맞지 않는 것 (참고)

- **zen-mcp-server 본체 / LangGraph / CrewAI / MetaGPT / AutoGen**
  - 전부 **API 키 과금 기반**. 구독료를 이미 지불 중인데 API 요금을 이중 지불하게 됨
  - zen-mcp-server는 `GEMINI_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY` 등을 환경변수로 요구
  - **유일한 예외가 위 A·B의 "CLI 바이너리를 그대로 실행" 방식**
- **gemini-mcp-tool (jamubc) 등 구 Gemini CLI 래퍼**
  - Gemini CLI 종료로 사실상 무력화. 일부는 `agy` 백엔드 자동 선택으로 대응 중이나 과도기

---

## 3. 요구사항 대조표

| 요구사항 | A. PAL MCP `clink` | B. 순정 MCP + 스킬 | C. Maestro |
|---|:---:|:---:|:---:|
| 1. 하나의 환경에서 여러 AI 사용 | O | O | O |
| 2. 특정 AI 지목 | O (`cli_name`) | O (MCP 툴명 / 슬래시커맨드) | O |
| 3. 역할을 프롬프트로 자유롭게 지정 | O | O | △ (표준 워크플로우 중심) |
| 4. 참여자 직관적 표시 | O (MCP 툴 호출로 표시) | O (`mcp__codex__*`) | X (별도 UI) |
| Claude Code 내부 동작 | O | O | X |
| `agy`(신 Gemini) 대응 | △ (커스텀 등록 필요) | O (브리지/스킬) | △ (구 gemini 기준) |
| 구독 인증만으로 무과금 | O | O | O |
| 도입 난이도 | 중 (Python 서버 상주) | 하 (설정 + 스킬) | 상 (별도 환경) |

---

## 부록 B. 초기 구도 선택 검토

> **결론: 3자 구도로 확정.** → `SPEC.md` §7.0 `D4`(참여자) 및 `D7`(Gemini 진입점)
> 아래는 판단 근거이며, 뒤집고 싶으면 `ARCHIVE.md` 부록 E 방법 4를 참고하십시오.

**`agy`를 Gemini 자리로 계속 사용할지**

| 선택지 | 장점 | 단점 |
|---|---|---|
| **3자 구도** (Claude + Codex + agy) | 다각도 검증. agy가 Gemini 3.1 Pro / Claude Opus 4.6 / GPT-OSS 라우팅 지원 | 클로즈드소스 전환. MCP 브리지가 서드파티 수준. 도구 생태계 과도기 |
| **2자 구도** (Claude + Codex) | 안정적. 둘 다 순정 MCP·헤드리스 지원 성숙 | 관점 다양성 감소. Gemini 구독 미활용 |

> 실측상 `agy -p`는 문제없이 동작했고, 별도 브리지 없이 셸 호출만으로 충분해 보입니다.
> 3자 구도를 권장하되, `agy` 연동은 MCP 브리지 대신 **스킬 + 셸 호출** 방식이 안전합니다.

---

## 부록 C. 참고 자료

### 공식 발표

- [Google Developers Blog — An important update: Transitioning Gemini CLI to Antigravity CLI](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/)
- [google-gemini/gemini-cli Discussion #27274](https://github.com/google-gemini/gemini-cli/discussions/27274)
- [The Register — Bye-bye, Gemini CLI](https://www.theregister.com/ai-ml/2026/05/20/bye-bye-gemini-cli-google-nudges-devs-toward-antigravity/5243605)

### 오케스트레이션 도구

- [BeehiveInnovations/pal-mcp-server](https://github.com/BeehiveInnovations/pal-mcp-server) — 구 zen-mcp-server
- [pal-mcp-server / docs/tools/clink.md](https://github.com/BeehiveInnovations/pal-mcp-server/blob/main/docs/tools/clink.md)
- [josstei/maestro-orchestrate](https://github.com/josstei/maestro-orchestrate)

### Antigravity MCP 브리지

- [TurkerYakup/mcp-server-google-antigravity](https://github.com/TurkerYakup/mcp-server-google-antigravity)
- [SinanTufekci/Claude-Code-Antigravity-CLI-MCP-Server](https://github.com/SinanTufekci/Claude-Code-Antigravity-CLI-MCP-Server)
- [a3lab01create-bit/antigravity-mcp-server](https://github.com/a3lab01create-bit/antigravity-mcp-server)

### 패턴·비교 자료

- [Agentmaxxing: Parallel Multi-CLI Orchestration with Codex CLI, Claude Code and Gemini CLI](https://codex.danielvaughan.com/2026/04/11/agentmaxxing-parallel-multi-cli-orchestration/)
- [Antigravity CLI (agy): Commands, Modes, and Auto-Approve](https://www.aibuilderclub.com/blog/antigravity-cli-guide)
- [Antigravity CLI: install agy, commands, models, flags](https://continuumcode.ai/guides/antigravity-cli/)

---

