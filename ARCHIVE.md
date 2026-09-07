# ARCHIVE — 폐기된 초기안 (실행 금지 · 동결)

> # ⚠ 이 문서는 실행 기준이 아니다
>
> 여기 실린 모든 지시·명령·체크리스트는 **2026-09-03에 폐기됐다.**
> 현행 기준은 `INTENT.md` 와 `SPEC.md` 다. 역사적 비교를 위해서만 보존한다.
>
> **이 문서는 동결(frozen)이다. 갱신하지 않는다.**
> **검토자·구현자에게 제공하지 않는다.**

## 왜 위험한가

폐기안임에도 **복붙 가능한 셸 명령, 완성된 JSON 스키마, DoD 체크박스**를 갖추고 있다. 현행 `SPEC.md` §7이 인터페이스와 원칙 위주로 추상적인 반면, 이쪽은 훨씬 실행 가능해 보인다. "그래서 뭘 하면 되지"를 찾는 사람이 여기에 걸리기 쉽다.

**이 함정은 이미 한 번 발동했다.** codex가 `SPEC.md` §6.3에 "Bash 전용 `< /dev/null`은 사용하지 않는다"고 써놓고, 같은 문서의 이 부록에는 "`< /dev/null` **필수**"를 그대로 남겼다. 문서를 쓴 당사자조차 두 구간을 동시에 붙들지 못했다.

## 현행 기준과 충돌하는 지점

| 이 문서의 내용 | 현행 기준 |
|---|---|
| `~/.claude/skills/` 에 스킬 4종 설치 | **폐기.** `multi_ai_cli`가 직접 호스트다 (`INTENT.md` N1) |
| 셸 호출 시 `< /dev/null` **필수** | **금지.** Java `ProcessBuilder`로 stdin을 닫는다 (`SPEC.md` §6.3) |
| `agy -c` 로 세션 재개 | **배제.** Stateless prompt packing이 정식이다 (`SPEC.md` §5.3, §8.4 K3) |
| 결정 ID `LD1`~`LD8` | `SPEC.md` §7.0의 `D1`~`D13`과 **다른 체계다** |
| 절 번호 `D.0`~`D.7` | `SPEC.md` §7과 번호가 겹치던 것을 분리한 결과다 |

## 남겨둔 이유

`tri-review` 파이프라인(D.5)의 **아이디어 자체는 살아 있다.** 합의/이견/단독 지적/미해결 분류, 동일 JSON 스키마 강제, 최대 2라운드 반론 구조는 `SPEC.md` §7.9 **Phase 3(구조화 수렴)** 으로 이어졌다. 제품 형태만 스킬에서 독립 애플리케이션으로 바뀌었을 뿐 목적은 그대로다(`INTENT.md` §2).

Phase 3을 구현할 때 D.1.2의 응답 스키마와 D.5.2의 라운드 진행 절차를 **설계 참고 자료로** 볼 수 있다. 그때도 셸 호출 방식과 산출물 경로는 현행 기준을 따른다.

---

## 부록 A. 초기 결론 및 권장안 — 개정 전 조사 기록

> 아래 내용은 Claude Code 내부 스킬을 최종 산출물로 보던 초기안이다. 역사적 비교 자료로만 유지하며 실제 구현 기준은 §4~8이다.
>
> #codex작성

### 결론: Claude Code 안에서 구현 가능합니다.

**권장: B(순정) 기반 + A에서 설계 아이디어 차용**

#### 선정 근거

1. **`codex mcp-server`가 순정으로 존재** → Codex 연결은 설정 한 줄로 끝
2. **이미 `gstack/codex` 스킬로 동일 패턴을 검증된 형태로 사용 중** → 학습 비용 0
3. PAL MCP `clink`는 강력하지만
   - 구 `gemini` 기준이라 `agy` 커스텀 등록 필요
   - 기본 권한 우회 플래그가 위험
   - Python 서버 상주 필요
4. **요구사항 2번(수렴 프로세스)은 어차피 개인 워크플로우 맞춤 설계가 필요.** PAL의 `consensus`도 그대로는 맞지 않을 가능성이 큼

### 구축할 구성요소

