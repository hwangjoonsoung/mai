# REBUILD — Rust + PTY 재작성 계획

> 작성일: 2026-09-04
> 결정: **Java 헤드리스 구현을 폐기하고 Rust + PTY 로 다시 만든다.**
> 이유: 에이전트의 실제 대화형 UI를 패널에 띄우고 실시간으로 개입하려면 PTY 가 필수인데, Java 표준으로는 불가능하다.

---

## 1. 왜 다시 만드는가

### 1.1. 지금까지 만든 것의 한계

Java 구현(38파일 4,268줄, Phase 1~4 완료)은 **헤드리스 오케스트레이터**다. `claude -p` 로 호출해 최종 텍스트를 받아 우리가 렌더한다. 이 구조로는 아래가 **원리적으로 불가능**하다.

| 원하는 것 | 왜 안 되는가 |
|---|---|
| 에이전트 실제 UI 표시 | 헤드리스는 UI를 만들지 않는다. 텍스트만 나온다 |
| 권한 프롬프트 응답 | 대화형 세션이 없으니 물어볼 상대가 없다 |
| 응답 스트리밍 | 완료 후 한 번에 받는다 |
| `/` 네이티브 자동완성 | 에이전트의 TUI 기능이다 |
| 중간 개입 (esc 로 중단, 방향 수정) | 세션이 없다 |

### 1.2. Java 로는 못 넘는 벽 세 가지

1. **PTY** — Java 표준에 없다. Windows 는 `CreatePseudoConsole`(ConPTY) 호출이 필요하고 JNI/JNA 가 있어야 한다.
2. **VT 에뮬레이션** — 에이전트가 뱉는 ANSI 를 파싱해 화면 버퍼를 유지해야 한다.
3. **raw 모드 입력** — 키를 포커스된 패널로 넘겨야 하는데 Java 표준으로 불가능하다.

`D12`(외부 라이브러리 금지)를 포기하고 `pty4j` 를 넣어도 2·3 이 남는다. **언어를 바꾸는 것이 맞다.**

### 1.3. 왜 Rust 인가 (Go 아님)

참고 대상인 **herdr 가 Rust 다.** 확인 결과:

| 항목 | herdr |
|---|---|
| 언어 | Rust (`cargo build --release`, "one rust binary, no electron") |
| PTY | `portable-pty` **0.9.0 — 벤더링해서 패치까지 함** |
| TUI | `ratatui` 0.30 + `crossterm` 0.29 |
| 비동기 | `tokio` |
| IPC | `interprocess` 2.4.2 |
| 문자폭 | `unicode-width` |
| 라이선스 | Apache 2.0 · Windows 지원 |

Windows PTY(ConPTY) 지원은 **Rust 생태계가 Go 보다 성숙하다.** `portable-pty` 는 wezterm 이 실사용 중인 크레이트다.

---

## 2. 무엇이 달라지는가

```text
[ 지금 — Java 헤드리스 ]

  사용자 ──> multi_ai_cli ──> claude -p "프롬프트"  ──> 최종 텍스트
                          └─> codex exec -         ──> 최종 텍스트
                          └─> agy -p "프롬프트"    ──> 최종 텍스트
                                                        └─> 우리가 렌더

[ 앞으로 — Rust + PTY ]

  사용자 ──키──> 포커스된 패널 ──> PTY ──> claude (대화형 그대로)
                                              │
                    화면 버퍼 <── VT 파서 <────┘
                          └──> ratatui 로 패널에 합성
```

**핵심 전환**: 우리가 답을 렌더하는 게 아니라, **에이전트가 자기 터미널에 그린 것을 그대로 보여준다.**

### 2.1. 그런데 헤드리스도 버리지 않는다

`/converge` 교차검증은 **헤드리스가 맞다.** 스키마 강제·구조화 출력·자동 분류가 대화형에서는 안 나온다.

**한 바이너리에 두 실행 모드를 둔다.**

| 용도 | 실행 방식 | 근거 |
|---|---|---|
| 일반 대화, 패널 | **PTY** (`portable-pty`) | 실제 UI, 개입 가능 |
| `/converge` | **헤드리스** (`std::process::Command`) | 스키마 강제 필요 |

`std::process::Command` 는 Rust 표준이라 추가 비용이 없다. **`/converge` 로직은 그대로 이식된다** — 스키마·분류 규칙·2라운드 조건은 언어와 무관하다.

---

## 3. 아키텍처

### 3.1. 모듈 구성

```text
multi_ai_cli/                     (Cargo 워크스페이스 루트)
  Cargo.toml
  src/
    main.rs                       진입점, CLI 인자
    app.rs                        앱 상태, 이벤트 루프
    config.rs                     config.toml 로드

    pty/
      mod.rs                      PtySession 계약
      spawn.rs                    portable-pty 로 자식 기동
      reader.rs                   PTY 출력을 비동기로 읽어 파서에 공급

    vt/
      mod.rs                      화면 버퍼 (셀 격자, 커서, 속성)
      parser.rs                   ANSI/VT 시퀀스 파싱
      screen.rs                   ratatui 위젯으로 변환

    ui/
      layout.rs                   spaces·agents 사이드바 + 탭 + 패널 분할
      pane.rs                     패널 하나 (VT 화면 또는 텍스트)
      sidebar.rs                  에이전트 상태 목록
      input.rs                    입력 라우팅, 포커스 관리

    agent/
      mod.rs                      Agent 계약 (id, 표시명, 기동 명령)
      claude.rs / codex.rs / agy.rs
      registry.rs                 설치 탐색 (기존 CommandResolver 이식)

    converge/                     ← Java 에서 그대로 이식
      schema.rs                   ReviewSchema
      review.rs                   StructuredReview 파싱
      engine.rs                   합의/이견/단독지적/미해결 분류
      report.rs                   REPORT.md 생성
      session.rs                  1R → 분류 → 조건부 2R

    room/
      transcript.rs               길이 기반 프레이밍 (Java 규격 그대로)
      repository.rs               방 저장·재개
```

