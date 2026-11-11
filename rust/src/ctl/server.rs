use super::{Request, Response};
use std::{ffi::CString, io::{BufRead, BufReader, Write}, os::unix::{fs::DirBuilderExt, net::{UnixListener, UnixStream}}, path::{Path, PathBuf}, sync::{mpsc, Arc, OnceLock, atomic::{AtomicBool, Ordering}}, time::Duration};
pub struct Server { pub path: PathBuf, pub rx: mpsc::Receiver<(Request, UnixStream)>, stop: Arc<AtomicBool>, thread: Option<std::thread::JoinHandle<()>> }
pub fn reply(mut stream: UnixStream, response: &Response) {
    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
    if let Ok(mut bytes) = serde_json::to_vec(response) { bytes.push(b'\n'); let _ = stream.write_all(&bytes); }
}

/// 시그널 핸들러가 지울 경로. `Drop` 은 SIGTERM 으로 죽을 때 돌지 않으므로
/// 핸들러에서 직접 `unlink`·`rmdir` 한다(둘 다 async-signal-safe).
static SOCKET_PATH: OnceLock<CString> = OnceLock::new();
static SOCKET_DIR: OnceLock<CString> = OnceLock::new();

extern "C" fn on_signal(sig: libc::c_int) {
    // SAFETY: unlink/rmdir/signal/raise 는 async-signal-safe 이고, 인자는
    // 프로세스 수명 동안 살아 있는 정적 CString 이다.
    unsafe {
        if let Some(p) = SOCKET_PATH.get() { libc::unlink(p.as_ptr()); }
        if let Some(d) = SOCKET_DIR.get() { libc::rmdir(d.as_ptr()); }
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

fn install_signal_cleanup(path: &Path, dir: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let (Ok(p), Ok(d)) = (CString::new(path.as_os_str().as_bytes()), CString::new(dir.as_os_str().as_bytes())) else { return };
    let _ = SOCKET_PATH.set(p);
    let _ = SOCKET_DIR.set(d);
    for sig in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
        // SAFETY: 핸들러는 위에서 정의한 extern "C" 함수다.
        unsafe { libc::signal(sig, on_signal as *const () as libc::sighandler_t); }
    }
}

/// `pid` 가 살아 있는가. 권한이 없어 신호를 못 보내는(EPERM) 경우도 «살아 있다» 로 본다.
fn alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
    // SAFETY: kill(pid, 0) 은 신호를 보내지 않고 존재만 확인한다.
    if unsafe { libc::kill(pid, 0) } == 0 { return true; }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// 죽은 mai 가 남긴 `mai-<pid>` 디렉터리를 치운다. 비정상 종료(SIGKILL 등)까지 회수하는 뒷단이다.
pub fn sweep_stale(temp: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(temp) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.strip_prefix("mai-")).and_then(|n| n.parse::<u32>().ok()) else { continue };
        if pid == std::process::id() || alive(pid) { continue }
        if !entry.path().is_dir() { continue }
        if std::fs::remove_dir_all(entry.path()).is_ok() { removed += 1; }
    }
    removed
}

impl Server {
    pub fn start() -> std::io::Result<Self> {
        let temp = std::env::temp_dir();
        sweep_stale(&temp);
        let dir = temp.join(format!("mai-{}", std::process::id()));
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        let path = dir.join("ctl.sock");
        let listener = match UnixListener::bind(&path) { Ok(l) => l, Err(e) => { let _ = std::fs::remove_dir(&dir); return Err(e); } };
        listener.set_nonblocking(true)?;
        install_signal_cleanup(&path, &dir);
        let (tx, rx) = mpsc::channel(); let stop = Arc::new(AtomicBool::new(false)); let done = stop.clone();
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let tx = tx.clone();
                        std::thread::spawn(move || {
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                            let mut line = String::new();
                            use std::io::Read;
                            let result = BufReader::new((&stream).take(1_048_577)).read_line(&mut line);
                            if result.is_err() || !line.ends_with('\n') || line.len() > 1_048_576 { reply(stream, &Response::error("잘못된 요청")); return; }
                            match serde_json::from_str(&line) { Ok(req) => { let _ = tx.send((req, stream)); }, Err(e) => reply(stream, &Response::error(e)) }
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
                    Err(_) => break,
                }
            }
        });
        Ok(Self { path, rx, stop, thread: Some(thread) })
    }
}
impl Drop for Server { fn drop(&mut self) { self.stop.store(true, Ordering::Relaxed); if let Some(t) = self.thread.take() { let _ = t.join(); } let _ = std::fs::remove_file(&self.path); if let Some(p) = self.path.parent() { let _ = std::fs::remove_dir(p); } } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 죽은_pid의_디렉터리만_치운다() {
        let temp = std::env::temp_dir().join(format!("mai-sweep-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        // pid_max 를 넘는 pid 는 존재할 수 없다 → 죽은 것으로 판정돼 지워진다.
        let dead = temp.join("mai-2000000000"); std::fs::create_dir(&dead).unwrap(); std::fs::File::create(dead.join("ctl.sock")).unwrap();
        let mine = temp.join(format!("mai-{}", std::process::id())); std::fs::create_dir(&mine).unwrap();
        let other = temp.join("mai-codex-1-2-0"); std::fs::create_dir(&other).unwrap();
        let plain = temp.join("mai-1"); std::fs::create_dir(&plain).unwrap(); // init 은 항상 살아 있다
        assert_eq!(sweep_stale(&temp), 1);
        assert!(!dead.exists()); assert!(mine.exists()); assert!(other.exists()); assert!(plain.exists());
        let _ = std::fs::remove_dir_all(&temp);
    }
}