```
~/.claude/skills/
  ├─ ask-codex/     codex exec 래퍼
  │                 - resume 기반 세션 연속성
  │                 - -s read-only / -C <repo-root>
  │                 - --json JSONL 스트리밍 파싱
  │                 - 타임아웃 + hang 감지
  │
  ├─ ask-agy/       agy -p 래퍼
  │                 - --add-dir 로 컨텍스트 주입
  │                 - --model 로 모델 선택 (Gemini 3.1 Pro / Claude Opus 4.6 등)
  │                 - --print-timeout 조정
  │                 - --conversation 으로 대화 재개
  │
  └─ tri-review/    3자 교차검증 파이프라인  ← 핵심
                    1) 대상 계획서/디프 수집
                    2) codex + agy 에 동시 투척 (병렬)
                    3) 의견 수집 및 정규화
                    4) 쟁점 표로 정리
                    5) 합의 / 이견 / 미해결 3분류
                    6) 미해결 항목만 사용자에게 결정 요청

~/.claude/settings.json
  └─ mcpServers:
       "codex": { "command": "codex", "args": ["mcp-server"] }
```

### 요구사항 충족 매핑

| 요구사항 | 담당 구성요소 |
|---|---|
| 1. 역할 분담 | Claude Code 메인 세션(계획) → `ask-codex`(구현) → `tri-review`(검증) |
| 2. 교차검토·수렴 | `/tri-review` — 계획서를 codex·agy에 동시 투척 후 쟁점 수렴 |
| 3. 특정 AI 지목 | `/ask-codex`, `/ask-agy` 슬래시 커맨드 |
| 4. 직관적 표시 | MCP 툴명(`mcp__codex__*`) + 스킬명이 트랜스크립트에 그대로 노출 |

---

## 부록 D. 초기 Claude Code 스킬 구현안 (개정 전)

