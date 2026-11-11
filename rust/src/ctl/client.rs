use super::*;
use std::io::{BufRead, BufReader, Read, Write};
pub fn run(args: &[String]) -> i32 {
    let result = execute(args);
    match result { Ok(answer) => { println!("{answer}"); 0 }, Err((code, error)) => { eprintln!("{error}"); code } }
}
fn execute(args: &[String]) -> Result<String, (i32, String)> {
    let bad = || (2, "사용법: mai ctl ask <에이전트> <질문|-> [--timeout|-t 초] [--keep|-k] [--model|-m 이름]  (옵션 위치는 자유)".into());
    if args.first().map(String::as_str) != Some("ask") { return Err(bad()); }
    // 옵션은 에이전트 이름 앞뒤 어디에 와도 된다. `mai ctl ask codex -m sol -k "..."` 처럼
    // 자연스럽게 치게 되기 때문이다. 위치 인자는 정확히 <에이전트> <질문> 둘이다.
    let mut timeout = None; let mut keep = false; let mut model = None; let mut positional = Vec::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--timeout" | "-t" => { timeout = Some(args.get(i+1).ok_or_else(bad)?.parse::<u64>().map_err(|_| bad())?); i += 2; }
            "--keep" | "-k" => { keep = true; i += 1; }
            "--model" | "-m" => { let m = args.get(i+1).ok_or_else(bad)?; if m.trim().is_empty() || m.starts_with('-') { return Err(bad()); } model = Some(m.clone()); i += 2; }
            "--" => { positional.extend(args[i+1..].iter().cloned()); break; }
            a if a.starts_with('-') && a != "-" => return Err(bad()),
            _ => { positional.push(args[i].clone()); i += 1; }
        }
    }
    let [agent_name, question] = positional.as_slice() else { return Err(bad()) };
    let agent = normalize(agent_name).ok_or_else(bad)?;
    let path = std::env::var_os("MAI_CTL").ok_or((2, "mai 안에서 실행해야 한다".into()))?;
    let pane = std::env::var("MAI_PANE").ok().and_then(|s| s.parse().ok()).ok_or((2, "mai 안에서 실행해야 한다".into()))?;
    let mut question = question.clone();
    if question == "-" { question.clear(); std::io::stdin().read_to_string(&mut question).map_err(|e| (2, e.to_string()))?; }
    if question.trim().is_empty() { return Err(bad()); }
    let req = Request { op: "ask".into(), pane, role: std::env::var("MAI_ROLE").unwrap_or_else(|_| "main".into()), agent: agent.into(), question, timeout_secs: timeout, keep, model };
    let mut stream = std::os::unix::net::UnixStream::connect(path).map_err(|e| (1, e.to_string()))?;
    let bytes = serde_json::to_string(&req).map_err(|e| (2, e.to_string()))?;
    writeln!(stream, "{bytes}").map_err(|e| (1, e.to_string()))?;
    let mut line = String::new(); BufReader::new(stream).read_line(&mut line).map_err(|e| (1, e.to_string()))?;
    let response: Response = serde_json::from_str(&line).map_err(|e| (1, e.to_string()))?;
    if response.ok { response.answer.ok_or((1, "응답에 답이 없다".into())) } else { Err((1, response.error.unwrap_or_else(|| "검증 실패".into()))) }
}
