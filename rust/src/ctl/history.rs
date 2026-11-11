//! `mai ctl ask` 기록. 요청 1건 = 줄 1개의 JSONL 로 홈에 쌓는다.
//!
//!   ~/.multi-ai-cli/ctl_history.jsonl
//!
//! 방(room)의 transcript 와 섞지 않는 이유: 그쪽은 `--converge` 라운드 단위로
//! 만들어지고 길이 프레이밍을 쓴다. 검증 요청은 작업 공간을 가리지 않고
//! 수시로 일어나므로 한 파일에 시간순으로 붙이는 편이 찾기 쉽다.
//! 읽기는 소켓 없이 파일만 보므로 mai 밖에서도 `mai ctl history` 가 된다.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Outcome { pub agent: String, pub model: Option<String>, pub ok: bool, pub answer: Option<String>, pub error: Option<String>, pub origin: Option<String> }

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Record {
    /// 밀리초 단위 유닉스 시각. 조회 id 로도 쓴다.
    pub id: u64,
    pub time: String,
    pub cwd: String,
    pub requester: String,
    pub question: String,
    pub elapsed_secs: u64,
    pub keep: bool,
    pub results: Vec<Outcome>,
}

pub fn path() -> PathBuf { crate::room::home().join("ctl_history.jsonl") }

/// 줄 하나를 덧붙인다. 실패해도 요청 자체는 이미 끝났으므로 조용히 넘어간다.
pub fn append(record: &Record) {
    let Ok(line) = serde_json::to_string(record) else { return };
    let _ = std::fs::create_dir_all(crate::room::home());
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path()) { let _ = writeln!(f, "{line}"); }
}

pub fn now_id() -> (u64, String) {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    (ms, local_time(ms))
}

/// `YYYY-MM-DD HH:MM:SS` (현지 시각). 의존성 없이 libc 의 localtime_r 로 계산한다.
#[cfg(unix)]
fn local_time(ms: u64) -> String {
    let t = (ms / 1000) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r 는 주어진 tm 버퍼만 채운다.
    unsafe { libc::localtime_r(&t, &mut tm); }
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}
#[cfg(not(unix))]
fn local_time(ms: u64) -> String { format!("@{}", ms / 1000) }

pub fn read_all() -> Vec<Record> {
    let Ok(text) = std::fs::read_to_string(path()) else { return Vec::new() };
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// `mai ctl history [N | id]`. 인자가 없으면 최근 10건.
pub fn run(args: &[String]) -> i32 {
    let records = read_all();
    match args.first().map(String::as_str) {
        None => { print!("{}", render_list(&records, 10)); 0 }
        Some(a) => match a.parse::<u64>() {
            Ok(n) if n <= 1000 => { print!("{}", render_list(&records, n as usize)); 0 }
            Ok(id) => match records.iter().find(|r| r.id == id) { Some(r) => { print!("{}", render_one(r)); 0 } None => { eprintln!("그런 기록이 없다: {id}"); 1 } },
            Err(_) => { eprintln!("사용법: mai ctl history [건수 | id]"); 2 }
        },
    }
}

pub fn render_list(records: &[Record], n: usize) -> String {
    if records.is_empty() { return "기록이 없다.\n".into(); }
    let mut out = String::from(" id             시각                 대상                 결과  시간   질문\n");
    for r in records.iter().rev().take(n) {
        let targets = r.results.iter().map(|o| match &o.model { Some(m) => format!("{}:{m}", o.agent), None => o.agent.clone() }).collect::<Vec<_>>().join(",");
        let ok = if r.results.iter().all(|o| o.ok) { "OK" } else if r.results.iter().any(|o| o.ok) { "일부" } else { "실패" };
        let q: String = r.question.lines().next().unwrap_or("").chars().take(40).collect();
        out.push_str(&format!(" {:<14} {:<20} {:<20} {:<5} {:>4}s  {q}\n", r.id, r.time, trunc(&targets, 20), ok, r.elapsed_secs));
    }
    out.push_str("자세히: mai ctl history <id>\n");
    out
}

pub fn render_one(r: &Record) -> String {
    let mut out = format!("# {} · {} · {}초{}\n요청자: {}\n경로: {}\n\n## 질문\n{}\n", r.id, r.time, r.elapsed_secs, if r.keep { " · keep" } else { "" }, r.requester, r.cwd, r.question);
    for o in &r.results {
        let label = match &o.model { Some(m) => format!("{}({m})", crate::ctl::display(&o.agent)), None => crate::ctl::display(&o.agent).to_string() };
        out.push_str(&format!("\n## {label} — {}{}\n", if o.ok { "OK" } else { "실패" }, o.origin.as_deref().map(|s| format!(" · {s}")).unwrap_or_default()));
        match (&o.answer, &o.error) { (Some(a), _) => out.push_str(a), (None, Some(e)) => out.push_str(e), _ => {} }
        out.push('\n');
    }
    out
}

fn trunc(s: &str, n: usize) -> String { let c: Vec<char> = s.chars().collect(); if c.len() <= n { s.to_string() } else { c[..n.saturating_sub(1)].iter().collect::<String>() + "…" } }

#[cfg(test)]
mod tests {
    use super::*;
    fn rec(id: u64, ok: bool) -> Record {
        Record { id, time: "2026-11-11 21:00:00".into(), cwd: "/w".into(), requester: "zsh".into(), question: "1+1은?\n둘째".into(), elapsed_secs: 9, keep: false,
            results: vec![Outcome { agent: "codex".into(), model: Some("gpt-6.1-sol".into()), ok, answer: ok.then(|| "2".into()), error: (!ok).then(|| "시간 초과".into()), origin: Some("세션 기록".into()) }] }
    }
    #[test]
    fn 기록을_직렬화하고_그린다() {
        let r = rec(1, true);
        let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back.results[0].answer.as_deref(), Some("2"));
        let list = render_list(&[rec(1, true), rec(2, false)], 10);
        assert!(list.contains("OK") && list.contains("실패") && list.contains("codex:gpt-6.1-sol") && list.contains("1+1은?") && !list.contains("둘째"));
        assert_eq!(render_list(&[rec(1, true), rec(2, false)], 1).matches("2026-11-11").count(), 1);
        let one = render_one(&rec(2, false));
        assert!(one.contains("## Codex(gpt-6.1-sol) — 실패") && one.contains("시간 초과") && one.contains("둘째"));
        assert_eq!(render_list(&[], 5), "기록이 없다.\n");
    }
}