### 3.2. 실행 흐름

```text
1. 시작
   - config.toml 로드, 에이전트 설치 탐색
   - 시작 화면 (질문 입력 대기)

2. 질문 입력
   - 대상 에이전트마다 PTY 열고 대화형으로 기동
   - 각 PTY 에 프롬프트를 키 입력으로 주입

3. 실행 중
   - PTY 출력 → VT 파서 → 화면 버퍼 갱신
   - 이벤트 루프가 주기적으로 패널 재렌더
   - 사용자 키 입력 → 포커스된 패널의 PTY 로 전달
     (권한 승인, esc 중단, / 자동완성 전부 여기서 됨)

4. /converge
   - PTY 를 쓰지 않고 헤드리스로 별도 spawn
   - 스키마 강제 → JSON 파싱 → 분류 → REPORT.md
```

### 3.3. 화면 구성 (herdr 참고)

```text
┌ spaces ─────┬─ [탭] claude · codex · agy ──────────────────────┐
│ ● herdr     │┌ claude ────┐┌ codex ─────┐┌ agy ──────────────┐│
│   master    ││            ││            ││                   ││
│ ● web-dash  ││  에이전트  ││  에이전트  ││   에이전트        ││
│   feat/...  ││  실제 TUI  ││  실제 TUI  ││   실제 TUI        ││
├ agents ─────┤│  그대로    ││  그대로    ││   그대로          ││
│ ○ claude    ││            ││            ││                   ││
│   working   ││            ││            ││                   ││
│ ● codex     │└────────────┘└────────────┘└───────────────────┘│
│   blocked   │ ~/proj > master * ↑1 > ctx ── 3% 31k/1M          │
└─────────────┴──────────────────────────────────────────────────┘
```

- **왼쪽 위 spaces** — 작업 디렉터리/브랜치 묶음
- **왼쪽 아래 agents** — 에이전트별 상태 (working / idle / blocked / done)
- **가운데** — 에이전트마다 패널 하나. 그 안이 **에이전트의 실제 화면**

---

## 4. 기술 스택

| 크레이트 | 용도 | 비고 |
|---|---|---|
| `portable-pty` | PTY 생성·크기 조절 | Windows ConPTY 지원. herdr 는 패치해서 씀 |
| `ratatui` | TUI 레이아웃·렌더 | 패널·사이드바·탭 |
| `crossterm` | 터미널 백엔드, raw 모드, 키 이벤트 | ratatui 백엔드 |
| `tokio` | 비동기 런타임 | PTY 읽기, 타이머, 프로세스 |
| `unicode-width` | 한글 2칸 폭 계산 | Java 에서 직접 짰던 것 |
| `serde` + `serde_json` | 구조화 출력 파싱 | Java 에서 자작한 JSON 파서 대체 |
| `toml` | 설정 파일 | `.properties` 대체 |
| `anyhow` / `thiserror` | 오류 처리 | |
| **VT 에뮬레이션** | **미정 — §7.1 참조** | 최대 위험 요소 |

### 4.1. Java 대비 사라지는 제약

| Java 제약 | Rust 에서는 |
|---|---|
| `D12` 외부 라이브러리 금지 → JSON 파서 자작 | `serde_json` 사용. Cargo 가 의존성을 다룬다 |
| Windows 명령행 32,767자 한계 | **사라진다.** PTY 는 stdin 으로 주입한다 |
| agy 가 stdin 을 못 받음 | **사라진다.** 대화형이라 키 입력으로 넣는다 |
| 문맥 상한 16,000자 | 헤드리스(`/converge`)에만 남는다 |
| raw 입력 불가 → `/s` 로 스크롤 | 방향키·PgUp 정상 동작 |

---

## 5. 이월 / 폐기

### 5.1. 그대로 쓰는 것

| 자산 | 상태 |
|---|---|
| `INTENT.md` | **그대로.** 목적·동기·금지사항은 언어 무관 |
| `SPEC.md` §0~§5 | 그대로 (요구사항, 환경 실측, UX 규약) |
| `SPEC.md` §7.5-1 수렴자 계약 | 그대로 |
| `REVIEW_REPORT.md` | 그대로 (5라운드 교차검토 결과) |
| `prompts/` | 그대로 |
| **§7.6 transcript 프레이밍 규격** | 그대로. 길이 기반 프레이밍은 언어와 무관 |
| **converge 분류 규칙** | 그대로 이식 |

### 5.2. 실측으로 확정했던 사실들

전부 유효하다. Rust 로 바꿔도 CLI 동작은 안 바뀐다.

- codex 스키마는 모든 object 에 `additionalProperties: false` 필요
- claude `--json-schema` 는 경로가 아니라 문자열만 받음
- agy 는 `--json-schema <경로>` + `--output-format json` 정상
- agy 기본 모델 `gemini-3.1-pro-high` (`D13`)
- codex 네이티브 실행 파일 경로 (`node_modules/@openai/codex-win32-x64/vendor/.../codex.exe`)
- 셸 래퍼(`.cmd`/`.ps1`) 경유 금지 — 셸 메타문자 재해석

### 5.3. 개정이 필요한 것

| 문서 | 무엇을 |
|---|---|
| `SPEC.md` §6.3 | 프로세스 실행 규칙을 PTY 기준으로 다시 쓴다. `ProcessBuilder` 전제가 사라진다 |
| `SPEC.md` §7.2 | 호출 프로필을 대화형/헤드리스 두 벌로 나눈다 |
| `SPEC.md` §7.1 | 프로젝트 구조를 Rust 모듈로 교체 |
| `SPEC.md` `D12` | 폐기. Cargo 로 의존성을 관리한다 |
| `SPEC.md` `D15` | 헤드리스 경로에만 적용으로 축소 |
| `USAGE.md` | 전면 재작성 (빌드·실행·조작이 전부 바뀜) |

