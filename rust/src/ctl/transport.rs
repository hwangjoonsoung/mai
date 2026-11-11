//! 제어 통로의 바닥 — 유닉스 소켓 또는 루프백 TCP.
//!
//! 유닉스는 `$TMPDIR/mai-<pid>/ctl.sock` 소켓이다(권한 0700 디렉터리라 다른
//! 사용자가 못 붙는다). Windows 는 유닉스 소켓이 없고 이름 있는 파이프는 표준
//! 라이브러리에 없어 의존성이 늘므로, **127.0.0.1 의 빈 포트에 TCP 로 듣고
//! 무작위 토큰으로 가린다.** 토큰은 칸의 환경변수 `MAI_CTL_TOKEN` 으로만 전해지고
//! 요청 JSON 의 `token` 과 맞아야 한다. 유닉스에서도 `MAI_CTL_TCP=1` 이면
//! TCP 를 쓴다 — Windows 경로를 macOS 에서 시험하기 위해서다.
//!
//! `MAI_CTL` 값: 소켓 경로이거나 `tcp://127.0.0.1:<포트>`.
//! `MAI_CTL_DIR`: 마커 파일(`verified-<칸>`)을 두는 디렉터리. 두 전송 모두 만든다.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// 한 연결. 읽고 쓰고, 응답 전에 쓰기 시간 제한을 건다.
pub trait Conn: Read + Write + Send {
    fn set_write_timeout(&self, d: Option<Duration>) -> std::io::Result<()>;
}
impl Conn for TcpStream {
    fn set_write_timeout(&self, d: Option<Duration>) -> std::io::Result<()> { TcpStream::set_write_timeout(self, d) }
}
#[cfg(unix)]
impl Conn for std::os::unix::net::UnixStream {
    fn set_write_timeout(&self, d: Option<Duration>) -> std::io::Result<()> { std::os::unix::net::UnixStream::set_write_timeout(self, d) }
}

/// 어디서 듣는가. `MAI_CTL` 문자열과 1:1 이다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    #[cfg(unix)] Unix(std::path::PathBuf),
    Tcp { port: u16, token: String },
}

impl Endpoint {
    /// 칸에 물려줄 `MAI_CTL` 값.
    pub fn env_value(&self) -> std::ffi::OsString {
        match self {
            #[cfg(unix)] Endpoint::Unix(p) => p.as_os_str().to_owned(),
            Endpoint::Tcp { port, .. } => format!("tcp://127.0.0.1:{port}").into(),
        }
    }
    pub fn token(&self) -> Option<&str> {
        match self { Endpoint::Tcp { token, .. } => Some(token), #[cfg(unix)] _ => None }
    }
    /// 클라이언트 쪽: 환경변수에서 읽는다. 토큰은 `MAI_CTL_TOKEN`.
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var("MAI_CTL").ok()?;
        if let Some(rest) = raw.strip_prefix("tcp://127.0.0.1:") {
            let port = rest.parse().ok()?;
            return Some(Endpoint::Tcp { port, token: std::env::var("MAI_CTL_TOKEN").unwrap_or_default() });
        }
        #[cfg(unix)] { Some(Endpoint::Unix(raw.into())) }
        #[cfg(not(unix))] { None }
    }
    pub fn connect(&self) -> std::io::Result<Box<dyn Conn>> {
        match self {
            #[cfg(unix)] Endpoint::Unix(p) => Ok(Box::new(std::os::unix::net::UnixStream::connect(p)?)),
            Endpoint::Tcp { port, .. } => Ok(Box::new(TcpStream::connect(("127.0.0.1", *port))?)),
        }
    }
}

/// 서버 쪽 리스너. 비동기 accept 를 위해 논블로킹으로 둔다.
pub enum Listener {
    #[cfg(unix)] Unix(std::os::unix::net::UnixListener),
    Tcp(TcpListener),
}

impl Listener {
    pub fn accept(&self) -> std::io::Result<Box<dyn Conn>> {
        match self {
            #[cfg(unix)] Listener::Unix(l) => { let (s, _) = l.accept()?; let _ = s.set_read_timeout(Some(Duration::from_secs(5))); Ok(Box::new(s)) }
            Listener::Tcp(l) => { let (s, _) = l.accept()?; let _ = s.set_read_timeout(Some(Duration::from_secs(5))); Ok(Box::new(s)) }
        }
    }
}

/// 유닉스 소켓을 쓸 수 있는가. Windows 거나 `MAI_CTL_TCP=1` 이면 아니다.
pub fn prefer_unix() -> bool {
    cfg!(unix) && std::env::var_os("MAI_CTL_TCP").map_or(true, |v| v.is_empty() || v == "0")
}

/// 루프백 TCP 로 듣고 토큰을 만든다.
pub fn bind_tcp() -> std::io::Result<(Listener, Endpoint)> {
    let l = TcpListener::bind(("127.0.0.1", 0))?;
    let port = l.local_addr()?.port();
    l.set_nonblocking(true)?;
    Ok((Listener::Tcp(l), Endpoint::Tcp { port, token: random_token() }))
}

/// 의존성 없이 뽑는 무작위 토큰. `RandomState` 는 프로세스마다 OS 엔트로피로 시드된다.
fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..4u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i ^ std::process::id() as u64);
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tcp_엔드포인트는_환경변수와_왕복한다() {
        let e = Endpoint::Tcp { port: 4321, token: "t".into() };
        assert_eq!(e.env_value(), std::ffi::OsString::from("tcp://127.0.0.1:4321"));
        assert_eq!(e.token(), Some("t"));
        let a = random_token(); let b = random_token();
        assert_eq!(a.len(), 64); assert_ne!(a, b);
    }
    #[test]
    fn tcp_로_듣고_붙는다() {
        let (l, e) = bind_tcp().unwrap();
        let mut c = e.connect().unwrap();
        c.write_all(b"ping\n").unwrap();
        let mut s = None;
        for _ in 0..50 { match l.accept() { Ok(x) => { s = Some(x); break } Err(_) => std::thread::sleep(Duration::from_millis(10)) } }
        let mut buf = String::new();
        s.unwrap().read_to_string(&mut buf).ok();
        assert!(buf.starts_with("ping"));
    }
}
