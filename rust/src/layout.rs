//! Persistent workspace layout. The lock is held by the OS, including after crashes.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    pub version: u32,
    pub active_space: usize,
    pub spaces: Vec<SpaceEntry>,
    pub panes: Vec<PaneEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceEntry { pub path: PathBuf }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneEntry {
    pub space: usize,
    pub title: String,
    pub shell_title: String,
    pub user_titled: bool,
    pub cwd: PathBuf,
    pub agent: Option<String>,
    pub session_id: Option<String>,
}

impl Layout {
    pub fn capture(spaces: &[crate::model::Space], active_space: usize, sessions: &[crate::model::Session]) -> Self {
        Self {
            version: 1, active_space,
            spaces: spaces.iter().map(|s| SpaceEntry { path: s.path.clone() }).collect(),
            panes: sessions.iter().filter(|s| s.verifier_of.is_none()).filter_map(|s| {
                let space = spaces.get(s.space)?;
                Some(PaneEntry {
                    space: s.space, title: s.title.clone(), shell_title: s.shell_title.clone(),
                    user_titled: s.user_titled, cwd: s.cwd.clone().unwrap_or_else(|| space.path.clone()),
                    agent: s.resume.as_ref().map(|r| r.0.clone()),
                    session_id: s.resume.as_ref().and_then(|r| r.1.clone()),
                })
            }).collect(),
        }
    }
}

pub struct Store { dir: PathBuf, _lock: File }
impl Store {
    pub fn acquire(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(windows)] {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let mut lock = options.open(dir.join("layout.lock"))?;
        #[cfg(unix)] {
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        lock.set_len(0)?;
        writeln!(lock, "{}", std::process::id())?;
        // Do not unlink the lock: another process may already hold the same inode.
        Ok(Self { dir, _lock: lock })
    }
    pub fn load(&self) -> Option<Layout> {
        let layout: Layout = serde_json::from_reader(File::open(self.dir.join("layout.json")).ok()?).ok()?;
        (layout.version == 1).then_some(layout)
    }
    pub fn save(&self, layout: &Layout) -> io::Result<()> {
        let tmp = self.dir.join("layout.json.tmp");
        let mut file = File::create(&tmp)?;
        serde_json::to_writer_pretty(&mut file, layout)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        replace(&tmp, &self.dir.join("layout.json"))
    }
}

