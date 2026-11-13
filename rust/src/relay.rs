//! 한 칸의 마지막 답변을 꺼내 다른 칸으로 넘긴다.
//!
//! 세션은 공유할 수 없다. 세 CLI 는 서로 다른 서버·포맷·인증을 쓰고, API 키
//! 경로는 INTENT §3.1 이 막아 놨다. 그래서 **답변만 옮긴다.**
//!
//! 꺼내는 곳은 두 군데다. 순서가 곧 우선순위다.
//!
//!   1순위  에이전트가 스스로 남긴 세션 기록 (JSONL). **전문이고 잘림이 없다.**
//!   2순위  우리가 들고 있는 화면 버퍼. 보이는 만큼만.
//!
//! 2순위가 왜 열등한가 — 에이전트는 대체 화면(`ESC[?1049h`)에서 돈다. vt100 은
//! 대체 화면에 스크롤백을 두지 않으므로(`Grid::new(size, 0)`) 칸에 보이는
//! 것이 전부다. 3분할에서 한 칸은 60컬럼 남짓이라 긴 답은 앞부분이 없다.
//!
//! **JSONL 포맷은 비공개 내부 규약이다.** CLI 가 올라가면 깨질 수 있다. 그래서
//! 실패를 오류로 다루지 않고 조용히 2순위로 내려간다. 어느 경로로 가져왔는지는
//! 상자에 표시해 사용자가 알 수 있게 한다.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// 인용문을 어디서 가져왔는가. 사용자에게 그대로 보여준다.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// 에이전트의 세션 기록 파일 — 전문
    Session,
    /// 우리 화면 버퍼 — 보이는 만큼
    Screen,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::Session => "세션 기록",
            Origin::Screen => "화면",
        }
    }
}

pub struct Quote {
    pub text: String,
    pub origin: Origin,
    /// 길이 상한에 걸려 앞을 잘랐는가
    pub truncated: bool,
}

/// 한 번에 넘길 수 있는 최대 길이.
///
/// 상한을 두는 이유는 상대의 컨텍스트를 통째로 먹지 않기 위해서다. 넘치면
/// **뒤를 남긴다** — 결론이 뒤에 있기 때문이다.
const MAX_CHARS: usize = 20_000;

/// `agent_id` 칸의 마지막 답변을 찾는다. 못 찾으면 None.
pub fn last_answer(agent_id: &str, cwd: &Path) -> Option<Quote> {
    let text = match agent_id {
        "claude" => claude_last(cwd, None),
        "codex" => codex_last(cwd, None),
        // agy 는 기록 위치가 아직 확인되지 않았다. 화면으로 떨어진다.
        _ => None,
    }?;
    Some(clamp(text, Origin::Session))
}

/// 화면 버퍼에서 긁는다. 세션 기록을 못 찾았을 때의 폴백이다.
pub fn from_screen(screen: &vt100::Screen) -> Quote {
    let raw = screen.contents();
    // 아래쪽 빈 줄과 우측 공백을 턴다. 칸 대부분이 공백인 경우가 많다.
    let lines: Vec<&str> = raw.lines().map(|l| l.trim_end()).collect();
    let end = lines.iter().rposition(|l| !l.is_empty()).map_or(0, |i| i + 1);
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(0);
    let text = lines[start..end].join("\n");
    clamp(text, Origin::Screen)
}

fn clamp(text: String, origin: Origin) -> Quote {
    let n = text.chars().count();
    if n <= MAX_CHARS {
        return Quote { text, origin, truncated: false };
    }
    let cut: String = text.chars().skip(n - MAX_CHARS).collect();
    Quote { text: cut, origin, truncated: true }
}

/// 넘길 본문을 만든다.
///
/// 여러 줄이라 **브래킷 붙여넣기로 감싼다.** 그냥 쓰면 첫 줄바꿈에서 상대가
/// 전송해 버려 인용문 한 줄만 질문으로 들어간다(에이전트들은 `ESC[?2004h` 로
/// 붙여넣기 모드를 켜 둔다 — 실측).
pub fn compose(source: &str, quote: &Quote, sentence: &str) -> String {
    let mut body = format!("[{source} 의 답변]\n\"\"\"\n");
    if quote.truncated {
        body.push_str("(앞부분 생략)\n");
    }
    body.push_str(&quote.text);
    body.push_str("\n\"\"\"\n\n");
    body.push_str(sentence);
    format!("\u{1b}[200~{body}\u{1b}[201~")
}