### 5.4. 폐기하는 것

**Java 소스 38파일.** 다만 **바로 지우지 않는다.**

- Rust 가 동등 기능에 도달할 때까지 `legacy-java/` 로 옮겨 참조용으로 남긴다
- 특히 `converge/` 와 `TranscriptCodec` 은 이식 시 원본 대조가 필요하다
- 도달 후 태그를 남기고 제거한다 (`git tag java-final`)

---

## 6. 단계 계획

각 단계 끝에 **동작하는 바이너리**가 나와야 한다. Java 때처럼 실행해 보지 않고 쌓지 않는다.

### R0 — 선행 조건 (사용자 작업)

Rust 툴체인과 링커가 없다. 둘 중 하나를 설치한다.

```powershell
# (권장) 가볍고 관리자 권한 불필요
scoop install rustup mingw
rustup default stable-x86_64-pc-windows-gnu

# (대안) Rust 공식 기본값. 2~3GB, 관리자 권한 필요할 수 있음
winget install --id Rustlang.Rustup -e
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --quiet"
```

확인: `cargo --version` 이 나오면 된다.

### R1 — PTY 한 개를 띄워 화면에 붙인다

**이 단계가 전체의 성패를 가른다.** 여기서 막히면 나머지는 의미가 없다.

- `portable-pty` 로 `claude` 를 PTY 에 띄운다
- 출력을 VT 파서에 넣어 화면 버퍼를 만든다
- `ratatui` 로 전체 화면에 그린다
- 키 입력을 PTY 로 넘긴다

**완료 기준**: 터미널에서 `claude` 를 직접 쓰는 것과 **구분이 안 될 것.** 로고·상태바·`/` 자동완성·권한 프롬프트가 다 보이고 조작돼야 한다.

### R2 — 패널 분할과 포커스

- 에이전트 셋을 각각 PTY 에 띄우고 나란히 배치
- 패널마다 크기에 맞춰 PTY resize
- 탭/포커스 전환, 포커스된 패널로만 키 전달
- 시작 화면(질문 입력) → 패널 화면 전환

**완료 기준**: 셋을 동시에 띄우고 하나를 골라 조작. 나머지는 계속 돌아간다.

### R3 — 사이드바와 상태

- spaces (작업 디렉터리·브랜치)
- agents (working / idle / blocked / done)
- 상태 판정 근거 정의 — 출력 변화, 프로세스 상태

### R4 — 헤드리스 경로와 `/converge` 이식

- `std::process::Command` 로 헤드리스 spawn
- 스키마 강제 → `serde_json` 파싱
- 분류 엔진·2라운드·`REPORT.md` 이식
- transcript 프레이밍 이식

### R5 — 방 저장·재개, 설정, macOS

- `~/.multi-ai-cli/` 구조 유지
- `config.toml`
- macOS 확인 (Rust 는 크로스플랫폼이라 Java 때보다 부담이 적다)

---

## 7. 위험

### 7.1. VT 에뮬레이션 — **최대 위험**

**herdr 의 `Cargo.toml` 에 VT 에뮬레이션 크레이트가 없다.** `portable-pty` + `ratatui` + `crossterm` 뿐이다. 즉 **직접 구현했다는 뜻이다.**

에이전트 TUI 를 패널에 정확히 그리려면 아래를 다뤄야 한다.

- CSI/OSC/SGR 시퀀스 파싱
- 셀 격자 + 속성(색·굵기·역상)
- 커서 이동·저장·복원
- 스크롤 영역, 화면 지우기
- 대체 화면 버퍼 (에이전트 TUI 가 쓴다)
- 와이드 문자(한글) 셀 점유

**대응 순서**

1. **먼저 기성 크레이트를 시도한다** — `vt100`, `avt`, `wezterm-term` 후보. R1 에서 실제로 붙여보고 판정한다.
2. 기성품이 부족하면 **필요한 부분만 직접 구현**한다. 전체 VT 스펙이 아니라 대상 에이전트 셋이 실제로 쓰는 시퀀스만.
3. **R1 에서 판정한다.** 여기서 안 되면 계획을 다시 짠다.

> 이건 조사로 결론 낼 문제가 아니라 **붙여봐야 아는 문제**다. R1 을 짧게 끊어 빨리 확인한다.

### 7.2. Windows ConPTY

- `portable-pty` 가 지원하지만 GNU 툴체인에서의 동작을 R0 직후 확인해야 한다
- 안 되면 MSVC 툴체인으로 전환

### 7.3. 리소스

- PTY 셋 + VT 파싱 + 주기적 렌더는 헤드리스보다 무겁다
- 렌더 주기와 파싱 배치를 조절할 여지를 처음부터 둔다

### 7.4. Rust 학습 비용

- 소유권·비동기가 처음이면 R1 이 오래 걸릴 수 있다
- 단계를 짧게 끊고 매번 돌려보는 것으로 상쇄한다

---

## 8. 미결정 사항

착수 전에 정하지 않아도 되지만, 해당 단계 전에는 정해야 한다.

| # | 안건 | 언제까지 |
|:---:|---|---|
| **P1** | VT 에뮬레이션 — 기성 크레이트 vs 자작 | R1 중 |
| **P2** | 툴체인 — GNU vs MSVC | R0 |
| **P3** | 프롬프트 주입 방식 — PTY 에 키 입력으로 넣을지, 에이전트 기동 인자로 넣을지 | R1 |
| **P4** | 에이전트 상태(working/blocked) 판정 근거 | R3 |
| **P5** | Java 코드 제거 시점 | R4 이후 |
| **P6** | `/converge` 를 PTY 패널에서도 트리거할지, 별도 화면으로 뺄지 | R4 |

---

## 9. 다음에 할 일

