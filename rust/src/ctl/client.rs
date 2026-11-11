use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use super::transport::Endpoint;

const USAGE: &str = "사용법: mai ctl ask <에이전트[:모델][,에이전트...]> <질문|-> [--timeout|-t 초] [--keep|-k] [--model|-m 이름]  (옵션 위치는 자유)\n      mai ctl status | mai ctl screen <칸 id> | mai ctl history [건수|id] | mai ctl model <에이전트> <별칭> | mai ctl instructions";

pub fn run(args: &[String]) -> i32 {
    match execute(args) {
        Ok(text) => { println!("{text}"); 0 }
        Err((code, error, partial)) => { if let Some(p) = partial { println!("{p}"); } eprintln!("{error}"); code }
    }
}

type Failure = (i32, String, Option<String>);

fn execute(args: &[String]) -> Result<String, Failure> {
    let bad = || (2, USAGE.to_string(), None);
    match args.first().map(String::as_str) {
        Some("ask") => ask(args),
        Some("status") if args.len() == 1 => {
            let r = send(&Request { op: "status".into(), pane: pane_env().unwrap_or(0), role: role_env(), agent: String::new(), question: String::new(), timeout_secs: None, keep: false, model: None, targets: vec![], target_pane: None, token: None })?;
            let status: Status = serde_json::from_str(r.answer.as_deref().unwrap_or("")).map_err(|e| (1, e.to_string(), None))?;
            Ok(render_status(&status))
        }
        Some("screen") if args.len() == 2 => {
            let id = args[1].parse().map_err(|_| bad())?;
            send(&Request { op: "screen".into(), pane: pane_env().unwrap_or(0), role: role_env(), agent: String::new(), question: String::new(), timeout_secs: None, keep: false, model: None, targets: vec![], target_pane: Some(id), token: None }).map(|r| r.answer.unwrap_or_default())
        }
        _ => Err(bad()),
    }
}

fn ask(args: &[String]) -> Result<String, Failure> {
    let bad = || (2, USAGE.to_string(), None);
    // 옵션은 에이전트 이름 앞뒤 어디에 와도 된다. `mai ctl ask codex -m sol -k "..."` 처럼
    // 자연스럽게 치게 되기 때문이다. 위치 인자는 정확히 <에이전트> <질문> 둘이다.
    let mut timeout = None; let mut keep = false; let mut model: Option<String> = None; let mut positional = Vec::new();
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
    let [spec, question] = positional.as_slice() else { return Err(bad()) };
    let targets = parse_targets(spec, model.as_deref()).ok_or_else(bad)?;
    let pane = pane_env().ok_or((2, "mai 안에서 실행해야 한다".into(), None))?;
    let mut question = question.clone();
    if question == "-" { question.clear(); std::io::stdin().read_to_string(&mut question).map_err(|e| (2, e.to_string(), None))?; }
    if question.trim().is_empty() { return Err(bad()); }
    let first = targets[0].clone();
    let req = Request { op: "ask".into(), pane, role: role_env(), agent: first.agent, question, timeout_secs: timeout, keep, model: first.model, targets, target_pane: None, token: None };
    let response = send(&req)?;
    // 성공하면 칸별 마커를 남긴다. hooks/mai-verify-stop.py 가 «이번 턴에 검증했는가» 를 이걸로 본다.
    if response.ok { touch_verified(pane); }
    // 여러 대상 중 일부만 실패하면 합친 답은 stdout 으로 주고, 실패 목록은 stderr · 종료 코드 1.
    if response.ok { response.answer.ok_or((1, "응답에 답이 없다".into(), None)) }
    else { Err((1, response.error.unwrap_or_else(|| "검증 실패".into()), response.answer)) }
}

/// `<MAI_CTL 디렉터리>/verified-<pane>` 의 mtime 을 지금으로. 실패해도 조용히.
fn touch_verified(pane: u64) {
    let dir = match std::env::var_os("MAI_CTL_DIR") {
        Some(d) => std::path::PathBuf::from(d),
        None => match std::env::var_os("MAI_CTL").and_then(|c| std::path::Path::new(&c).parent().map(|p| p.to_path_buf())) { Some(d) => d, None => return },
    };
    let _ = std::fs::write(dir.join(format!("verified-{pane}")), b"");
}

fn pane_env() -> Option<u64> { std::env::var("MAI_PANE").ok().and_then(|s| s.parse().ok()) }
fn role_env() -> String { std::env::var("MAI_ROLE").unwrap_or_else(|_| "main".into()) }

fn send(req: &Request) -> Result<Response, Failure> {
    let endpoint = Endpoint::from_env().ok_or((2, "mai 안에서 실행해야 한다".into(), None))?;
    let mut stream = endpoint.connect().map_err(|e| (1, e.to_string(), None))?;
    let mut req = Request { token: endpoint.token().map(str::to_string), ..clone_request(req) };
    if req.token.as_deref() == Some("") { req.token = None; }
    let bytes = serde_json::to_string(&req).map_err(|e| (2, e.to_string(), None))?;
    writeln!(stream, "{bytes}").map_err(|e| (1, e.to_string(), None))?;
    let mut line = String::new(); BufReader::new(stream).read_line(&mut line).map_err(|e| (1, e.to_string(), None))?;
    let response: Response = serde_json::from_str(&line).map_err(|e| (1, e.to_string(), None))?;
    if response.ok || req.op == "ask" { Ok(response) } else { Err((1, response.error.unwrap_or_else(|| "실패".into()), None)) }
}

fn clone_request(r: &Request) -> Request {
    Request { op: r.op.clone(), pane: r.pane, role: r.role.clone(), agent: r.agent.clone(), question: r.question.clone(), timeout_secs: r.timeout_secs, keep: r.keep, model: r.model.clone(), targets: r.targets.clone(), target_pane: r.target_pane, token: r.token.clone() }
}

fn render_status(s: &Status) -> String {
    let mut out = String::from("칸\n id  역할      에이전트  모델             상태   제목\n");
    for p in &s.panes {
        let state = if !p.alive { "종료" } else if p.pending { "대기" } else { "실행" };
        out.push_str(&format!(" {:<3} {:<9} {:<9} {:<16} {:<5} {}\n", p.id, p.role, p.agent, p.model.as_deref().unwrap_or("-"), state, p.title));
    }
    out.push_str("\n진행 중 요청\n");
    if s.asks.is_empty() { out.push_str(" (없음)\n"); }
    for a in &s.asks {
        let left = a.remaining_secs.map_or("무제한".to_string(), |r| format!("{r}초 남음"));
        out.push_str(&format!(" 칸 {} → 칸 {} ({}{}) {}초 경과 · {} · {}\n", a.requester, a.verifier, a.agent, a.model.as_deref().map(|m| format!(":{m}")).unwrap_or_default(), a.elapsed_secs, left, if a.injected { "답 기다리는 중" } else { "질문 넣기 전" }));
    }
    out.trim_end().to_string()
}