// ---------- claude ----------

/// `~/.claude/projects/<이스케이프한 cwd>/<uuid>.jsonl`
///
/// 디렉터리 이름은 경로에서 **영숫자와 하이픈이 아닌 문자를 전부 `-` 로** 바꾼
/// 것이다(실측: `/Users/hj/intelliJ/multi_ai_x` → `-Users-hj-intelliJ-multi-ai-x`,
/// `~/.claude/skills` → `-Users-hj--claude-skills`).
fn claude_last(cwd: &Path, since: Option<std::time::SystemTime>) -> Option<String> {
    let dir = home()?.join(".claude").join("projects").join(escape(cwd));
    let file = if since.is_none() { newest(&dir, "jsonl")? } else {
        let mut files = Vec::new(); collect(&dir, "jsonl", 1, &mut files);
        files.into_iter().filter(|(p, _)| created_since(p, since)).max_by_key(|(_, m)| *m)?.0
    };
    let text = fs::read_to_string(file).ok()?;

    let mut last = None;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        // 서브에이전트가 남긴 것은 건너뛴다. 사용자가 본 답이 아니다.
        if v.get("isSidechain").and_then(|b| b.as_bool()) == Some(true) {
            continue;
        }
        let joined = join_text(v.pointer("/message/content"), "text", "text");
        if !joined.trim().is_empty() {
            last = Some(joined);
        }
    }
    last
}

// ---------- codex ----------

/// `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`
///
/// 경로에 cwd 가 없다. 첫 줄의 `session_meta` 가 `cwd` 를 들고 있어 그걸로
/// 가른다. 첫 줄만 읽으므로 후보가 많아도 싸다.
fn codex_last(cwd: &Path, since: Option<std::time::SystemTime>) -> Option<String> {
    let root = home()?.join(".codex").join("sessions");
    let mut files = Vec::new();
    collect(&root, "jsonl", 5, &mut files);
    files.sort_by_key(|(_, m)| std::cmp::Reverse(*m));

    for (path, _) in files {
        // **첫 줄만 읽어 거른다.** 전부 읽어 놓고 버리면 세션 기록 전체를
        // 훑게 된다 — 실측으로 358개 220MB 였다. 이건 키를 누른 순간 도는
        // 코드라 그 비용을 낼 수 없다.
        if !created_since(&path, since) || !first_line_accepts(&path, cwd) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        return codex_answer_in(&text);
    }
    None
}

/// 기록 본문에서 마지막 답. `phase` 가 `final_answer` 인 메시지를 우선한다.
///
/// codex 는 작업 도중 «서브에이전트가 검색 중이다» 같은 중간 보고를
/// `phase: commentary` 로 남긴다. 그걸 답으로 집으면 안 된다. `phase` 가 없는
/// 옛 기록은 마지막 assistant 메시지로 떨어진다.
fn codex_answer_in(text: &str) -> Option<String> {
    let (mut last_final, mut last_any) = (None, None);
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if v.get("type").and_then(|t| t.as_str()) != Some("response_item") {
            continue;
        }
        // `?` 를 쓰면 payload 없는 레코드 하나에 스캔 전체가 끝난다.
        let Some(p) = v.get("payload") else { continue };
        if p.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        let joined = join_text(p.get("content"), "output_text", "text");
        if joined.trim().is_empty() {
            continue;
        }
        if p.get("phase").and_then(|s| s.as_str()) == Some("final_answer") {
            last_final = Some(joined.clone());
        }
        last_any = Some(joined);
    }
    last_final.or(last_any)
}

/// 파일의 첫 줄(`session_meta`)이 이 cwd 의 **주 스레드**인가.
fn first_line_accepts(path: &Path, cwd: &Path) -> bool {
    use std::io::{BufRead, BufReader};
    let Ok(f) = fs::File::open(path) else { return false };
    let mut line = String::new();
    if BufReader::new(f).read_line(&mut line).is_err() {
        return false;
    }
    let Ok(meta) = serde_json::from_str::<serde_json::Value>(&line) else { return false };
    meta_accepts(&meta, cwd)
}