1. **R0 — 툴체인 설치** (사용자 작업, 또는 지시 시 대행)
   ```powershell
   scoop install rustup mingw
   rustup default stable-x86_64-pc-windows-gnu
   cargo --version
   ```
2. **Cargo 프로젝트 골격 생성** — 의존성 확정, 빈 모듈
3. **R1 착수** — PTY 하나에 `claude` 를 띄워 화면에 붙인다. **VT 에뮬레이션 판정이 여기서 난다.**

R1 결과에 따라 이 문서를 갱신한다. 특히 §7.1 판정 결과는 이후 전부에 영향을 준다.

---

## 참고

| 문서 | 내용 |
|---|---|
| [INTENT.md](INTENT.md) | 목적·동기·금지사항 (유효) |
| [SPEC.md](SPEC.md) | 기존 스펙. §6.3·§7.1·§7.2 는 개정 대상 |
| [REVIEW_REPORT.md](REVIEW_REPORT.md) | 5라운드 교차검토 (유효) |
| [USAGE.md](USAGE.md) | Java 판 사용법. R5 에서 재작성 |
| herdr | https://github.com/herdrdev/herdr · Apache 2.0 · Rust |

---

## 10. R0·R1 진행 기록 (2026-09-04)

### R0 — 툴체인: **완료**

| 항목 | 결과 |
|---|---|
| `cargo` | **1.98.1** 설치됨 (`~/.cargo/bin`) |
| 툴체인 | `stable-x86_64-pc-windows-msvc` |
| MSVC 링커 | `BuildTools\...\MSVC\14.44.35207\bin\HostX64\x64\link.exe` |
| Windows SDK | `10.0.26100.0` (ucrt·um 라이브러리 존재) |

**막혔던 것**: Developer Command Prompt 가 아니면 `LIB`·`INCLUDE` 가 비어 링커가
`LNK1181: 'dbghelp.lib' 를 열 수 없습니다` 로 실패한다. `DbgHelp.Lib` 는 SDK 에 있지만
경로가 안 잡힌다.

**해결**: `rust/build.ps1` 이 설치된 MSVC 툴셋과 SDK 버전을 자동 탐색해 `LIB`/`INCLUDE` 를
세팅한다. 이후 `cargo build` 정상 동작 확인.

### 크레이트 확정

```toml
portable-pty = "0.9"      # PTY (Windows ConPTY)
ratatui      = "0.30"     # TUI
crossterm    = "0.29"     # 백엔드·raw 모드·키 이벤트
vt100        = "0.16"     # VT 에뮬레이션 ← §7.1 위험 요소의 답
anyhow       = "1"
```

**버전 충돌 하나**: `ratatui 0.29` 는 `unicode-width` 를 `=0.2.0` 으로 고정하는데
`vt100 0.16` 은 `^0.2.1` 을 요구한다. **herdr 와 같은 `ratatui 0.30` + `crossterm 0.29`
로 맞추니 해소됐다.**

### §7.1 VT 에뮬레이션 판정: **기성품으로 된다 (vt100 0.16)**

직접 구현하지 않아도 된다. `vt100::Parser` 가 화면 버퍼·커서·색·와이드 셀을 모두
제공하며, `src/vtscreen.rs` 에서 ratatui 버퍼로 옮기는 코드는 60줄이면 끝난다.

> herdr 에 VT 크레이트가 없던 것은 직접 짰기 때문이지, 기성품이 없어서가 아니었다.

### R1 — PTY 렌더: **통과 (원인 규명 및 해결 완료)**

> 아래 「미판정」 기록은 과정 보존용이다. **결론은 §10.1 을 본다.**

### R1 — 진행 중 기록

작성 완료된 것:

| 파일 | 내용 |
|---|---|
| `rust/src/pty.rs` | PTY 열기·자식 기동·비동기 읽기·resize·키 쓰기 |
| `rust/src/vtscreen.rs` | vt100 화면 → ratatui 버퍼 (색·굵기·역상·와이드 셀) |
| `rust/src/main.rs` | 렌더 루프, 키 인코딩, `--selftest` |
| `rust/build.ps1` · `rust/run.ps1` | LIB 자동 설정, 실행 |

**막힌 지점**: 자동 검증 환경(Bash·PowerShell `Start-Job`)에서 자식 출력이 전혀 오지 않고
자식이 종료도 하지 않는다. PTY 기동 자체는 성공한다(`controlling_tty: true`, argv 정상).

```
자식: cmd ["/c", "echo 빨강 OK"]
자식 종료: false
화면 첫 줄: "                    (전부 공백)                    "
```

**원인 후보**

1. **콘솔 부재** — ConPTY 는 콘솔이 붙어 있어야 한다. 이 세션의 Bash 와
   `Start-Job` 은 둘 다 콘솔이 없다. **가장 유력하다.**
2. **portable-pty 의 Windows 문제** — herdr 가 `portable-pty 0.9.0` 을
   **벤더링해서 패치**한 것이 방증이다. 스톡 크레이트에 손볼 곳이 있다는 뜻.

**판정 방법 — 사용자가 실제 터미널에서 실행**

```powershell
cd C:\Users\HJS\Desktop\multi_ai\rust
.\run.ps1 -SelfTest      # PTY+VT 파이프라인만 자동 점검
.\run.ps1                # claude 를 PTY 로 띄운다. Ctrl+] 로 탈출
```

- `-SelfTest` 가 `RESULT: PTY + VT 파이프라인 정상` 을 내면 원인 1 이 맞고 **R1 통과**다.
- 실제 터미널에서도 화면이 비면 원인 2 다. 그때는 아래로 간다.

**원인 2 일 때의 대응 순서**

1. `portable-pty` 최신 버전 시도
2. herdr 의 벤더 패치 내용 확인 (Apache 2.0 이라 참고 가능)
3. `windows-sys` 로 `CreatePseudoConsole` 직접 호출

### 다음