> 이 부록은 개정 전 기록이며 실행 기준이 아니다. 독립 Java 채팅방 구현에는 `SPEC.md` §7을 적용한다.
>
> #codex작성
>
> **⚠ 이 부록의 지시를 그대로 실행하지 말 것 (#claude정리)**
> 1. 절 번호는 `D.0`~`D.7`, 결정 ID는 `LD1`~`LD8`이다. `SPEC.md` §7.0의 `D1`~`D13`과 다른 체계다.
> 2. 셸 호출 예시의 `< /dev/null`은 **`SPEC.md` §6.3에서 금지**한다. Java `ProcessBuilder`로 stdin을 닫는 방식이 정식 규칙이다.
> 3. `D.3`의 `agy -c` 세션 재개 방식은 **`SPEC.md` §5.3·§8.4 K3에서 배제**됐다. Stateless prompt packing이 정식이다.
> 4. `D.7` 완료 기준(`~/.claude/skills/` 스킬 4종)은 `SPEC.md` §4.2에서 폐기됐다. 전역 Claude 설정을 수정하지 않는다.

### D.0. 확정 결정사항 (Locked)

| # | 항목 | 결정 | 근거 |
|:---:|---|---|---|
| LD1 | 구도 | **3자** — Claude(사회자·계획) + Codex(구현·검증) + agy(검증) | Gemini 구독 활용. agy 실측 정상 동작 |
| LD2 | Gemini 진입점 | **`agy`만 사용.** 구 `gemini` CLI는 전면 배제 | `IneligibleTierError`로 사용 불가 (§1.2) |
| LD3 | 연동 방식 | **스킬 + 셸 호출.** MCP 등록 안 함 | 검증된 패턴 보유. MCP는 툴셋 중복 + 컨텍스트 소모 |
| LD4 | 스킬 위치 | `~/.claude/skills/` (전역) | 프로젝트 무관하게 사용 |
| LD5 | 산출물 위치 | **저장소 바깥** — `~/.tri-review/` | 대상 프로젝트 다수가 SVN. `svn status` 오염 방지 |
| LD6 | 상담 시 권한 | `codex -s read-only` 고정 | 의견 요청이 코드를 건드리면 안 됨 |
| LD7 | 구현 시 권한 | `codex -s workspace-write` | §1.6 실측값 |
| LD8 | 의견 교환 형식 | **JSON 스키마 강제** (§D.1.2) | 자연어 파싱 제거 |

### D.1. 공통 규약

#### D.1.1. 경로

```
~/.tri-review/
  <repo-slug>/                     # 저장소명. 저장소 밖이면 "_adhoc"
    <yyyymmdd-HHMM>-<slug>/
      subject.md                   # 검토 대상 (프롬프트 팩)
      schema.json                  # 응답 스키마
      codex.json                   # Codex 응답
      agy.json                     # agy 응답
      codex-r2.json / agy-r2.json  # 2라운드 (발생 시)
      REPORT.md                    # 최종 수렴 보고서
```

`<slug>` — 대상 파일명 또는 안건에서 생성한 kebab-case 짧은 식별자.

#### D.1.2. 응답 스키마 (`schema.json`)

Codex·agy 양쪽에 **동일하게** 강제한다.

```json
{
  "type": "object",
  "required": ["verdict", "summary", "issues", "open_questions"],
  "properties": {
    "verdict": {
      "type": "string",
      "enum": ["AGREE", "CONCERNS", "BLOCK"]
    },
    "summary": { "type": "string" },
    "issues": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["id", "severity", "claim", "rationale", "suggestion"],
        "properties": {
          "id":         { "type": "string" },
          "severity":   { "type": "string", "enum": ["critical", "major", "minor"] },
          "claim":      { "type": "string" },
          "rationale":  { "type": "string" },
          "suggestion": { "type": "string" }
        }
      }
    },
    "open_questions": { "type": "array", "items": { "type": "string" } }
  }
}
```

| verdict | 의미 |
|---|---|
| `AGREE` | 계획대로 진행 가능 |
| `CONCERNS` | 진행 가능하나 보완 필요 |
| `BLOCK` | 이대로 진행하면 안 됨 |

#### D.1.3. 타임아웃·실패 처리

| 상황 | 처리 |
|---|---|
| 기본 타임아웃 | Codex 600초 / agy `--print-timeout 10m` |
| 한쪽 실패·타임아웃 | **진행한다.** `REPORT.md` 머리말에 `PARTIAL — <실패한 CLI> 응답 없음` 명시 |
| 양쪽 실패 | **중단.** 원인(인증 만료 / 쿼터 / 네트워크)을 보고하고 종료 |
| JSON 파싱 실패 | 해당 CLI 1회 재시도. 재실패 시 위 "한쪽 실패"와 동일 처리 |
| 응답이 스키마 위반 | 파싱 실패와 동일 처리 |

#### D.1.4. 금지사항

- 상담·검증 단계에서 **어떤 CLI에도 쓰기 권한을 주지 않는다.**
- `--dangerously-bypass-approvals-and-sandbox`(codex), `--dangerously-skip-permissions`(agy), `--yolo` **사용 금지.**
- 대상 저장소에 파일을 생성하지 않는다 (LD5). `--in-repo` 를 명시한 경우만 예외.
- 사용자 지시 없이 커밋·푸시하지 않는다. SVN 프로젝트 포함.

---

### D.2. `S1` — `/ask-codex`

**목적**: Codex에게 단발 질의. 세션 연속성 지원.

**호출**

```bash
codex exec \
  --skip-git-repo-check \
  -C "$REPO_ROOT" \
  -s read-only \
  -c 'model_reasoning_effort="high"' \
  -o "$OUT/codex-last.md" \
  "$PROMPT" < /dev/null
```

**규칙**

- `-C` 는 저장소 루트. 저장소 밖이면 현재 디렉터리
- `< /dev/null` **필수** — 없으면 stdin 대기로 행(hang)
- 후속 질문은 `codex exec resume --last "<후속>"` 로 세션 유지
- 결과는 `-o` 파일에서 읽어 사용자에게 그대로 제시. 요약하지 말 것

**주의**: 저장소 밖에서 실행 시 `--skip-git-repo-check` 없으면
`Not inside a trusted directory` 오류 (§1.1 실측).

---

### D.3. `S2` — `/ask-agy`

**목적**: agy에게 단발 질의.

**호출**

```bash
agy -p "$PROMPT" \
  --add-dir "$REPO_ROOT" \
  --print-timeout 10m
```

**규칙**

- 기본 모델 지정 없음(agy 기본값). 필요 시 `--model "Gemini 3.1 Pro (High)"`
- 대용량 컨텍스트가 필요하면 `--add-dir` 를 반복 사용
- 후속 질문은 `agy -c "<후속>"` (최근 대화 이어가기)
- `--mode plan` 을 붙이면 읽기 전용에 준하는 계획 모드

---

### D.4. `S3` — `/impl-codex`

**목적**: 계획서를 받아 Codex가 실제 구현.

**전제**: 대상이 git 저장소이고, 작업 브랜치가 이미 분리되어 있을 것.
SVN 저장소면 **실행 전 사용자에게 확인**한다 (롤백 난이도 때문).

**호출**

```bash
codex exec \
  -C "$REPO_ROOT" \
  -s workspace-write \
  -c 'model_reasoning_effort="high"' \
  -o "$OUT/impl-last.md" \
  "$PROMPT" < /dev/null
```

**규칙**

- 프롬프트에 **수정 허용 범위를 명시**한다 (예: `src/main/java` 만, 설정·DB 금지)
- 실행 후 반드시 `git status` / `svn status` 로 변경 파일을 사용자에게 제시
- **커밋하지 않는다.** 커밋은 사용자 지시 후 별도 수행

---

### D.5. `S4` — `/tri-review` ★ 핵심

**목적**: 계획·코드를 Codex·agy에 동시 투척 → 의견 수렴 → 쟁점 정리.

#### D.5.1. 입력 해석

| 입력 | 모드 | 동작 |
|---|---|---|
| 존재하는 파일 경로 | **파일 모드** | 해당 파일 전문을 검토 대상으로 |
| `--diff [base]` | **디프 모드** | git이면 `git diff <base>`, SVN이면 `svn diff`. base 기본값 `origin/dev` (없으면 `HEAD`) |
| 그 외 문자열 | **안건 모드** | 자유 텍스트를 안건으로. Claude가 먼저 안건서를 작성해 `subject.md` 로 저장 |

#### D.5.2. 실행 순서

**1) 대상 수집**
작업 디렉터리(§D.1.1) 생성 → `subject.md`, `schema.json` 기록.