/// cwd 가 맞고 서브에이전트 기록이 아니어야 한다.
///
/// codex 서브에이전트는 같은 cwd 로 **자기 rollout 파일을 따로** 만든다
/// (`source: {"subagent": {"thread_spawn": …}}` 와 `parent_thread_id`). 실측:
/// 토픽 10개를 서브에이전트 10개에 맡기자 파일 11개가 생겼고, mtime 순으로 고르면
/// 마지막 서브에이전트의 답(기사 1건)이 주 스레드의 답(10건)을 가렸다.
/// Windows 는 열려 있는 파일의 mtime 을 바로 갱신하지 않아 순서로는 못 가른다.
fn meta_accepts(meta: &serde_json::Value, cwd: &Path) -> bool {
    let subagent = meta.pointer("/payload/source/subagent").is_some()
        || meta.pointer("/payload/parent_thread_id").is_some_and(|p| !p.is_null());
    !subagent && meta.pointer("/payload/cwd").and_then(|c| c.as_str()).map(Path::new) == Some(cwd)
}

// ---------- 공통 ----------

/// content 배열에서 `kind` 인 블록의 `field` 를 이어 붙인다.
///
/// tool_use 같은 블록은 걸러진다. 사람이 읽는 답만 넘긴다.
fn join_text(content: Option<&serde_json::Value>, kind: &str, field: &str) -> String {
    let Some(arr) = content.and_then(|c| c.as_array()) else { return String::new() };
    arr.iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some(kind))
        .filter_map(|b| b.get(field).and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 경로를 claude 의 디렉터리 이름으로 바꾼다.
fn escape(p: &Path) -> String {
    p.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect()
}

/// 디렉터리에서 확장자가 맞는 가장 최근 파일.
fn newest(dir: &Path, ext: &str) -> Option<PathBuf> {
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for e in fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some(ext) {
            continue;
        }
        let Ok(m) = e.metadata().and_then(|m| m.modified()) else { continue };
        if best.as_ref().is_none_or(|(_, bm)| m > *bm) {
            best = Some((p, m));
        }
    }
    best.map(|(p, _)| p)
}

/// 깊이 제한 재귀 수집. (경로, 수정시각)
fn collect(dir: &Path, ext: &str, depth: usize, out: &mut Vec<(PathBuf, std::time::SystemTime)>) {
    if depth == 0 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, ext, depth - 1, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some(ext) {
            if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                out.push((p, m));
            }
        }
    }
}

fn home() -> Option<PathBuf> {
    crate::app::home_var().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_디렉터리_이름_규칙() {
        // 실측한 이름들과 맞춰 본다.
        assert_eq!(
            escape(Path::new("/Users/hwangjoonsoung/intelliJ/multi_ai_use_in_one_shell")),
            "-Users-hwangjoonsoung-intelliJ-multi-ai-use-in-one-shell"
        );
        assert_eq!(
            escape(Path::new("/Users/hwangjoonsoung/.claude/skills")),
            "-Users-hwangjoonsoung--claude-skills"
        );
        assert_eq!(escape(Path::new("/Users/hwangjoonsoung")), "-Users-hwangjoonsoung");
    }

    #[test]
    fn 긴_인용은_뒤를_남긴다() {
        let long: String = std::iter::repeat('가').take(MAX_CHARS + 500).collect();
        let q = clamp(long, Origin::Session);
        assert!(q.truncated);
        assert_eq!(q.text.chars().count(), MAX_CHARS);
    }

    #[test]
    fn 붙여넣기로_감싼다() {
        let q = Quote { text: "1줄\n2줄".into(), origin: Origin::Session, truncated: false };
        let out = compose("Claude", &q, "넌 어떻게 생각해?");
        assert!(out.starts_with("\u{1b}[200~"), "브래킷 붙여넣기로 시작해야 한다");
        assert!(out.ends_with("\u{1b}[201~"), "브래킷 붙여넣기로 끝나야 한다");
        assert!(out.contains("[Claude 의 답변]"));
        assert!(out.contains("넌 어떻게 생각해?"));
        assert!(out.contains("1줄\n2줄"), "줄바꿈이 살아 있어야 한다");
    }

    #[test]
    fn codex_서브에이전트_기록은_거른다() {
        let cwd = Path::new("/work");
        let main: serde_json::Value = serde_json::from_str(r#"{"type":"session_meta","payload":{"cwd":"/work","source":"vscode"}}"#).unwrap();
        let sub: serde_json::Value = serde_json::from_str(r#"{"type":"session_meta","payload":{"cwd":"/work","parent_thread_id":"p","source":{"subagent":{"thread_spawn":{"parent_thread_id":"p","depth":1}}}}}"#).unwrap();
        let other: serde_json::Value = serde_json::from_str(r#"{"type":"session_meta","payload":{"cwd":"/else","source":"cli"}}"#).unwrap();
        assert!(meta_accepts(&main, cwd));
        assert!(!meta_accepts(&sub, cwd), "서브에이전트 rollout 은 주 스레드의 답이 아니다");
        assert!(!meta_accepts(&other, cwd));
    }

    #[test]
    fn codex_중간_보고보다_최종_답을_고른다() {
        let line = |phase: &str, text: &str| format!(r#"{{"type":"response_item","payload":{{"type":"message","role":"assistant","phase":"{phase}","content":[{{"type":"output_text","text":"{text}"}}]}}}}"#);
        let text = [line("commentary", "찾는 중"), line("final_answer", "답 10건"), line("commentary", "다음 턴 작업 중")].join("\n");
        assert_eq!(codex_answer_in(&text).as_deref(), Some("답 10건"));
        let old = r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"옛 기록"}]}}"#;
        assert_eq!(codex_answer_in(old).as_deref(), Some("옛 기록"), "phase 가 없으면 마지막 메시지");
        assert_eq!(codex_answer_in("{\"type\":\"turn_context\"}"), None);
    }

    #[test]
    fn tool_use_블록은_빼고_텍스트만_잇는다() {
        let v: serde_json::Value = serde_json::from_str(
            r#"[{"type":"text","text":"앞"},{"type":"tool_use","name":"Bash"},{"type":"text","text":"뒤"}]"#,
        )
        .unwrap();
        assert_eq!(join_text(Some(&v), "text", "text"), "앞\n뒤");
    }
}