R1 판정 결과를 이 절에 추가한 뒤 R2(패널 분할)로 넘어간다. **판정 전에는 R2 를
시작하지 않는다** — 기반이 안 되면 위에 쌓는 것이 의미가 없다.


---

## 10.1. R1 판정 — **통과**

### 원인: ConPTY 의 질의에 답하지 않아 막혀 있었다

실제 터미널에서도 동일하게 실패해 「콘솔 부재」 가설은 기각됐다. 수신 바이트를
덤프해 원인을 특정했다.

```
[rx 16] "[?9001h[?1004h"      win32-input-mode, 포커스 리포팅 활성화
[rx  4] "[6n"                      ← DSR: 커서 위치 질의
```

**ConPTY 는 기동 직후 `ESC[6n` 으로 커서 위치를 묻고 응답을 기다린다.** 답하지 않으면
자식의 출력이 한 바이트도 흘러나오지 않고 자식도 종료하지 않는다.

`vt100` 크레이트는 이 응답을 만들어 주지 않는다 — `Callbacks` 트레이트에 벨·리사이즈·
제목 훅만 있고 질의 응답 훅이 없다. **터미널 에뮬레이터를 만든다는 것은 질의에 답할
책임까지 진다는 뜻이고, 그 부분은 우리 몫이다.**

### 해결

`PtySession::answer_queries()` 신설. 수신 청크에서 CSI 질의를 훑어 응답을 되돌린다.

| 질의 | 의미 | 응답 |
|---|---|---|
| `ESC[6n` · `ESC[?6n` | 커서 위치 (DSR) | `ESC[{row};{col}R` (1-기준) |
| `ESC[5n` | 상태 (DSR) | `ESC[0n` |
| `ESC[c` · `ESC[0c` | 장치 속성 (DA1) | `ESC[?1;2c` |
| `ESC[>c` | 보조 장치 속성 (DA2) | `ESC[>0;10;1c` |

### 검증 결과

```
자식 종료: true (코드 Some(0))
수신 바이트: 110
화면 첫 줄: "빨강 OK"
'빨' 전경색: Some(Idx(1))
'빨' 와이드 셀: Some(true)

RESULT: PTY + VT 파이프라인 정상
```

| 확인 항목 | 결과 |
|---|:---:|
| Windows ConPTY 로 자식 기동·정상 종료 | **통과** |
| 출력이 vt100 파서를 거쳐 셀 격자에 반영 | **통과** |
| ANSI 색 속성 보존 (빨강 = `Idx(1)`) | **통과** |
| 한글 와이드 셀 (두 칸) 처리 | **통과** |

### 이 단계에서 겪은 함정 기록

| 함정 | 내용 |
|---|---|
| MSVC `LIB` 미설정 | Developer Command Prompt 밖에서 `LNK1181: dbghelp.lib`. `build.ps1` 이 자동 설정 |
| `ratatui 0.29` ↔ `vt100` | `unicode-width` 를 `=0.2.0` 으로 고정해 충돌. `ratatui 0.30` 으로 해소 |
| **ConPTY 질의 미응답** | **자식 출력이 전혀 안 나온다. 위 참조** |
| Git Bash 경로 변환 | `cmd /c` 인자가 `cmd C:/` 로 바뀐다. `MSYS_NO_PATHCONV=1` 필요 |
| 소스의 raw ESC 바이트 | 도구를 거치며 깨진다. `` 이스케이프로 작성한다 |

### 다음 — R2

기반이 확인됐으므로 패널 분할로 넘어간다.

1. 사용자가 실제 터미널에서 `.un.ps1` 로 에이전트 TUI 육안 확인
2. R2 — 에이전트 셋을 각각 PTY 에 띄우고 나란히 배치, 포커스 전환

---

## 10.2. R2·R3 진행 (2026-09-04)

### R2 — 패널 분할: **완료**

- 화면 둘: Idle(질문 입력) → Panes(참여자별 칸)
- 질문 확정 시 참여자를 각각 PTY 로 띄우고 각 칸 크기로 resize
- 포커스된 칸에만 키가 가고, 커서도 그 칸에만 표시된다

**키 라우팅 결정**: 기본은 자식에게 그대로 보낸다. 우리 조작을 늘리면 그만큼
에이전트가 쓰는 키를 뺏게 되고 "직접 쓰는 것과 같다"는 전제가 깨진다.

| 키 | 동작 |
|---|---|
| `Alt+←` `Alt+→` | 패널 이동 |
| `Alt+1~9` | 직접 선택 |
| `Ctrl+]` → `n` | 새 질문 |
| `Ctrl+]` → `q` | 종료 |
| 그 외 전부 | 포커스된 자식에게 전달 |

### R2 에서 잡은 결함

| 증상 | 원인 | 조치 |
|---|---|---|
| 키가 두 번 입력 (`/` → `//`) | Windows crossterm 이 Press·Release 를 모두 보낸다 | `KeyEventKind` 필터 |
| codex 가 «대기» 로 안 뜸 | PATH 에 `codex.exe` 가 없고 셸 shim 만 있다. `CreateProcessW` os error 193 | 벤더 네이티브 exe → node+codex.js 순 탐색. **Java 판 로직 이식 누락이었다** |
| 첫 화면 한글 커서 어긋남 | `chars().count()` 로 셌다 | `unicode-width` 표시 폭 |
| agy 가 질문에 답 안 함 | 고정 1.5초 뒤 일괄 주입. agy 는 기동이 느려 입력이 삼켜졌다 | 패널별로 **출력이 멎으면** 주입 |
| 종료 후 자식 잔존 | 정리를 안 했다 | `kill()` + `Drop` |
| 신뢰 대화상자가 막음 | 처음 보는 디렉터리 | `--trust` 로 사전 등록 (§10.3) |

### R3 — 사이드바: **완료**

herdr 배치를 따라 왼쪽에 두 칸을 뒀다.