**2) 병렬 질의** — 두 Bash 호출을 **한 메시지에 동시 발행**한다.

```bash
# Codex
codex exec --skip-git-repo-check -C "$REPO_ROOT" -s read-only \
  -c 'model_reasoning_effort="high"' \
  --output-schema "$WORK/schema.json" \
  -o "$WORK/codex.json" \
  "$REVIEW_PROMPT" < /dev/null
```

```bash
# agy
agy -p "$REVIEW_PROMPT" \
  --add-dir "$REPO_ROOT" \
  --output-format json \
  --json-schema "$WORK/schema.json" \
  --print-timeout 10m > "$WORK/agy.json"
```

`$REVIEW_PROMPT` 는 다음을 포함한다:
- 검토 대상 (`subject.md` 내용 또는 경로)
- 역할 지시: *"당신은 독립 검토자다. 동의를 위한 동의를 하지 말 것. 근거 없는 지적도 하지 말 것."*
- 스키마 준수 요구
- **가드**: `~/.claude/`, `.claude/skills/`, `agents/` 하위를 읽거나 수정하지 말 것

**3) 1차 대조** — Claude가 `codex.json` + `agy.json` 의 issue를 대조해 분류한다.

| 분류 | 조건 |
|---|---|
| **합의** | 양쪽이 같은 쟁점을 제기하고 방향이 일치 |
| **이견** | 같은 쟁점에 대해 판단이 상충 |
| **단독 지적** | 한쪽만 제기. severity `critical`/`major` 면 2라운드 대상 |
| **미해결** | `open_questions` 항목 + 2라운드 후에도 안 좁혀진 이견 |

**4) 2라운드 (조건부)**
*이견* 또는 *단독 지적(critical/major)* 이 있을 때만 실행한다.
각 CLI에 **상대의 반대 의견을 첨부**해 재질의:

> "다음은 다른 검토자의 반대 의견이다. 이를 읽고 당신의 입장을 유지할지 철회할지 밝히고 근거를 대라."

결과는 `codex-r2.json` / `agy-r2.json`. **최대 2라운드까지만.** 그 이상 반복 금지.

**5) 보고서 작성** — `REPORT.md`

```markdown
# tri-review: <대상>
- 일시 / 모드 / 대상 / 참여자(Claude·Codex·agy) / 라운드 수
- 판정 요약표: | 검토자 | verdict | 핵심 우려 |

## 합의 사항        (→ 그대로 반영)
## 이견 → 수렴 결과 (→ 2라운드에서 좁혀진 내용)
## 미해결 쟁점      (→ 사용자 결정 필요)
## 부록: 원본 응답  (codex.json / agy.json 경로)
```

**6) 사용자 제시**
터미널에는 **판정 요약표 + 미해결 쟁점만** 출력한다. 전문은 `REPORT.md` 경로만 안내.
미해결이 있으면 사용자에게 결정을 요청하고 **여기서 멈춘다.** 임의로 다음 단계로 진행하지 않는다.

