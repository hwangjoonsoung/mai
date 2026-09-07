# 검토 프롬프트와 수렴 절차

## 전체 흐름

```text
review-request.md  ──┬──> codex_review.md ──┐
   (동일 프롬프트)    └──> agy_review.md   ──┤
                                            ▼
                              consolidate.md 절차 (Claude)
                                            │
                                            ▼
                                   REVIEW_REPORT.md   ← 검토·수정 대상
                                            │
                        이견 남으면 ────────┤
                                            ▼
                              rebuttal.md (2라운드, 최대 1회)
                                            │
                                            ▼
                          사용자 결정 → SPEC.md §9.2 반영
```

| 파일 | 역할 | 실행 주체 |
|---|---|---|
| `review-request.md` | 1라운드 검토 요청 (양쪽 동일) | codex, agy |
| `rebuttal.md` | 2라운드 반론 템플릿 | codex, agy |
| `consolidate.md` | 두 리포트 → 검토 보고서 수렴 절차 | Claude |

> 이 흐름 전체가 `SPEC.md` §7.9 **Phase 3(구조화 수렴)** 의 수동 예행연습이다. 지금은 사람과 프롬프트로 돌리고, 나중에 `multi_ai_cli` 가 자동으로 돌린다.

---

## review-request.md

`INTENT.md` + `SPEC.md` 를 codex·agy에 던져 독립 설계 검토를 받는 프롬프트.

**두 CLI에 동일한 프롬프트를 준다.** 답을 나란히 대조해야 하므로 프롬프트를 다르게 만들면 안 된다.

### 산출물

| 검토자 | 리포트 파일 |
|---|---|
| Codex | `codex_review.md` |
| agy (Gemini) | `agy_review.md` |
| 2라운드 | `codex_review_r2.md` / `agy_review_r2.md` |

**에이전트가 리포트 파일을 직접 만든다.** 프롬프트가 파일명과 형식을 지시하고, 에이전트가 그 파일 하나만 생성한다. 따라서 **쓰기 권한이 필요하다.**

기존 문서(`INTENT.md`, `SPEC.md` 등) 수정은 프롬프트에서 금지했다. 검토 중 고칠 곳을 발견해도 직접 고치지 말고 리포트에 적게 했다. **반영 여부는 사람이 정한다.**

### 실행 — PowerShell

프로젝트 루트(`multi_ai`)에서 실행한다.

```powershell
$P = Get-Content prompts/review-request.md -Raw

# Codex — 리포트 파일 생성을 위해 workspace-write
codex exec --skip-git-repo-check -C . -s workspace-write `
  -c model_reasoning_effort="high" `
  $P

# agy (Gemini) — 리포트 파일 생성을 위해 accept-edits, 샌드박스는 유지
agy -p $P `
  --add-dir . `
  --model gemini-3.1-pro-high `
  --mode accept-edits --sandbox `
  --disable-slash-commands `
  --print-timeout 10m
```

두 명령은 **동시에 띄워도 된다.** 서로 독립이고, 각자 다른 파일에 쓴다.

실행 후 `codex_review.md` 와 `agy_review.md` 가 생겼는지, 그리고 **기존 문서가 변경되지 않았는지** 확인한다.

```powershell
Get-ChildItem *_review*.md
Get-ChildItem INTENT.md, SPEC.md, BACKGROUND.md, ARCHIVE.md | Select-Object Name, LastWriteTime
```

### 플래그 근거

| 플래그 | 이유 |
|---|---|
| `-s workspace-write` / `--mode accept-edits` | **리포트 파일 생성에 필요.** 워크스페이스 안에서만 쓸 수 있다 |
| `--sandbox` (agy) | 쓰기를 허용해도 워크스페이스 밖으로 나가지 못하게 유지 (§7.2) |
| `-C .` / `--add-dir .` | 문서를 붙여넣지 않고 필요한 파일만 직접 읽게 한다. `ARCHIVE.md` 회피가 가능해진다 |
| `--disable-slash-commands` | 프롬프트에 `/run` 같은 문자열이 있어도 agy가 내부 명령으로 오인하지 않게 (§7.2) |
| `--model gemini-3.1-pro-high` | 검토 용도이므로 flash 대신 추론 강한 pro. `SPEC.md` §9.2 Q6 참고 |

> **쓰기 권한에 대하여**
> `SPEC.md` §7.8과 `INTENT.md` N7의 "검토 단계 쓰기 금지"는 **`multi_ai_cli` 제품이 지켜야 할 규칙**이지, 이 수동 검토 실행에 대한 규칙이 아니다. 여기서는 리포트 파일을 받는 것이 목적이므로 워크스페이스 쓰기를 연다.
> 대신 범위를 좁혔다 — **리포트 파일 1개만 생성, 기존 파일 수정·삭제 금지, 커밋·푸시 금지.** 전부 프롬프트에 명시돼 있다.
> 여전히 불안하면 `-s read-only` + `-o codex_review.md`(codex), `--mode plan` + `> agy_review.md`(agy) 로 CLI가 대신 쓰게 할 수도 있다. 다만 그때는 프롬프트의 "파일을 직접 만들어라" 지시와 어긋나므로 산출물 절을 함께 고쳐야 한다.

### 주의

- `--json-schema` / `--output-schema` 는 **쓰지 않는다.** 구조화 출력은 Phase 3 과제이고, 지금은 사람이 읽고 판단하는 단계다 (`SPEC.md` §9.2 Q1).
- agy에 인라인 JSON을 인자로 넘기면 Windows에서 따옴표 파싱이 깨진다. 스키마가 필요해지면 **반드시 파일 경로**로 준다 (§1.5).
- 리포트가 생성되지 않았다면 프롬프트의 지시대로 전문이 터미널에 출력됐을 것이다. 그 경우 수동으로 저장한다.

### 받은 다음 — 수렴

두 리포트를 눈으로 비교하지 말고 **`consolidate.md` 절차를 태워 `REVIEW_REPORT.md` 를 만든다.**

**이 단계는 CLI 명령이 아니다.** `consolidate.md` 는 codex·agy에게 보내지 않는다. Claude 대화창에 이렇게 치면 된다.

> consolidate.md 대로 수렴해줘

수렴을 codex나 agy에게 시키면 **자기 답안을 자기가 채점**하게 된다. 검토자와 수렴자는 분리한다.

`REVIEW_REPORT.md` 가 실제로 검토·수정의 대상이 되는 문서다. 개별 리포트는 그 근거로 남는다.

이견이나 critical·major 단독 지적이 남으면 `rebuttal.md` 로 2라운드를 돌린다. **최대 2라운드까지만.**

확정된 결정은 `SPEC.md` §9.2에 반영하고 §7.0 `D` 표를 갱신한다.