```
┌ spaces ────┐
│ rust       │  디렉터리 이름
│ master     │  git 브랜치 (.git/HEAD 직접 읽음)
│ …/multi_ai │  경로
├ agents ────┤
│ ● Claude   │
│   출력 중  │
│ ◌ Codex    │
│   멎음     │
└────────────┘
```

**상태는 관측 가능한 사실만 쓴다.** 프로세스가 살아 있는지, 최근 출력이 있었는지만
안다. "무엇을 하는 중인지"는 알 수 없고, 추측해 표시하면 사용자를 오도한다.

| 상태 | 근거 |
|---|---|
| 대기 | 아직 안 띄웠다 |
| 기동 중 | 띄웠고 프롬프트 주입 대기 |
| 출력 중 | 최근 800ms 안에 출력이 있었다 |
| 멎음 | 살아 있지만 잠잠하다 |
| 종료 | 프로세스가 끝났다 |

## 10.3. 신뢰 대화상자 대응

에이전트들이 처음 보는 디렉터리에서 신뢰를 묻고 멈춘다. claude 는 대화형에서
이를 건너뛸 플래그가 없다(비대화형 `-p` 에서만 생략).

각 CLI 의 저장 위치를 실측해 같은 기록을 남기는 `--trust` 를 만들었다.

| 에이전트 | 저장 위치 |
|---|---|
| claude | `~/.claude.json` → `projects[<경로>].hasTrustDialogAccepted = true` |
| codex | `~/.codex/config.toml` → `[projects."<소문자 경로>"] trust_level = "trusted"` |
| agy | 신뢰 대화상자를 쓰지 않는다 |

**보안 결정이라 자동으로 하지 않는다.** `--trust` 를 직접 실행할 때만, 지정한
워크스페이스 하나에 대해서만 기록한다.

## 10.4. 다음 — R4

R4 는 헤드리스 경로와 `/converge` 이식이다. PTY 와 무관한 별도 실행 경로라
지금 구조를 건드리지 않는다.

- `std::process::Command` 로 헤드리스 spawn
- 스키마 강제 → `serde_json` 파싱 (Java 판 자작 파서 불필요)
- 분류 엔진·2라운드·`REPORT.md` 이식
- transcript 길이 기반 프레이밍 이식

## 10.5. R4 — 헤드리스와 `/converge`: **완료**

PTY 와 무관한 **별도 실행 경로**로 구현했다. 대화형에서는 스키마 강제·구조화
출력이 안 나오므로 한 바이너리에 두 모드를 둔다.

| 모듈 | 내용 |
|---|---|
| `converge/schema.rs` | 공통 스키마와 1·2라운드 프롬프트 |
| `converge/review.rs` | 응답 파싱. 텍스트에서 JSON 객체를 찾아낸다 |
| `converge/engine.rs` | 합의·이견·단독지적 분류, 2라운드 조건 |
| `converge/report.rs` | REPORT.md |
| `converge/headless.rs` | 공급자별 호출 프로필 (`std::process::Command`) |
| `converge/mod.rs` | 1R → 분류 → 조건부 2R → 보고서 |

**Java 판에서 실측으로 확정한 사실을 그대로 옮겼다.**

| 공급자 | 프로필 |
|---|---|
| claude | 프롬프트 stdin, `--output-format text`. `--json-schema` 는 안 쓴다(문자열만 받고 인자 전달이 깨지기 쉽다). 프롬프트에 스키마를 싣는다 |
| codex | `exec -` 로 stdin, `-o <파일>`, `--output-schema <파일>` |
| agy | stdin 불가 → 인자. `--json-schema <경로>` + `--output-format json` |

**실측 검증** (`--converge "16000자 문맥 상한이 적절한가?"`)

```
1라운드 — 3명 독립 검토
  [Claude] CONCERNS · 지적 3건
  [Codex]  CONCERNS · 지적 3건
  [agy]    CONCERNS · 지적 2건
2라운드 — critical·major 단독 지적이 있다   ← 자동 발동
보고서 생성 · 미해결 9건을 사용자 결정으로 올림
```

### 진단 장치

파싱 실패를 눈으로 볼 수 없으면 원인을 좁힐 수 없다. **공급자 원시 출력을 항상
남긴다** — `runs/<round>/converge/<id>.r1.raw.txt`. 실패 시 그 경로를 함께 보고한다.

실제로 한 번 Claude 파싱이 실패했는데, 원문 덤프로 재현·확인한 결과 일시적
문제였고 재실행에서 정상이었다.

## 10.6. R5 — 저장·설정·macOS: **완료**

| 항목 | 내용 |
|---|---|
| 저장 구조 | `~/.multi-ai-cli/{config.toml, temp/, rooms/<id>/}` — Java 판과 동일 |
| transcript | **길이 기반 프레이밍** 이식. LF 고정, 이스케이프 없음 |
| 재동기화 | 단조 증가 + `next_id` 상한. 복구분은 `[복원 의심]` 으로 표시 |
| 설정 | `config.toml`. 읽는 키가 몇 개뿐이라 toml 크레이트를 쓰지 않는다 |
| 방 조회 | `--rooms` 목록, `--show <ID>` 기록 |
| macOS | 확장자 없는 PATH 탐색, Homebrew·`~/.local/bin` 보조 경로, codex 벤더 바이너리 |

**`id == last_id + 1` 로 강화하지 않는다.** 손상으로 id 가 건너뛴 뒤 모든 후속
메시지를 잃는다 — 5라운드 교차검토에서 실측으로 확정한 결론이다.

### PTY 에서 "방 재개"의 의미

Java 판과 다르다. PTY 세션은 되살릴 수 없다 — 에이전트 프로세스는 이미 끝났다.
따라서 재개는 **기록 조회**를 뜻한다. 에이전트 대화 자체의 재개는 각 CLI 가
자기 방식으로 제공한다(`claude --resume` 등). 우리가 흉내 내지 않는다.

## 10.7. 전체 진행