#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> io::Result<()> { fs::rename(from, to) }
#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" { fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32; }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // REPLACE_EXISTING | WRITE_THROUGH, without removing the previous file first.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 {
        Err(io::Error::last_os_error())
    } else { Ok(()) }
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    let normalize = |p: &Path| {
        let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let s = p.to_string_lossy().into_owned();
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    normalize(a) == normalize(b)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.starts_with('-')
        && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
pub fn resume_command(agent: &str, id: Option<&str>) -> Option<String> {
    let id = id.filter(|id| valid_id(id));
    Some(match (agent, id) {
        ("claude", Some(id)) => format!("claude --resume {id}"),
        ("codex", Some(id)) => format!("codex resume {id}"),
        ("agy", Some(id)) => format!("agy --conversation {id}"),
        ("claude", None) => "claude --continue".into(),
        ("codex", None) => "codex resume --last".into(),
        ("agy", None) => "agy --continue".into(),
        _ => return None,
    })
}

fn user_home() -> Option<PathBuf> { crate::app::home_var().map(PathBuf::from) }
pub fn claude_session(pid: u32) -> Option<String> {
    claude_file(&user_home()?.join(format!(".claude/sessions/{pid}.json")))
}
fn claude_file(path: &Path) -> Option<String> {
    let value: serde_json::Value = serde_json::from_reader(File::open(path).ok()?).ok()?;
    let id = value.get("sessionId")?.as_str()?;
    valid_id(id).then(|| id.to_owned())
}
fn command_id(cmd: &[String], flag: &str) -> Option<String> {
    cmd.windows(2).find_map(|w| (w[0] == flag && valid_id(&w[1])).then(|| w[1].clone()))
}
fn files_under(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() && depth > 0 { files_under(&entry.path(), depth - 1, out); }
        else if kind.is_file() { out.push(entry.path()); }
    }
}
fn created_since(path: &Path, start: u64) -> Option<SystemTime> {
    if start == 0 { return None; }
    let time = fs::metadata(path).ok()?.created().ok()?;
    (time >= UNIX_EPOCH + Duration::from_secs(start)).then_some(time)
}
fn codex_meta(path: &Path, cwd: &Path) -> Option<String> {
    let line = BufReader::new(File::open(path).ok()?).lines().next()?.ok()?;
    let value: serde_json::Value = serde_json::from_str(&line).ok()?;
    if value.get("type")?.as_str()? != "session_meta" { return None; }
    let p = value.get("payload")?;
    if p.get("parent_thread_id").is_some_and(|v| !v.is_null())
        || p.get("source").and_then(|s| s.get("subagent")).is_some() { return None; }
    if !same_path(Path::new(p.get("cwd")?.as_str()?), cwd) { return None; }
    let id = p.get("id")?.as_str()?;
    valid_id(id).then(|| id.to_owned())
}
pub fn codex_session(cmd: &[String], cwd: &Path, start: u64, excluded: &HashSet<String>) -> Option<String> {
    if let Some(id) = command_id(cmd, "resume") { return Some(id); }
    codex_files(&user_home()?.join(".codex/sessions"), cwd, start, excluded)
}
fn codex_files(dir: &Path, cwd: &Path, start: u64, excluded: &HashSet<String>) -> Option<String> {
    let mut files = Vec::new();
    files_under(dir, 3, &mut files);
    let mut candidates = files.into_iter().filter(|p| {
        p.extension().is_some_and(|e| e == "jsonl")
            && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("rollout-"))
    }).filter_map(|p| {
        let time = created_since(&p, start)?;
        let id = codex_meta(&p, cwd)?;
        (!excluded.contains(&id)).then_some((time, id))
    }).collect::<Vec<_>>();
    candidates.sort();
    // Refresh when the process creates a new conversation (e.g. /new).
    candidates.into_iter().last().map(|(_, id)| id)
}
pub fn agy_session(cmd: &[String], start: u64, excluded: &HashSet<String>) -> Option<String> {
    if let Some(id) = command_id(cmd, "--conversation") { return Some(id); }
    agy_files(&user_home()?.join(".gemini/antigravity-cli/conversations"), start, excluded)
}
fn agy_files(dir: &Path, start: u64, excluded: &HashSet<String>) -> Option<String> {
    let mut files = Vec::new();
    files_under(dir, 0, &mut files);
    let mut candidates = files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "db"))
        .filter_map(|p| {
            let time = created_since(&p, start)?;
            let id = p.file_stem()?.to_str()?.to_owned();
            (valid_id(&id) && !excluded.contains(&id)).then_some((time, id))
        }).collect::<Vec<_>>();
    candidates.sort();
    candidates.into_iter().last().map(|(_, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("mai-layout-{}-{}-{}", std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn layout_roundtrip_replace_corruption_and_version() {
        let dir = Temp::new();
        let store = Store::acquire(dir.0.clone()).unwrap();
        assert!(store.load().is_none());
        let mut layout = Layout { version: 1, active_space: 0,
            spaces: vec![SpaceEntry { path: dir.0.clone() }], panes: vec![] };
        store.save(&layout).unwrap();
        assert_eq!(store.load(), Some(layout.clone()));
        layout.spaces.push(SpaceEntry { path: dir.0.join("second") });
        store.save(&layout).unwrap();
        assert_eq!(store.load(), Some(layout.clone()));
        layout.version = 2;
        store.save(&layout).unwrap();
        assert!(store.load().is_none());
        fs::write(dir.0.join("layout.json"), "{broken").unwrap();
        assert!(store.load().is_none());
    }

    #[test]
    fn layout_lock_is_exclusive_and_released() {
        let dir = Temp::new();
        let first = Store::acquire(dir.0.clone()).unwrap();
        assert!(Store::acquire(dir.0.clone()).is_err());
        drop(first);
        assert!(Store::acquire(dir.0.clone()).is_ok());
    }

    #[test]
    fn layout_failed_write_keeps_previous_snapshot() {
        let dir = Temp::new();
        let store = Store::acquire(dir.0.clone()).unwrap();
        let layout = Layout { version: 1, active_space: 0, spaces: vec![], panes: vec![] };
        store.save(&layout).unwrap();
        fs::create_dir(dir.0.join("layout.json.tmp")).unwrap();
        let mut changed = layout.clone();
        changed.active_space = 1;
        assert!(store.save(&changed).is_err());
        assert_eq!(store.load(), Some(layout));
    }

    #[test]
    fn layout_excludes_verifiers_and_preserves_pending_resume() {
        let spaces = vec![crate::model::Space::new(PathBuf::from("work"))];
        let mut main = crate::model::Session::terminal("custom", 0);
        main.resume = Some(("codex".into(), Some("thread-1".into())));
        main.resume_waiting = true;
        let mut verifier = crate::model::Session::new("claude", "verify", 0);
        verifier.verifier_of = Some(1);
        let saved = Layout::capture(&spaces, 0, &[main, verifier]);
        assert_eq!(saved.panes.len(), 1);
        assert_eq!(saved.panes[0].session_id.as_deref(), Some("thread-1"));
        assert_eq!(saved.panes[0].cwd, PathBuf::from("work"));
    }

    #[test]
    fn layout_resume_commands_and_untrusted_ids() {
        for (agent, exact, fallback) in [
            ("claude", "claude --resume abc-123", "claude --continue"),
            ("codex", "codex resume abc-123", "codex resume --last"),
            ("agy", "agy --conversation abc-123", "agy --continue"),
        ] {
            assert_eq!(resume_command(agent, Some("abc-123")).as_deref(), Some(exact));
            assert_eq!(resume_command(agent, None).as_deref(), Some(fallback));
            assert_eq!(resume_command(agent, Some("x; shutdown")).as_deref(), Some(fallback));
        }
        assert!(resume_command("unknown", Some("123")).is_none());
        assert_eq!(command_id(&["codex".into(), "resume".into(), "--last".into()], "resume"), None);
        assert_eq!(codex_session(&["codex".into(), "resume".into(), "known-id".into()],
            Path::new("unused"), 0, &HashSet::new()).as_deref(), Some("known-id"));
        assert_eq!(agy_session(&["agy".into(), "--conversation".into(), "known-id".into()],
            0, &HashSet::new()).as_deref(), Some("known-id"));
    }

    #[test]
    fn layout_claude_pid_metadata_refreshes() {
        let dir = Temp::new();
        let path = dir.0.join("123.json");
        fs::write(&path, r#"{"sessionId":"first"}"#).unwrap();
        assert_eq!(claude_file(&path).as_deref(), Some("first"));
        fs::write(&path, r#"{"sessionId":"second"}"#).unwrap();
        assert_eq!(claude_file(&path).as_deref(), Some("second"));
        fs::write(&path, "broken").unwrap();
        assert!(claude_file(&path).is_none());
    }

    #[test]
    fn layout_codex_matches_cwd_time_and_exclusions() {
        let dir = Temp::new();
        let path = dir.0.join("rollout-main.jsonl");
        let meta = serde_json::json!({"type":"session_meta", "payload": {
            "id":"main-id", "cwd":dir.0, "source":"cli"
        }});
        fs::write(&path, format!("{meta}\n")).unwrap();
        assert_eq!(codex_meta(&path, &dir.0).as_deref(), Some("main-id"));
        assert!(codex_meta(&path, &dir.0.join("other")).is_none());
        // Creation timestamps are not supported by every Unix filesystem.
        if fs::metadata(&path).unwrap().created().is_ok() {
            assert_eq!(codex_files(&dir.0, &dir.0, 1, &HashSet::new()).as_deref(), Some("main-id"));
            assert!(codex_files(&dir.0, &dir.0, 1, &HashSet::from(["main-id".into()])).is_none());
            let future = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 60;
            assert!(codex_files(&dir.0, &dir.0, future, &HashSet::new()).is_none());
        }
        let mut sub = meta;
        sub["payload"]["parent_thread_id"] = serde_json::json!("parent");
        fs::write(&path, format!("{sub}\n")).unwrap();
        assert!(codex_meta(&path, &dir.0).is_none());
    }

    #[test]
    fn layout_agy_ignores_sidecars_and_claimed_ids() {
        let dir = Temp::new();
        let db = dir.0.join("conversation-1.db");
        fs::write(&db, []).unwrap();
        fs::write(dir.0.join("conversation-2.db-wal"), []).unwrap();
        if fs::metadata(&db).unwrap().created().is_ok() {
            assert_eq!(agy_files(&dir.0, 1, &HashSet::new()).as_deref(), Some("conversation-1"));
            assert!(agy_files(&dir.0, 1, &HashSet::from(["conversation-1".into()])).is_none());
        }
        assert!(agy_files(&dir.0, 0, &HashSet::new()).is_none());
    }
}
