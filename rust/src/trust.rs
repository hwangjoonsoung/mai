//! 워크스페이스를 각 에이전트에 신뢰 등록한다.
//!
//! 에이전트들은 처음 보는 디렉터리에서 "이 폴더를 신뢰합니까?" 를 묻고 답할 때까지
//! 멈춘다. 패널 안에서 이걸 매번 답하는 건 번거롭고, 자동 주입한 프롬프트가
//! 그 대화상자에 먹혀 버린다.
//!
//! **이건 보안 결정이므로 자동으로 하지 않는다.** `--trust` 를 명시적으로 실행할
//! 때만 기록하며, 하는 일은 사용자가 대화상자에서 "예" 를 누르는 것과 같다.
//! 대상은 지정한 워크스페이스 하나뿐이다.
//!
//! 저장 위치 (실측)
//!   claude  ~/.claude.json      projects[<경로>].hasTrustDialogAccepted = true
//!   codex   ~/.codex/config.toml  [projects."<경로>"] trust_level = "trusted"
//!           경로 표기는 OS 마다 다르다 — Windows 는 소문자 + 이스케이프된
//!           역슬래시, macOS/Linux 는 원본 경로 그대로다 (실측).
//!   agy     신뢰 대화상자를 쓰지 않는다 (확인된 바 없음)

use anyhow::{Context, Result};
use std::{fs, path::Path};

pub struct Report {
    pub agent: &'static str,
    pub outcome: String,
}

/// 워크스페이스를 신뢰 목록에 넣는다. 이미 있으면 그대로 둔다.
pub fn trust_workspace(ws: &Path) -> Vec<Report> {
    let ws = ws.canonicalize().unwrap_or_else(|_| ws.to_path_buf());
    let display = strip_unc(&ws.to_string_lossy());

    vec![
        Report { agent: "claude", outcome: describe(trust_claude(&display)) },
        Report { agent: "codex", outcome: describe(trust_codex(&display)) },
        Report {
            agent: "agy",
            outcome: "해당 없음 (신뢰 대화상자를 쓰지 않는다)".into(),
        },
    ]
}

fn describe(r: Result<bool>) -> String {
    match r {
        Ok(true) => "등록함".into(),
        Ok(false) => "이미 등록돼 있음".into(),
        Err(e) => format!("실패: {e}"),
    }
}

/// Windows 정규화 경로의 `\\?\` 접두어를 뗀다. 설정 파일에는 이게 없다.
fn strip_unc(s: &str) -> String {
    s.strip_prefix(r"\\?\").unwrap_or(s).to_string()
}

// ---------- claude ----------

/// `~/.claude.json` 의 `projects` 에 항목을 넣는다.
///
/// 파일이 크고(수십만 바이트) 우리가 모르는 필드가 많아 **JSON 을 다시 쓰지 않는다.**
/// 텍스트로 열어 해당 프로젝트 블록에 플래그만 넣거나, 없으면 항목을 추가한다.
fn trust_claude(ws: &str) -> Result<bool> {
    let path = home()?.join(".claude.json");
    if !path.is_file() {
        anyhow::bail!("{} 가 없다", path.display());
    }
    let text = fs::read_to_string(&path).context("claude 설정을 읽지 못했다")?;
    let mut v: serde_json::Value =
        serde_json::from_str(&text).context("claude 설정이 JSON 이 아니다")?;

    // claude 는 경로를 슬래시로 저장한다.
    let key = ws.replace(chr_backslash(), "/");
    let projects = v
        .get_mut("projects")
        .and_then(|p| p.as_object_mut())
        .context("projects 항목이 없다")?;

    if let Some(entry) = projects.get_mut(&key) {
        if entry.get("hasTrustDialogAccepted") == Some(&serde_json::Value::Bool(true)) {
            return Ok(false);
        }
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("hasTrustDialogAccepted".into(), serde_json::Value::Bool(true));
        }
    } else {
        let mut obj = serde_json::Map::new();
        obj.insert("hasTrustDialogAccepted".into(), serde_json::Value::Bool(true));
        projects.insert(key, serde_json::Value::Object(obj));
    }

    write_atomic(&path, &serde_json::to_string_pretty(&v)?)?;
    Ok(true)
}

// ---------- codex ----------

/// `~/.codex/config.toml` 에 `[projects."<경로>"] trust_level = "trusted"` 를 덧붙인다.
///
/// TOML 을 파싱해 다시 쓰면 주석과 순서가 사라진다. 이미 있는지 확인하고 없을 때만
/// 파일 끝에 추가한다.
fn trust_codex(ws: &str) -> Result<bool> {
    let path = home()?.join(".codex").join("config.toml");
    if !path.is_file() {
        anyhow::bail!("{} 가 없다", path.display());
    }
    let text = fs::read_to_string(&path).context("codex 설정을 읽지 못했다")?;

    // Windows 의 codex 는 경로를 소문자 + 이스케이프된 역슬래시로 저장한다.
    // macOS/Linux 는 파일 시스템이 대소문자를 가리므로 원본 경로를 그대로 쓴다.
    // 소문자로 낮추면 codex 가 못 알아보는 항목이 하나 더 붙을 뿐이다.
    let key = if cfg!(windows) {
        let lower = ws.to_lowercase();
        lower.replace(chr_backslash(), &format!("{0}{0}", chr_backslash()))
    } else {
        ws.to_string()
    };
    let header = format!("[projects.\"{key}\"]");

    if text.contains(&header) {
        return Ok(false);
    }
    let mut out = text;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&header);
    out.push('\n');
    out.push_str("trust_level = \"trusted\"\n");
    write_atomic(&path, &out)?;
    Ok(true)
}

// ---------- 공통 ----------

fn home() -> Result<std::path::PathBuf> {
    let h = crate::app::home_var().context("홈 디렉터리를 찾지 못했다")?;
    Ok(std::path::PathBuf::from(h))
}

/// 임시 파일에 쓰고 교체한다. 도중에 죽어도 원본이 깨지지 않는다.
fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("multiai.tmp");
    fs::write(&tmp, content).context("임시 파일을 쓰지 못했다")?;
    fs::rename(&tmp, path).context("설정 파일을 교체하지 못했다")?;
    Ok(())
}

fn chr_backslash() -> char {
    92 as char
}
