use super::transport::{Conn, Endpoint, Listener};
use super::{Request, Response};
use std::{io::Write, path::PathBuf, sync::{mpsc, Arc, atomic::{AtomicBool, Ordering}}, time::Duration};

pub struct Server {
    pub endpoint: Endpoint,
    /// 소켓과 마커 파일(`verified-<칸>`)을 두는 `$TMPDIR/mai-<pid>/`.
    pub dir: PathBuf,
    pub rx: mpsc::Receiver<(Request, Box<dyn Conn>)>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

pub fn reply(mut stream: Box<dyn Conn>, response: &Response) {
    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
    if let Ok(mut bytes) = serde_json::to_vec(response) { bytes.push(b'\n'); let _ = stream.write_all(&bytes); }
}

#[cfg(unix)]
mod unix_cleanup {
    //! 시그널 핸들러가 지울 경로. `Drop` 은 SIGTERM 으로 죽을 때 돌지 않으므로
    //! 핸들러에서 직접 `unlink`·`rmdir` 한다(둘 다 async-signal-safe).
    use std::{ffi::CString, path::Path, sync::OnceLock, sync::atomic::{AtomicU64, Ordering}};
    static SOCKET_PATH: OnceLock<CString> = OnceLock::new();
    static SOCKET_DIR: OnceLock<CString> = OnceLock::new();
    /// 지금까지 쓴 가장 큰 칸 id. 핸들러가 `verified-1..=N` 마커를 지우는 상한이다.
    pub static MAX_PANE: AtomicU64 = AtomicU64::new(0);

    extern "C" fn on_signal(sig: libc::c_int) {
        // SAFETY: unlink/rmdir/signal/raise 는 async-signal-safe 이고, 인자는
        // 프로세스 수명 동안 살아 있는 정적 CString 이거나 스택 버퍼다.
        // 할당 없이 경로를 만들려고 숫자를 손으로 적는다.
        unsafe {
            if let Some(p) = SOCKET_PATH.get() { libc::unlink(p.as_ptr()); }
            if let Some(d) = SOCKET_DIR.get() {
                let dir = d.as_bytes();
                let max = MAX_PANE.load(Ordering::Relaxed).min(100_000);
                let mut buf = [0u8; 4096];
                for n in 1..=max {
                    let prefix = b"/verified-";
                    if dir.len() + prefix.len() + 21 >= buf.len() { break; }
                    buf[..dir.len()].copy_from_slice(dir);
                    buf[dir.len()..dir.len() + prefix.len()].copy_from_slice(prefix);
                    let mut i = dir.len() + prefix.len();
                    let mut digits = [0u8; 20]; let mut k = 0; let mut v = n;
                    loop { digits[k] = b'0' + (v % 10) as u8; k += 1; v /= 10; if v == 0 { break; } }
                    while k > 0 { k -= 1; buf[i] = digits[k]; i += 1; }
                    buf[i] = 0;
                    libc::unlink(buf.as_ptr() as *const libc::c_char);
                }
                libc::rmdir(d.as_ptr());
            }
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    }

    pub fn install(path: Option<&Path>, dir: &Path) {
        use std::os::unix::ffi::OsStrExt;
        if let Some(p) = path.and_then(|p| CString::new(p.as_os_str().as_bytes()).ok()) { let _ = SOCKET_PATH.set(p); }
        if let Ok(d) = CString::new(dir.as_os_str().as_bytes()) { let _ = SOCKET_DIR.set(d); }
        for sig in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
            // SAFETY: 핸들러는 위에서 정의한 extern "C" 함수다.
            unsafe { libc::signal(sig, on_signal as *const () as libc::sighandler_t); }
        }
    }