| | 내용 | 상태 |
|:---:|---|:---:|
| R0 | 툴체인 | 완료 |
| R1 | PTY 한 개 렌더 | 완료 |
| R2 | 패널 분할·포커스 | 완료 |
| R3 | 사이드바·상태 | 완료 |
| R4 | 헤드리스·`/converge` | 완료 |
| R5 | 저장·설정·macOS | 완료 |

macOS 는 **코드 경로만 마련했고 실기 검증은 못 했다.** Windows 에서만 실행해 봤다.

## §10.7 — 「공간 × 세션」 구조 전환과 서브에이전트 관측 (실측)

### 서브에이전트는 프로세스가 아니다

사이드바에 서브에이전트를 띄우려고 처음엔 **프로세스 트리**를 훑었다. 틀렸다.

측정: Claude Code 를 PTY 로 띄우고 Explore 서브에이전트 둘을 실제로 끝까지 돌리며
자손 프로세스 수를 1초마다 셌다.

```
   8s  자손 20개 (기준)          ← 프롬프트 주입
  24s  자손 20개 (기준 대비 +0)  ← 서브에이전트 2개 확인, 도는 중
 109s  자손 15개 (기준 대비 -5)  ← 서브에이전트 완료
```

자손은 **한 개도 늘지 않았고 오히려 줄었다.** 잡히는 20개는 전부 `conhost.exe`,
`node.exe`, `cmd.exe` — MCP·셸 도구용 인프라다. 서브에이전트는 별도 OS 프로세스가
아니라 **같은 프로세스 안의 작업**이다. 프로세스 트리로는 원리적으로 볼 수 없다.

`procs.rs`(sysinfo)는 전제가 무너졌으므로 삭제했다.

### 실제 관측 지점 — 에이전트가 그리는 명부

화면을 뜨자 답이 있었다. 에이전트가 자기 TUI 아래에 명부를 그린다.

```
  ● main
  ◯ Explore  Find all TOML files                9s · ↓ 26.5k tokens
  ◯ Explore  Count Rust source files            6s · ↓ 24.3k tokens
```

`subagents.rs` 가 VT 화면에서 이걸 읽는다. 남의 TUI 를 읽는 것이라 형식이 바뀌면
깨지는데, **깨질 때 아무것도 안 보이도록** 만들었다 — `● main` 이라는 정확한 앵커를
못 찾으면 빈 목록을 준다. 틀린 것을 자신 있게 보여주는 것보다 낫다.

검증: 단위 테스트 2건 + 실제 Explore 2개 동시 실행에서 종류·설명·진행 상태 모두 정확.

### 구조 전환

「고정 참여자 3인 × 워크스페이스 1개」 → 「공간 N개 × 세션 M개」.

- `model.rs` — `Space`(경로) 와 `Session`(에이전트 + PTY + 소속 공간)
- 공간을 바꾸면 다른 공간의 세션은 **죽이지 않고 숨긴다.** 돌아오면 그대로다
- 세션이 `MAX_SPLIT`(3) 을 넘으면 나란히 두지 않고 **탭**으로 바꾼다.
  좁은 칸에 에이전트 TUI 를 밀어 넣으면 자식이 자기 화면을 접어 읽을 수 없다
- `PtySession::spawn_in(.., cwd)` — 세션은 자기 공간의 경로에서 뜬다

### 셀프테스트 경합 (실측)

`--selftest` 가 간헐 실패했다. 원인은 자식 **종료를 보자마자 단정**한 것이다.
프로세스가 끝나도 PTY 버퍼에는 안 읽은 출력이 남는다.

```
실패한 회차: 수신 91 바이트  → 화면 첫 줄 비어 있음
성공한 회차: 수신 110 바이트 → "빨강 OK"
```

종료 후에도 **출력이 300ms 잠잠해질 때까지** 더 읽도록 고쳤다. 10회 연속 통과.

## §10.8 — 경로 입력·전체 보기 탭·클릭 판정 수정

### 클릭이 엉뚱한 에이전트로 가던 원인

탭 모드에서 `pane_hit`(클릭 대상 좌표)을 **비우지 않았다.** 탭 칩은 매 프레임
새로 밀어 넣는데 지우지 않으니 프레임마다 쌓였다. `hit_test` 는 먼저 맞는 것을
돌려주므로 **가장 오래된 항목**이 이겼고, 세션을 닫거나 더한 뒤로는 그 좌표가
가리키던 인덱스가 이미 다른 에이전트를 뜻하게 되어 있었다.

비우는 시점이 두 곳(`draw_tabbar`, `draw_bodies`)에 흩어져 있었고 한쪽 분기에만
있었던 것이 원인이다. **프레임 시작에 한 번** 비우도록 모았다.

### 한글 경로에서 패닉 (실측)

탭 완성 후보를 거를 때 바이트로 잘랐다.

```rust
n.len() >= prefix.len() && n[..prefix.len()].eq_ignore_ascii_case(&prefix)
```

`~/없는폴더` 를 넣자 그대로 터졌다.

```
panicked at src\app.rs:655:
end byte index 4 is not a char boundary; it is inside '작' (bytes 3..6)
```

문자 단위로 비교하는 `starts_with_ci` 로 바꿨다. TUI 에서 그대로 크래시했을 버그다.

### `~` 는 셸이 아니라 우리가 푼다

`~/foo` 는 셸이 풀어주는 표기다. 우리 입력창에는 **문자 그대로** 들어오므로
`is_dir()` 이 거짓이 된다. `expand_tilde` 가 선두의 `~` 만 홈으로 바꾼다.
`~notme` 처럼 다른 사용자를 뜻하는 표기는 손대지 않는다 — 윈도우에서 풀 방법이 없다.

후행 구분자도 지킨다. `~/` 는 「홈 **안**을 보여달라」는 뜻인데, 확장하면
`C:\Users\HJS` 가 되어 구분자가 사라지고 홈의 *이름*을 완성하려 든다.

