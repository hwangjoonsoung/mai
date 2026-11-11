use serde::{Deserialize, Serialize};
pub mod models;
#[cfg(unix)] pub mod server;
#[cfg(unix)] mod client;
#[derive(Serialize, Deserialize, Debug)]
pub struct Request { pub op: String, pub pane: u64, pub role: String, pub agent: String, pub question: String, pub timeout_secs: Option<u64>, #[serde(default)] pub keep: bool, #[serde(default, skip_serializing_if="Option::is_none")] pub model: Option<String> }
#[derive(Serialize, Deserialize, Debug)]
pub struct Response { pub ok: bool, #[serde(skip_serializing_if="Option::is_none")] pub answer: Option<String>, #[serde(skip_serializing_if="Option::is_none")] pub origin: Option<String>, #[serde(skip_serializing_if="Option::is_none")] pub error: Option<String> }
impl Response {
    pub fn error(e: impl ToString) -> Self { Self { ok: false, answer: None, origin: None, error: Some(e.to_string()) } }
}
pub fn normalize(name: &str) -> Option<&'static str> { crate::AGENTS.iter().find(|(id, title)| name.eq_ignore_ascii_case(id) || name.eq_ignore_ascii_case(title)).map(|(id, _)| *id) }
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
        let r = Request { op: "ask".into(), pane: 7, role: "main".into(), agent: "codex".into(), question: "질문\n두 줄".into(), timeout_secs: None, keep: true, model: Some("gpt-5".into()) };
        let decoded: Request = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(decoded.question, r.question); assert!(decoded.keep); assert_eq!(decoded.model.as_deref(), Some("gpt-5"));
        let old: Request = serde_json::from_str(r#"{"op":"ask","pane":1,"role":"main","agent":"codex","question":"q"}"#).unwrap(); assert!(!old.keep); assert!(old.model.is_none());
        let e: Response = serde_json::from_str(&serde_json::to_string(&Response::error("오류")).unwrap()).unwrap(); assert!(!e.ok);
        assert!(serde_json::from_str::<Request>("broken").is_err());
    }
}