    /// `pid` 가 살아 있는가. 권한이 없어 신호를 못 보내는(EPERM) 경우도 «살아 있다» 로 본다.
    pub fn alive(pid: u32) -> bool {
        let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
        // SAFETY: kill(pid, 0) 은 신호를 보내지 않고 존재만 확인한다.
        if unsafe { libc::kill(pid, 0) } == 0 { return true; }
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}

#[cfg(not(unix))]
mod unix_cleanup {
    /// Windows 는 pid 생존을 간단히 알 길이 없어(표준 라이브러리만으로는) 쓸어내지 않는다.
    pub fn alive(_pid: u32) -> bool { true }
    pub static MAX_PANE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
}

/// 칸이 하나 생길 때마다 부른다. 시그널 정리가 지울 마커 범위를 넓힌다.
pub fn note_pane(id: u64) { unix_cleanup::MAX_PANE.fetch_max(id, std::sync::atomic::Ordering::Relaxed); }

/// 죽은 mai 가 남긴 `mai-<pid>` 디렉터리를 치운다. 비정상 종료(SIGKILL 등)까지 회수하는 뒷단이다.
pub fn sweep_stale(temp: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(temp) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.strip_prefix("mai-")).and_then(|n| n.parse::<u32>().ok()) else { continue };
        if pid == std::process::id() || unix_cleanup::alive(pid) { continue }
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
        make_private_dir(&dir)?;
        let (listener, endpoint) = match bind(&dir) { Ok(x) => x, Err(e) => { let _ = std::fs::remove_dir_all(&dir); return Err(e); } };
        #[cfg(unix)]
        {
            let sock = match &endpoint { Endpoint::Unix(p) => Some(p.as_path()), _ => None };
            unix_cleanup::install(sock, &dir);
        }
        let (tx, rx) = mpsc::channel(); let stop = Arc::new(AtomicBool::new(false)); let done = stop.clone();
        let token = endpoint.token().map(str::to_string);
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok(stream) => {
                        let tx = tx.clone(); let token = token.clone();
                        std::thread::spawn(move || {
                            let mut stream = stream;
                            let Some(line) = read_line(&mut *stream) else { reply(stream, &Response::error("잘못된 요청")); return; };
                            match serde_json::from_str::<Request>(&line) {
                                Ok(req) => {
                                    // TCP 는 누구나 붙을 수 있으니 토큰이 맞아야 한다.
                                    if token.as_deref().is_some_and(|t| req.token.as_deref() != Some(t)) { reply(stream, &Response::error("토큰이 맞지 않는다")); return; }
                                    let _ = tx.send((req, stream));
                                }
                                Err(e) => reply(stream, &Response::error(e)),
                            }
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
                    Err(_) => break,
                }
            }
        });
        Ok(Self { endpoint, dir, rx, stop, thread: Some(thread) })
    }
}

/// 개행까지 한 줄. 1MiB 를 넘거나 개행 없이 끊기면 None.
fn read_line(stream: &mut dyn Conn) -> Option<String> {
    let mut line = Vec::new(); let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return None,
            Ok(n) => {
                line.extend_from_slice(&buf[..n]);
                if line.len() > 1_048_576 { return None; }
                if let Some(i) = line.iter().position(|&b| b == b'\n') { line.truncate(i + 1); break; }
            }
        }
    }
    String::from_utf8(line).ok()
}

fn bind(dir: &std::path::Path) -> std::io::Result<(Listener, Endpoint)> {
    #[cfg(unix)]
    if super::transport::prefer_unix() {
        let path = dir.join("ctl.sock");
        let l = std::os::unix::net::UnixListener::bind(&path)?;
        l.set_nonblocking(true)?;
        return Ok((Listener::Unix(l), Endpoint::Unix(path)));
    }
    let _ = dir;
    super::transport::bind_tcp()
}

fn make_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    { use std::os::unix::fs::DirBuilderExt; std::fs::DirBuilder::new().mode(0o700).create(dir) }
    #[cfg(not(unix))]
    { std::fs::DirBuilder::new().create(dir) }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(unix)]
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