#### D.5.3. 옵션

| 옵션 | 동작 |
|---|---|
| `--diff [base]` | 디프 모드 |
| `--in-repo` | 산출물을 `docs/tri-review/` 에 기록 (LD5 예외) |
| `--solo codex` / `--solo agy` | 한쪽만 호출 (2자 구도 임시 전환) |
| `--no-round2` | 1라운드로 종료 |

---

### D.6. 착수 전 확인 절차

작업 시작 시 **반드시 먼저 실행**한다. 버전 드리프트가 잦다.

```bash
for c in claude codex agy; do printf "%s: " "$c"; command -v "$c" >/dev/null 2>&1 \
  && { "$c" --version 2>&1 | head -1; } || echo "MISSING"; done
```

| 결과 | 조치 |
|---|---|
| 문서 기준(codex 0.152.1 / agy 1.1.25)과 다름 | `--help` 로 §D.2~D.5 의 플래그 유효성 재확인 후 진행 |
| `agy` MISSING | 중단. https://antigravity.google 설치 안내 |
| `codex` MISSING | 중단 |
| 인증 만료 의심 | `codex exec --skip-git-repo-check -s read-only "ping" < /dev/null`, `agy -p "ping"` 로 스모크 테스트 |

### D.7. 완료 기준 (DoD)

- [ ] `~/.claude/skills/ask-codex/SKILL.md` 생성, `/ask-codex` 로 응답 확인
- [ ] `~/.claude/skills/ask-agy/SKILL.md` 생성, `/ask-agy` 로 응답 확인
- [ ] `~/.claude/skills/impl-codex/SKILL.md` 생성
- [ ] `~/.claude/skills/tri-review/SKILL.md` 생성
- [ ] `/tri-review` 를 **이 문서 자체**를 대상으로 1회 시연 → `REPORT.md` 생성 확인
- [ ] 산출물이 대상 저장소를 오염시키지 않음 (`svn status` / `git status` 클린)

---

## 부록 E. 초기 진행 방법 메뉴 (개정 전)

> 아래 프롬프트 메뉴는 Claude Code 스킬 방식에 대한 것이므로 현재 구현에는 사용하지 않는다.
>
> #codex작성

> 이 문서를 첨부하면서 아래 중 하나를 그대로 말하면 됩니다.

### 방법 1 — 전체 구축 (권장)

> **"이 문서 7장 스펙대로 전부 진행해줘."**

스킬 4종 + 7.6 확인 + 7.7 시연까지. 소요 예상 중간. 결정 질문 없음.

### 방법 2 — 최소 구축부터

> **"7장에서 S1, S2만 먼저 만들어줘."**

`/ask-codex`, `/ask-agy` 만. 먼저 감을 잡고 tri-review는 나중에.

### 방법 3 — 핵심만

> **"7.5의 tri-review만 만들어줘."**

교차검증 파이프라인만. 상담용 스킬은 생략.

### 방법 4 — 2자 구도로 축소

> **"LD1을 2자 구도(Claude+Codex)로 바꿔서 7장 진행해줘."**

`agy` 제외. `/ask-agy` 생략, tri-review는 `--solo codex` 고정.

### 방법 5 — MCP 방식 병행

> **"LD3을 뒤집어서 codex는 MCP로 등록하고 나머지는 7장대로 해줘."**

`claude mcp add codex -- codex mcp-server` 추가. 호출이 `mcp__codex__*` 로 표시됨.

### 방법 6 — 외부 솔루션 먼저 시험

> **"7장 대신 2장 A의 PAL MCP를 설치해서 clink부터 써보자."**

직접 만들기 전에 기성품 평가. `agy` 커스텀 등록 필요 (§2-A 주의점 2).

### 방법 7 — 스펙만 재검토

> **"7장 스펙을 tri-review로 교차검증해줘."**

구현 전에 Codex·agy에게 이 스펙 자체를 검토시킴. **단, tri-review가 아직 없으므로 수동 셸 호출로 대체 실행.**

---

### 부분 수정해서 진행하고 싶을 때

LD1~LD8 번호로 지정하면 됩니다.

> "LD5를 `--in-repo` 기본값으로 바꾸고 방법 1로 진행해줘."
> "LD7을 read-only로 고정하고 방법 2 진행해줘."
