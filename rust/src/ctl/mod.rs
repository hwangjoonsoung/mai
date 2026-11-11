use serde::{Deserialize, Serialize};
pub mod models;
#[cfg(unix)] pub mod server;
#[cfg(unix)] mod client;

/// 질문을 받을 칸 하나. 에이전트와 (선택) 모델.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Target { pub agent: String, #[serde(default, skip_serializing_if="Option::is_none")] pub model: Option<String> }

/// 제어 소켓 요청. `op` 는 `ask` · `status` · `screen`.
#[derive(Serialize, Deserialize, Debug)]
pub struct Request {
    pub op: String,
    /// 요청자 칸 id(MAI_PANE). status/screen 은 0 이어도 된다.
    pub pane: u64,
    pub role: String,
    /// 하위 호환: 첫 대상. `targets` 가 비어 있으면 이걸로 하나짜리 그룹을 만든다.
    #[serde(default)] pub agent: String,
    #[serde(default)] pub question: String,
    pub timeout_secs: Option<u64>,
    #[serde(default)] pub keep: bool,
    #[serde(default, skip_serializing_if="Option::is_none")] pub model: Option<String>,
    /// 여러 에이전트에 동시에 묻는다. 순서대로 답을 합친다.
    #[serde(default, skip_serializing_if="Vec::is_empty")] pub targets: Vec<Target>,
    /// `screen` 이 볼 칸 id.
    #[serde(default, skip_serializing_if="Option::is_none")] pub target_pane: Option<u64>,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct Response { pub ok: bool, #[serde(skip_serializing_if="Option::is_none")] pub answer: Option<String>, #[serde(skip_serializing_if="Option::is_none")] pub origin: Option<String>, #[serde(skip_serializing_if="Option::is_none")] pub error: Option<String> }
impl Response {
    pub fn error(e: impl ToString) -> Self { Self { ok: false, answer: None, origin: None, error: Some(e.to_string()) } }
    pub fn answer(text: impl ToString) -> Self { Self { ok: true, answer: Some(text.to_string()), origin: None, error: None } }
}
impl Request {
    /// 질문을 받을 칸 목록. `targets` 가 비면 `agent`/`model` 하나.
    pub fn targets(&self) -> Vec<Target> {
        if self.targets.is_empty() { vec![Target { agent: self.agent.clone(), model: self.model.clone() }] } else { self.targets.clone() }
    }
}

/// `status` 응답의 칸 하나.
#[derive(Serialize, Deserialize, Debug)]
pub struct PaneStatus { pub id: u64, pub title: String, pub agent: String, pub role: String, pub verifier_of: Option<u64>, pub model: Option<String>, pub cwd: String, pub alive: bool, pub pending: bool }
/// `status` 응답의 진행 중 요청 하나.
#[derive(Serialize, Deserialize, Debug)]
pub struct AskStatus { pub requester: u64, pub verifier: u64, pub agent: String, pub model: Option<String>, pub elapsed_secs: u64, pub remaining_secs: Option<u64>, pub injected: bool }
#[derive(Serialize, Deserialize, Debug)]
pub struct Status { pub panes: Vec<PaneStatus>, pub asks: Vec<AskStatus> }

pub fn normalize(name: &str) -> Option<&'static str> { crate::AGENTS.iter().find(|(id, title)| name.eq_ignore_ascii_case(id) || name.eq_ignore_ascii_case(title)).map(|(id, _)| *id) }
pub fn display(agent: &str) -> &'static str { crate::AGENTS.iter().find(|(id, _)| *id == agent).map(|(_, t)| *t).unwrap_or("?") }

/// `codex:sol,claude` 같은 대상 목록을 푼다. 모델이 없으면 `default_model`.
pub fn parse_targets(spec: &str, default_model: Option<&str>) -> Option<Vec<Target>> {
    let mut out = Vec::new();
    for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (name, model) = match item.split_once(':') { Some((n, m)) if !m.trim().is_empty() => (n.trim(), Some(m.trim().to_string())), Some(_) => return None, None => (item, None) };
        let agent = normalize(name)?.to_string();
        let model = model.or_else(|| default_model.map(str::to_string));
        if out.iter().any(|t: &Target| t.agent == agent && t.model == model) { continue; }
        out.push(Target { agent, model });
    }
    if out.is_empty() { None } else { Some(out) }
}

