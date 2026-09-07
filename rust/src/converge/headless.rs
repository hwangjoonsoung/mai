//! 헤드리스 실행. `/converge` 전용 경로다.
//!
//! PTY 를 쓰지 않는다. 대화형에서는 스키마 강제·구조화 출력이 안 나오기 때문이다.
//! 한 바이너리에 두 실행 모드를 두는 이유가 이것이다 (REBUILD.md §2.1).
//!
//! 공급자별 호출 프로필은 Java 판에서 실측으로 확정한 것을 그대로 쓴다.

use crate::pty::resolve_agent;
use anyhow::{anyhow, Result};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub struct Call {
    pub id: String,
    pub name: String,
    pub text: String,
    pub failure: Option<String>,
}

/// 한 공급자를 헤드리스로 호출해 최종 텍스트를 얻는다.
///
/// `schema` 는 스키마 파일 경로다. CLI 레벨 강제를 지원하는 공급자에만 넘어간다.
pub fn invoke(
    id: &str,
    name: &str,
    prompt: &str,
    workspace: &Path,
    schema: &Path,
    temp_dir: &Path,
    model: Option<&str>,
) -> Call {
    match run(id, prompt, workspace, schema, temp_dir, model) {
        Ok(text) => Call { id: id.into(), name: name.into(), text, failure: None },
        Err(e) => Call {
            id: id.into(),
            name: name.into(),
            text: String::new(),
            failure: Some(e.to_string()),
        },
    }
}

fn run(
    id: &str,
    prompt: &str,
    ws: &Path,
    schema: &Path,
    temp: &Path,
    model: Option<&str>,
) -> Result<String> {
    let (exe, prefix) = resolve_agent(id).ok_or_else(|| anyhow!("실행 파일을 찾지 못했다"))?;
    std::fs::create_dir_all(temp)?;

    let mut cmd = Command::new(&exe);
    for a in &prefix {
        cmd.arg(a);
    }
    cmd.current_dir(ws);
    // Claude Code 세션 식별 변수를 물려주지 않는다.
    for k in ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_SESSION_ID"] {
        cmd.env_remove(k);
    }

    // 공급자별 출력 파일 (codex 전용)
    let out_file = temp.join(format!("{id}-{}.md", std::process::id()));

    match id {
        // claude — 프롬프트는 stdin, 출력은 text.
        //
        // --json-schema 는 쓰지 않는다. 경로가 아니라 스키마 **문자열**만 받는데
        // 따옴표가 가득한 인자라 전달이 깨지기 쉽다(Java 판 실측). 프롬프트에
        // 스키마를 실어 보내는 것으로 대신한다.
        "claude" => {
            cmd.args([
                "-p",
                "--add-dir",
                &ws.to_string_lossy(),
                "--input-format",
                "text",
                "--output-format",
                "text",
                "--restricted",
                "--permission-mode",
                "plan",
                "--permission-prompts",
                "none",
                "--tools",
                "",
            ]);
        }
        // codex — 끝의 `-` 가 stdin 에서 프롬프트를 읽으라는 지시다.
        // --output-schema 는 계정 권한에 따라 거부될 수 있으나, 거부돼도
        // 프롬프트에 스키마가 있어 형식은 유지된다.
        "codex" => {
            cmd.args([
                "exec",
                "-",
                "--skip-git-repo-check",
                "-C",
                &ws.to_string_lossy(),
                "-s",
                "read-only",
                "-c",
                "model_reasoning_effort=\"high\"",
                "-o",
                &out_file.to_string_lossy(),
                "--output-schema",
                &schema.to_string_lossy(),
            ]);
        }
        // agy — 프롬프트를 stdin 으로 못 받는다(실측). 인자로 넘긴다.
        // --json-schema 는 **파일 경로**로 준다. 인라인 JSON 은 깨진다.
        "agy" => {
            cmd.args([
                "-p",
                prompt,
                "--add-dir",
                &ws.to_string_lossy(),
                "--model",
                model.unwrap_or("gemini-3.1-pro-high"),
                "--mode",
                "plan",
                "--sandbox",
                "--disable-slash-commands",
                "--output-format",
                "json",
                "--json-schema",
                &schema.to_string_lossy(),
                "--print-timeout",
                "11m",
            ]);
        }
        other => anyhow::bail!("알 수 없는 공급자: {other}"),
    }

    let takes_stdin = id != "agy";
    cmd.stdin(if takes_stdin { Stdio::piped() } else { Stdio::null() });
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    if takes_stdin {
        use std::io::Write;
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(prompt.as_bytes());
            // 닫아야 자식이 입력 끝을 안다. 안 닫으면 영원히 기다린다.
        }
    }

    let out = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    // codex 는 -o 파일에 최종 메시지를 쓴다. 없거나 비면 stdout 으로 폴백한다.
    if id == "codex" {
        if let Ok(s) = std::fs::read_to_string(&out_file) {
            let _ = std::fs::remove_file(&out_file);
            if !s.trim().is_empty() {
                return Ok(s);
            }
        }
    }
    if stdout.trim().is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: String = err.lines().rev().take(3).collect::<Vec<_>>().join(" / ");
        anyhow::bail!("출력이 비었다 ({})", tail.trim());
    }
    Ok(stdout)
}

/// 앱 임시 디렉터리. 워크스페이스를 오염시키지 않는다.
pub fn temp_dir() -> PathBuf {
    crate::room::home().join("temp")
}

/// 병렬 호출 시 한 참여자가 늦어도 나머지를 막지 않도록 하는 대기 상한.
pub const TIMEOUT: Duration = Duration::from_secs(600);