실측:

```
~/                  -> C:\Users\HJS\        57개 후보
~/Desk              -> C:\Users\HJS\Desktop\
~/D                 -> C:\Users\HJS\D       4개 후보 — DataGripProjects, Desktop, …
C:/Users/HJS/Idea   -> C:/Users/HJS/Idea    2개 후보 — IdeaProjects, IdeaSnapshots
~/없는폴더          -> (그대로)             일치하는 디렉터리가 없다
```

`--complete` 로 화면 없이 확인할 수 있다.

### [전체] 탭

세션이 넷을 넘으면 탭으로 바뀌는데, 그러면 한 번에 하나만 보인다. 탭 줄 맨 앞의
`[전체]` 가 전부 나란히 보여준다. 좁아지는 것은 감수하는 선택이므로 **기본값이
아니라 사용자가 켜는 값**으로 뒀다. 특정 에이전트 탭을 고르면 자동으로 꺼진다.

## §10.9 — `\?\` verbatim 접두사

새 공간을 추가하면 경로가 `\?\C:\…` 로 보였다. 우리가 붙인 게 아니라
`canonicalize()` 가 붙인 것이다. 윈도우에서 `std::fs::canonicalize` 는 260자 길이
제한을 넘는 경로까지 다루려고 **verbatim 접두사**를 붙여 돌려준다.

우리에겐 해가 된다.

- 화면 최상단과 spaces 목록에 그대로 나온다
- 자식의 작업 디렉터리로 넘어가는데, 이 표기를 못 읽는 프로그램이 있다

`strip_verbatim` 이 접두사만 뗀다. 다만 **드라이브 경로일 때만** 뗀다 —
`\?\PhysicalDrive0` 같은 장치 경로는 접두사를 떼면 뜻이 달라진다.
네트워크 경로 `\?\UNC\server\share` 는 `\server\share` 로 되돌린다.

canonicalize 자체는 남겨 뒀다. 심볼릭 링크를 풀고 절대 경로로 만들어 주어야
같은 공간을 두 번 추가하는 것을 막을 수 있다.

## §10.10 — 대화상자와 프롬프트 주입이 부딪히던 문제 (실측)

### 증상과 실제 원인

「새 공간에서 질문하면 화살표와 Enter 가 안 먹는다.」

먼저 화살표 인코딩을 의심했다. 자식이 application cursor 모드(DECCKM)를 켜면
`ESC[A` 가 아니라 `ESCOA` 를 보내야 하기 때문이다. 재 봤더니 아니었다.

```
application_cursor=false  keypad=false
```

PTY 로 직접 `ESC[B` 를 보내니 선택 표시가 정확히 움직였다.

```
보내기 전:   ❯ Continue without using this MCP server
아래 화살표: ❯ Use this MCP server          ← 움직인다
위 화살표:   ❯ Continue without using…      ← 되돌아온다
```

**키는 멀쩡했다. 우리가 사용자보다 먼저 답하고 있었다.**

새 경로에서 처음 뜨는 에이전트는 신뢰 여부나 MCP 사용을 먼저 묻는다. 그런데
우리 주입 규칙은 「출력이 600ms 잠잠해지면 넣는다」였고, 상자가 떠 있는 상태가
바로 그 «잠잠함»이다. 실측:

```
주입 시점: 1.5s  (상자가 떠 있는 채로)
주입 직전:   ❯ Continue without using this MCP server
             Enter to confirm · Esc to cancel
주입 6초 후: ❯ Try "fix typecheck errors"    ← 질문이 통째로 사라졌다
```

우리가 보낸 개행이 하이라이트된 항목을 확정했고 질문은 삼켜졌다. 사용자 눈에는
「누르기도 전에 상자가 닫히는」 것으로 보인다.

### 고친 방식

`modal.rs` 가 상자 바닥의 안내 문구(`Enter to confirm`, `Esc to cancel`, …)를 보고
상자가 떠 있는지 판정한다. 떠 있으면 **주입을 미룬다.** 사용자가 직접 답하고,
답이 끝나 화면이 잠잠해지면 그때 들어간다. 검증:

```
준비 시점: 1.4s
>> 대화상자 감지 — 주입 보류
>> (사용자 대신 ESC[B, Enter 로 답함)
>> 상자 남아 있나: false
주입 시점: 7.0s
결과: ❯ 1+1은? 숫자만 답해줘  /  ● 2      ← 질문이 살아남았다
```

형식이 바뀌어 상자를 못 알아보면 **예전 동작(바로 주입)으로 돌아갈 뿐** 새 고장이
생기지는 않는다.

사용자가 그 칸에 **직접 글자를 친 경우**에도 주입을 취소한다. 다만 화살표·Enter 는
세지 않는다 — 상자에 답하는 조작이지 「직접 쓰기」가 아니기 때문이다. 이걸 구분하지
않으면 상자에 답한 사용자가 질문을 잃는다.

## §10.11 — 시작 화면의 참여자 선택과 Ctrl+A

시작 화면에 체크박스를 뒀다. Space 가 입력창에서는 띄어쓰기, 체크박스에서는
켜기/끄기라 **같은 키가 두 뜻**을 가진다. 그래서 지금 어디에 있는지를 테두리 색과
반전 강조로 분명히 하고, Tab 으로만 오가게 했다. 체크박스에서 글자를 치면 입력창으로
돌아가며 친 글자를 살린다.

`Ctrl+A` 는 보이는 모든 에이전트에게 한 번에 묻는다. 직접 쓰지 않고 **대기 프롬프트로
걸어 둔다** — 그래야 위의 대화상자 규칙이 그대로 적용된다.

`Ctrl+A` 는 자식들도 쓰는 키다(줄 맨 앞으로). 가로챈 대신 `Ctrl+]` 다음 `a` 로
자식에게 보낼 길을 열어 뒀다.