/// 여러 대상의 답을 하나로 합친다. 대상이 하나면 그 답 그대로.
/// 실패한 대상은 «검증하지 못했다» 로 표시하고 전체 ok 는 모두 성공일 때만.
pub fn combine(results: &[(Target, Response)]) -> Response {
    if let [(_, r)] = results { return Response { ok: r.ok, answer: r.answer.clone(), origin: r.origin.clone(), error: r.error.clone() }; }
    let mut answer = String::new(); let mut failed = Vec::new();
    for (t, r) in results {
        let label = match &t.model { Some(m) => format!("{}({m})", display(&t.agent)), None => display(&t.agent).to_string() };
        if !answer.is_empty() { answer.push_str("\n\n"); }
        answer.push_str(&format!("## {label}\n"));
        match (&r.answer, &r.error) {
            (Some(a), _) if r.ok => answer.push_str(a),
            (_, e) => { let e = e.clone().unwrap_or_else(|| "검증 실패".into()); answer.push_str(&format!("(검증하지 못했다: {e})")); failed.push(format!("{label}: {e}")); }
        }
    }
    Response { ok: failed.is_empty(), answer: Some(answer), origin: None, error: if failed.is_empty() { None } else { Some(failed.join(" · ")) } }
}

pub fn run(args: &[String]) -> i32 {
    if args == ["instructions"] { print!("{}", include_str!("../../../prompts/verify.md")); return 0; }
    // `mai ctl model <에이전트> <별칭>` — 검증 칸에 어떤 모델 이름이 넘어가는지 미리 본다.
    if args.first().map(String::as_str) == Some("model") {
        let (Some(agent), Some(alias), None) = (args.get(1), args.get(2), args.get(3)) else { eprintln!("사용법: mai ctl model <에이전트> <별칭>"); return 2; };
        let Some(agent) = normalize(agent) else { eprintln!("알 수 없는 에이전트: {agent}"); return 2; };
        println!("{}", models::resolve(agent, alias)); return 0;
    }
    #[cfg(unix)] { client::run(args) }
    #[cfg(not(unix))] { eprintln!("이 플랫폼에서는 mai ctl을 지원하지 않는다"); 2 }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn protocol_roundtrip() {
        let r = Request { op: "ask".into(), pane: 7, role: "main".into(), agent: "codex".into(), question: "질문\n두 줄".into(), timeout_secs: None, keep: true, model: Some("gpt-5".into()), targets: vec![], target_pane: None };
        let decoded: Request = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(decoded.question, r.question); assert!(decoded.keep); assert_eq!(decoded.model.as_deref(), Some("gpt-5"));
        assert_eq!(decoded.targets(), vec![Target { agent: "codex".into(), model: Some("gpt-5".into()) }]);
        let old: Request = serde_json::from_str(r#"{"op":"ask","pane":1,"role":"main","agent":"codex","question":"q"}"#).unwrap(); assert!(!old.keep); assert!(old.model.is_none());
        let e: Response = serde_json::from_str(&serde_json::to_string(&Response::error("오류")).unwrap()).unwrap(); assert!(!e.ok);
        assert!(serde_json::from_str::<Request>("broken").is_err());
    }
    #[test] fn 대상_목록을_푼다() {
        let t = parse_targets("codex:sol, Claude ,gemini", Some("x")).unwrap();
        assert_eq!(t.iter().map(|t| (t.agent.as_str(), t.model.as_deref())).collect::<Vec<_>>(), vec![("codex", Some("sol")), ("claude", Some("x")), ("agy", Some("x"))]);
        assert_eq!(parse_targets("codex,codex", None).unwrap().len(), 1);
        assert!(parse_targets("nope", None).is_none()); assert!(parse_targets("codex:", None).is_none()); assert!(parse_targets("", None).is_none());
    }
    #[test] fn 여러_답을_합친다() {
        let t = |a: &str, m: Option<&str>| Target { agent: a.into(), model: m.map(Into::into) };
        let one = combine(&[(t("codex", None), Response::answer("2"))]);
        assert!(one.ok); assert_eq!(one.answer.as_deref(), Some("2"));
        let many = combine(&[(t("codex", Some("sol")), Response::answer("2")), (t("claude", None), Response::error("시간 초과"))]);
        assert!(!many.ok);
        assert_eq!(many.answer.as_deref(), Some("## Codex(sol)\n2\n\n## Claude\n(검증하지 못했다: 시간 초과)"));
        assert_eq!(many.error.as_deref(), Some("Claude: 시간 초과"));
    }
}