fn created_since(path: &Path, since: Option<std::time::SystemTime>) -> bool {
    since.map_or(true, |t| fs::metadata(path).and_then(|m| m.created()).is_ok_and(|c| c >= t))
}
pub fn last_answer_since(agent: &str, cwd: &Path, since: Option<std::time::SystemTime>) -> Option<Quote> {
    let text = match agent { "claude" => claude_last(cwd, since), "codex" => codex_last(cwd, since), _ => None }?;
    Some(clamp(text, Origin::Session))
}

/// agy(화면 기반) 칸에서 **마지막 질문 블록 뒤의 답만** 꺼낸다.
///
/// 화면 전체를 돌려주면 배너·질문·입력창까지 섞인다. 우리가 넣은 질문은
/// `"""` 로 감싸 두었으므로, 마지막 닫는 `"""` 뒤부터 입력창 테두리
/// (`─` 로 시작하는 줄) 앞까지가 답이다. `▸ Thought for …` 같은 생각 표시줄은
/// 뺀다. 질문 블록이 아직 안 보이면(스크롤·미주입) 화면 전체로 떨어진다.
pub fn agy_answer(screen_text: &str) -> String {
    let Some(end) = screen_text.rfind("\"\"\"") else { return screen_text.to_string() };
    let after = &screen_text[end + 3..];
    let mut out = Vec::new();
    for line in after.lines().skip(1) {
        let t = line.trim();
        if t.starts_with('─') { break; }
        if t.starts_with("▸ Thought") || t.starts_with("▸ Thinking") { continue; }
        out.push(line.trim_end());
    }
    let joined = out.join("\n");
    let start = joined.find(|c: char| !c.is_whitespace()).unwrap_or(joined.len());
    joined[start..].trim_end().to_string()
}

#[cfg(test)]
mod agy_tests {
    use super::agy_answer;
    #[test]
    fn 마지막_질문_뒤_답만_남긴다() {
        let screen = "배너\n> [zsh 의 검증 요청]\n  \"\"\"\n  1+1은?\n  \"\"\"\n\n▸ Thought for 3s, 151 tokens\n\n  2\n  둘째 줄\n\n──────\n>\n──────\n? for shortcuts";
        assert_eq!(agy_answer(screen), "2\n  둘째 줄");
        assert_eq!(agy_answer("아직 질문 없음"), "아직 질문 없음");
        assert_eq!(agy_answer("> q\n  \"\"\"\n  x\n  \"\"\"\n\n▸ Thought for 1s\n──────\n>"), "");
    }
}
