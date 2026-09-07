//! PTY 세션 하나. 자식 에이전트를 대화형 그대로 띄우고 출력을 VT 파서에 먹인다.
//!
//! 헤드리스(`claude -p`)와 다른 점: 에이전트가 자기 TUI 를 그린다. 우리는 그 화면을
//! 받아 그릴 뿐이다.

use anyhow::{anyhow, Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver, TryRecvError},
        Arc, Mutex,
    },
};

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
    parser: Arc<Mutex<vt100::Parser>>,
    done: bool,
    /// 진단용 — PTY 에서 실제로 받은 바이트 수
    pub rx_bytes: usize,
    /// 진단용 — 자식 종료 코드
    pub exit_code: Option<u32>,
    // Lifetime follows the shell; no global PATH or shell rc changes.
    _codex_shim: Option<CodexShim>,
}

impl PtySession {
    /// 에이전트를 현재 디렉터리에서 기동한다.
    pub fn spawn(agent: &str, rows: u16, cols: u16) -> Result<Self> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        Self::spawn_in(agent, rows, cols, &cwd)
    }

    /// 에이전트를 **주어진 경로에서** 기동한다. 크기는 화면과 같아야 한다.
    ///
    /// 공간(space)마다 경로가 다르므로 작업 디렉터리를 인자로 받는다.
    /// 에이전트는 cwd 를 기준으로 파일을 찾으므로 이게 곧 「어느 프로젝트인가」다.
    pub fn spawn_in(agent: &str, rows: u16, cols: u16, cwd: &std::path::Path) -> Result<Self> {
        let (exe, prefix) = resolve_agent(agent)
            .ok_or_else(|| anyhow!("실행 파일을 찾지 못했다: {agent}"))?;
        Self::spawn_exe(exe, prefix, rows, cols, cwd, false)
    }

    /// 사용자의 셸을 그 공간의 경로에서 띄운다.
    pub fn spawn_shell_in(rows: u16, cols: u16, cwd: &std::path::Path) -> Result<Self> {
        let (exe, prefix) = resolve_shell().ok_or_else(|| anyhow!("셸을 찾지 못했다"))?;
        Self::spawn_exe(exe, prefix, rows, cols, cwd, true)
    }

    fn spawn_exe(
        exe: PathBuf,
        prefix: Vec<String>,
        rows: u16,
        cols: u16,
        cwd: &std::path::Path,
        shell: bool,
    ) -> Result<Self> {

        let pty = portable_pty::native_pty_system();
        let pair = pty
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .context("PTY 를 열지 못했다")?;

        let codex_shim = if shell { CodexShim::for_shell()? } else { None };
        let mut cmd = CommandBuilder::new(exe);
        if let Some(shim) = &codex_shim { cmd.env("PATH", shim.path()?); }
        // Advertise the emulator we implement, not the outer terminal’s private protocols.
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for a in &prefix {
            cmd.arg(a);
        }
        cmd.cwd(cwd);
        // Claude Code 세션 식별 변수를 물려주지 않는다. 중첩 실행 시 혼동을 막는다.
        for k in ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_SESSION_ID"] {
            cmd.env_remove(k);
        }

        let child = pair.slave.spawn_command(cmd).context("에이전트를 띄우지 못했다")?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().context("PTY 리더 복제 실패")?;
        let writer = pair.master.take_writer().context("PTY 라이터 획득 실패")?;

        // 읽기는 블로킹이라 별도 스레드에 둔다. 안 그러면 렌더 루프가 멈춘다.
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            master: pair.master,
            child,
            writer,
            rx,
            parser: Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 2000))),
            done: false,
            rx_bytes: 0,
            exit_code: None,
            _codex_shim: codex_shim,
        })
    }

    /// 받은 출력을 전부 파서에 먹인다. 논블로킹이다.
    pub fn pump(&mut self) {
        let mut pending_replies: Vec<u8> = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(chunk) => {
                    self.rx_bytes += chunk.len();
                    if std::env::var("MAI_DUMP").is_ok() {
                        eprintln!("[rx {}] {:?}", chunk.len(), String::from_utf8_lossy(&chunk));
                    }
                    if let Ok(mut p) = self.parser.lock() {
                        p.process(&chunk);
                    }
                    // 자식이 던진 질의에 답한다. 안 하면 상대가 응답을 기다리며 멈춘다.
                    pending_replies.extend(self.answer_queries(&chunk));
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.done = true;
                    break;
                }
            }
        }
        if !pending_replies.is_empty() {
            let _ = self.write(&pending_replies);
        }
        if let Ok(Some(st)) = self.child.try_wait() {
            self.exit_code = Some(st.exit_code());
            self.done = true;
        }
    }

    /// 터미널 질의에 대한 응답을 만든다.
    ///
    /// **에뮬레이터의 필수 책무다.** ConPTY 는 기동 직후 `ESC[6n`(커서 위치)을 보내고
    /// 응답을 기다린다. 답하지 않으면 자식 출력이 한 바이트도 나오지 않는다
    /// (R1 에서 실측으로 확인한 원인). vt100 크레이트는 이걸 대신해 주지 않는다.
    fn answer_queries(&self, chunk: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < chunk.len() {
            if chunk[i] != 0x1b || chunk[i + 1] != b'[' {
                i += 1;
                continue;
            }
            // ESC [ ... <최종 바이트> 형태를 훑는다.
            let start = i + 2;
            let mut j = start;
            while j < chunk.len() && !(0x40..=0x7e).contains(&chunk[j]) {
                j += 1;
            }
            if j >= chunk.len() {
                break;
            }
            let params = &chunk[start..j];
            let final_byte = chunk[j];
            match (params, final_byte) {
                // DSR — 커서 위치 보고. 1-기준으로 답한다.
                (b"6", b'n') | (b"?6", b'n') => {
                    let (r, c) = self
                        .parser
                        .lock()
                        .map(|p| p.screen().cursor_position())
                        .unwrap_or((0, 0));
                    out.extend_from_slice(
                        format!("\x1b[{};{}R", r + 1, c + 1).as_bytes(),
                    );
                }
                // DSR — 상태 보고. "이상 없음".
                (b"5", b'n') => out.extend_from_slice(b"\x1b[0n"),
                // DA1 — 장치 속성. VT100 상당으로 답한다.
                (b"", b'c') | (b"0", b'c') => out.extend_from_slice(b"\x1b[?1;2c"),
                // DA2 — 보조 장치 속성.
                (b">", b'c') | (b">0", b'c') => out.extend_from_slice(b"\x1b[>0;10;1c"),
                _ => {}
            }
            i = j + 1;
        }
        out
    }

    /// 휠을 자식에게 넘긴다. 자식이 마우스를 안 받으면 false.
    ///
    /// 좌표는 **칸 안에서의 위치**여야 한다. 화면 좌표를 그대로 주면 자식은
    /// 자기 원점을 기준으로 해석해 엉뚱한 곳을 가리킨 것이 된다 — 칸마다
    /// 원점이 다르기 때문이다.
    ///
    /// 인코딩은 자식이 고른 것을 따른다. SGR(`ESC[?1006h`)은 좌표에 상한이
    /// 없고, 옛 방식은 한 바이트(32+좌표)라 223칸을 넘지 못한다.
    pub fn mouse_wheel(&mut self, up: bool, col: u16, row: u16) -> bool {
        use vt100::MouseProtocolMode as Mode;
        let (mode, enc) = match self.parser.lock() {
            Ok(p) => (p.screen().mouse_protocol_mode(), p.screen().mouse_protocol_encoding()),
            Err(_) => return false,
        };
        if mode == Mode::None {
            return false;
        }
        self.write(&encode_wheel(enc, up, col, row)).is_ok()
    }

    /// 우리가 들고 있는 스크롤백을 위아래로 옮긴다.
    ///
    /// 마우스를 안 받는 자식(평범한 셸)용이다. 대체 화면에서는 스크롤백이
    /// 0줄이라 아무 일도 일어나지 않는다 — 그쪽은 자식이 직접 그린다.
    pub fn scroll_view(&mut self, delta: i32) {
        if let Ok(mut p) = self.parser.lock() {
            let cur = p.screen().scrollback() as i32;
            let next = (cur - delta).max(0) as usize;
            p.screen_mut().set_scrollback(next);
        }
    }

    /// 자식 프로세스 ID. 서브에이전트(자손 프로세스) 조회의 기준점이다.
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    pub fn screen(&self) -> vt100::Screen {
        self.parser.lock().unwrap().screen().clone()
    }

    pub fn finished(&self) -> bool {
        self.done
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    /// 화면이 바뀌면 PTY 와 파서 양쪽을 같이 맞춘다. 한쪽만 바꾸면 어긋난다.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        if let Ok(mut p) = self.parser.lock() {
            p.screen_mut().set_size(rows, cols);
        }
        Ok(())
    }
}

impl PtySession {
    /// 자식을 종료한다. 종료 확인까지 짧게 기다린다.
    ///
    /// 이걸 안 하면 우리가 빠져나온 뒤에도 에이전트 프로세스가 남는다.
    /// Java 판에서 /cancel 을 만들며 겪었던 것과 같은 부류의 문제다.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        for _ in 0..20 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Session-local Codex entry point for commands typed in a shell panel.
/// The user's shell configuration and the parent process environment are untouched.
struct CodexShim(std::path::PathBuf);

impl CodexShim {
    #[cfg(unix)]
    fn for_shell() -> Result<Option<Self>> {
        match resolve_codex() {
            Some((exe, args)) => Self::new(&exe, &args).map(Some),
            None => Ok(None),
        }
    }

    #[cfg(not(unix))]
    fn for_shell() -> Result<Option<Self>> { Ok(None) }

    #[cfg(unix)]
    fn new(exe: &std::path::Path, args: &[String]) -> Result<Self> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
        let dir = std::env::temp_dir().join(format!("mai-codex-{}-{time}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        let shim = Self(dir);
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
        let command = std::iter::once(exe.to_string_lossy().into_owned()).chain(args.iter().cloned())
            .map(|arg| quote(&arg)).collect::<Vec<_>>().join(" ");
        let script = format!("#!/bin/sh\nfor arg do\n  if [ \"$arg\" = --no-alt-screen ]; then exec {command} \"$@\"; fi\ndone\nexec {command} --no-alt-screen \"$@\"\n");
        let file = shim.0.join("codex");
        std::fs::write(&file, script)?;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700))?;
        Ok(shim)
    }

    fn path(&self) -> Result<std::ffi::OsString> {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        Ok(std::env::join_paths(std::iter::once(self.0.clone()).chain(std::env::split_paths(&inherited)))?)
    }
}

impl Drop for CodexShim {
    fn drop(&mut self) {
        // Remove only the two artifacts we created; never recursively delete a path.
        let _ = std::fs::remove_file(self.0.join("codex"));
        let _ = std::fs::remove_dir(&self.0);
    }
}

/// 에이전트를 어떻게 띄울지 해석한다.
///
/// 반환값은 (실행 파일, 선행 인자) 다. codex 처럼 실행 파일이 PATH 에
/// `.exe` 로 없고 node 스크립트로 존재하는 경우가 있다.
///
/// **셸 래퍼(.cmd/.ps1)는 쓰지 않는다.** 셸이 인자를 재해석해 프롬프트가 깨진다
/// (Java 판에서 실측 확인. SPEC §6.3).
pub fn resolve_agent(agent: &str) -> Option<(PathBuf, Vec<String>)> {
    // 사용자가 경로를 직접 준 경우
    let direct = PathBuf::from(agent);
    if direct.is_file() {
        return Some((direct, vec![]));
    }
    match agent {
        "codex" => resolve_codex().map(|(exe, mut args)| {
            args.push("--no-alt-screen".into());
            (exe, args)
        }),
        // agy 는 macOS 에서 ~/.local/bin 에 설치되는 경우가 많다 (SPEC §8.4 K6).
        "agy" if !cfg!(windows) => {
            let local = home_dir().join(".local/bin/agy");
            if local.is_file() {
                return Some((local, vec![]));
            }
            resolve("agy").map(|p| (p, vec![]))
        }
        other => resolve(other).map(|p| (p, vec![])),
    }
}

/// 프로세스가 지금 들어가 있는 경로.
///
/// **cwd 는 명시해서 달라고 해야 준다.** `refresh_processes` 의 기본 갱신
/// 종류에는 cwd 가 없어서 프로세스는 찾아도 `cwd()` 가 None 으로 온다(실측).
/// 메모리·CPU·디스크는 우리가 안 쓰므로 빼는 편이 싸기도 하다.
///
/// `sys` 를 밖에서 받는 것은 매번 새로 만들면 비싸기 때문이다. 앱은 하나를
/// 계속 들고 쓰고, 진단은 그때그때 만든다.
pub fn process_cwd(sys: &mut sysinfo::System, pid: u32) -> Option<PathBuf> {
    let pid = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        true,
        sysinfo::ProcessRefreshKind::new().with_cwd(sysinfo::UpdateKind::Always),
    );
    sys.process(pid).and_then(|p| p.cwd().map(PathBuf::from))
}

/// 휠 한 칸을 자식이 고른 인코딩으로 바꾼다.
///
/// 휠은 버튼 64(위)/65(아래)로 보고하고 좌표는 1-기준이다. SGR 은 좌표에 상한이
/// 없지만 옛 방식은 `32 + 좌표` 를 한 바이트에 담아 223칸에서 막힌다.
pub fn encode_wheel(
    enc: vt100::MouseProtocolEncoding,
    up: bool,
    col: u16,
    row: u16,
) -> Vec<u8> {
    use vt100::MouseProtocolEncoding as Enc;
    let btn: u16 = if up { 64 } else { 65 };
    let (c, r) = (col + 1, row + 1);
    match enc {
        Enc::Sgr => format!("\u{1b}[<{btn};{c};{r}M").into_bytes(),
        // Utf8 도 우리가 보내는 좁은 범위에서는 옛 방식과 같은 바이트다.
        _ => vec![
            0x1b,
            b'[',
            b'M',
            (32 + btn).min(255) as u8,
            (32 + c).min(255) as u8,
            (32 + r).min(255) as u8,
        ],
    }
}

/// 사용자의 대화형 셸을 어떻게 띄울지 해석한다.
///
/// 에이전트와 달리 셸은 **환경이 정해 준다.** `$SHELL` 을 먼저 보고, 없으면
/// 그 OS 의 관례적인 셸로 떨어진다. 인자는 주지 않는다 — PTY 에 붙은 셸은
/// 그것만으로 대화형이고, rc 파일도 알아서 읽는다.
pub fn resolve_shell() -> Option<(PathBuf, Vec<String>)> {
    if cfg!(windows) {
        // COMSPEC 은 cmd.exe 를 가리킨다. PowerShell 이 있으면 그쪽이 낫다.
        for name in ["pwsh", "powershell"] {
            if let Some(p) = resolve(name) {
                return Some((p, vec!["-NoLogo".into()]));
            }
        }
        return std::env::var_os("COMSPEC").map(|c| (PathBuf::from(c), vec![]));
    }
    if let Some(sh) = std::env::var_os("SHELL") {
        let p = PathBuf::from(sh);
        if p.is_file() {
            return Some((p, vec![]));
        }
    }
    for cand in ["/bin/zsh", "/bin/bash", "/bin/sh"] {
        let p = PathBuf::from(cand);
        if p.is_file() {
            return Some((p, vec![]));
        }
    }
    None
}

/// 셸 실행 파일의 표시용 이름. `/bin/zsh` -> `zsh`.
pub fn shell_name() -> String {
    resolve_shell()
        .and_then(|(p, _)| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "셸".into())
}

fn home_dir() -> PathBuf {
    crate::app::home_var()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// codex 는 PATH 에 codex.exe 가 없다. 셸 shim(codex/.cmd/.ps1)만 있다.
///
///   1순위  벤더 네이티브 codex.exe
///   2순위  node.exe + codex.js  (shim 이 하는 일을 셸 없이 재현)
///   실패   지원 불가로 보고한다
fn resolve_codex() -> Option<(PathBuf, Vec<String>)> {
    let native = if cfg!(windows) { "codex.exe" } else { "codex" };
    for root in npm_roots() {
        let base = root.join("node_modules/@openai/codex/node_modules");
        if base.is_dir() {
            if let Some(exe) = find_file(&base, native, 8, Some("vendor")) {
                return Some((exe, vec![]));
            }
        }
    }
    if !cfg!(windows) {
        if let Some(exe) = resolve("codex") { return Some((exe, vec![])); }
    }
    let node = resolve("node")?;
    for root in npm_roots() {
        let js = root.join("node_modules/@openai/codex/bin/codex.js");
        if js.is_file() {
            return Some((node, vec![js.to_string_lossy().into_owned()]));
        }
    }
    None
}

fn npm_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        out.push(PathBuf::from(appdata).join("npm"));
    }
    if !cfg!(windows) {
        // Homebrew(Intel/Apple Silicon)와 사용자 npm prefix
        out.push(PathBuf::from("/opt/homebrew/lib"));
        out.push(PathBuf::from("/usr/local/lib"));
        out.push(home_dir().join(".npm-global/lib"));
    }
    // 셸 shim 이 있는 디렉터리도 후보다.
    if let Some(p) = resolve("codex.cmd").and_then(|p| p.parent().map(PathBuf::from)) {
        out.push(p);
    }
    out
}

/// 깊이 제한 재귀 탐색. 경로에 `must_contain` 이 들어간 것만 채택한다.
fn find_file(dir: &std::path::Path, name: &str, depth: usize, must_contain: Option<&str>) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    let mut dirs = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            dirs.push(path);
        } else if path.file_name().and_then(|s| s.to_str()) == Some(name) {
            let ok = match must_contain {
                Some(part) => path.to_string_lossy().replace(chr_backslash(), "/").contains(part),
                None => true,
            };
            if ok {
                return Some(path);
            }
        }
    }
    for d in dirs {
        if let Some(hit) = find_file(&d, name, depth - 1, must_contain) {
            return Some(hit);
        }
    }
    None
}

fn chr_backslash() -> char {
    92 as char
}

/// PATH 에서 실행 파일을 찾는다. Windows 는 확장자를 붙여 본다.
///
/// 셸 래퍼(.cmd/.ps1)는 쓰지 않는다 — 셸이 인자를 재해석해 프롬프트가 깨진다
/// (Java 판에서 실측으로 확인한 사항. SPEC §6.3).
fn resolve(name: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(name);
    if direct.is_file() {
        return Some(direct);
    }
    let exts: &[&str] = if cfg!(windows) { &[".exe", ""] } else { &[""] };
    let path = std::env::var_os("PATH")?;
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    if !cfg!(windows) {
        // GUI 로 띄우면 PATH 가 빈약하다. 흔한 위치를 보조로 덧붙인다.
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(home_dir().join(".local/bin"));
    }
    for dir in dirs {
        for ext in exts {
            let c = dir.join(format!("{name}{ext}"));
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

impl PtySession {
    /// 인자를 직접 지정해 띄운다. 셀프테스트와 헤드리스 호출에 쓴다.
    pub fn spawn_raw(program: &str, args: &[&str], rows: u16, cols: u16) -> Result<Self> {
        let exe = resolve(program)
            .ok_or_else(|| anyhow!("실행 파일을 찾지 못했다: {program}"))?;
        let pty = portable_pty::native_pty_system();
        let pair = pty
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .context("PTY 를 열지 못했다")?;

        let mut cmd = CommandBuilder::new(exe);
        for a in args {
            cmd.arg(a);
        }
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }


        let child = pair.slave.spawn_command(cmd).context("자식을 띄우지 못했다")?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().context("PTY 리더 복제 실패")?;
        let writer = pair.master.take_writer().context("PTY 라이터 획득 실패")?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            master: pair.master,
            child,
            writer,
            rx,
            parser: Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 2000))),
            done: false,
            rx_bytes: 0,
            exit_code: None,
            _codex_shim: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vt100::MouseProtocolEncoding as Enc;

    /// 좌표는 1-기준이다. 0-기준으로 보내면 자식이 한 칸씩 어긋나게 본다.
    #[test]
    fn sgr_휠은_1기준_좌표다() {
        assert_eq!(encode_wheel(Enc::Sgr, true, 0, 0), b"\x1b[<64;1;1M");
        assert_eq!(encode_wheel(Enc::Sgr, false, 9, 4), b"\x1b[<65;10;5M");
    }

    /// 옛 방식은 32 를 더해 한 바이트에 담는다.
    #[test]
    fn 옛_방식은_32를_더한다() {
        assert_eq!(
            encode_wheel(Enc::Default, true, 0, 0),
            vec![0x1b, b'[', b'M', 32 + 64, 33, 33]
        );
    }

    /// 넓은 칸에서 한 바이트를 넘겨도 패닉하지 않는다.
    #[test]
    fn 옛_방식은_상한에서_잘린다() {
        let v = encode_wheel(Enc::Default, false, 400, 400);
        assert_eq!(v.len(), 6);
        assert_eq!(v[4], 255);
    }
}

#[cfg(test)]
mod cwd_tests {
    use super::*;

    /// 자식 셸의 경로를 실제로 읽어 낼 수 있는가.
    ///
    /// 공간이 `cd` 를 따라가려면 이 고리가 성립해야 한다. pid 를 얻고,
    /// sysinfo 가 그 프로세스를 보고, cwd 를 돌려주는 것 셋 다 필요하다.
    #[test]
    fn 자식_셸의_경로를_읽는다() {
        let mut s = PtySession::spawn_shell_in(24, 80, std::path::Path::new("/usr"))
            .expect("셸을 띄우지 못했다");
        let pid = s.pid();
        eprintln!("pid = {pid:?}");
        let pid = pid.expect("pid 를 못 얻었다");

        // 셸이 자리를 잡을 때까지 잠깐 기다린다.
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            s.pump();
        }

        let mut sys = sysinfo::System::new();
        let cwd = process_cwd(&mut sys, pid);
        eprintln!("cwd = {cwd:?}");
        assert!(cwd.is_some(), "cwd 를 못 읽었다 — 공간이 cd 를 따라갈 수 없다");
        assert!(cwd.unwrap().ends_with("usr"), "띄운 자리와 다르다");
    }
}

#[cfg(all(test, unix))]
mod terminal_regressions {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn real_pty_unicode_input_and_resize() {
        let mut session = PtySession::spawn_raw("/bin/sh", &["-c", "stty -echo; printf READY; IFS= read -r line; printf '<%s>\\n' \"$line\"; stty size"], 24, 80).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !session.screen().contents().contains("READY") {
            session.pump();
            assert!(Instant::now() < deadline, "shell did not become ready");
            std::thread::sleep(Duration::from_millis(10));
        }
        session.resize(31, 97).unwrap();
        assert_eq!(session.screen().size(), (31, 97));
        session.write("한글 café\r".as_bytes()).unwrap();
        loop {
            session.pump();
            let output = session.screen().contents();
            if output.contains("<한글 café>") && output.contains("31 97") { break; }
            assert!(Instant::now() < deadline, "PTY output: {output:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn codex_shim_preserves_arguments_and_cleans_up() {
        let args = vec!["-c".into(), "printf '%s\\n' \"$@\"".into(), "arg0".into()];
        let shim = CodexShim::new(std::path::Path::new("/bin/sh"), &args).unwrap();
        let path = shim.0.clone();
        for input in [vec!["a b", "한글", "a'b"], vec!["--no-alt-screen", "resume", "--last"]] {
            let result = std::process::Command::new(path.join("codex")).args(&input).output().unwrap();
            assert!(result.status.success());
            let output = String::from_utf8(result.stdout).unwrap();
            let expected = if input[0] == "--no-alt-screen" { input.clone() } else { std::iter::once("--no-alt-screen").chain(input.iter().copied()).collect() };
            assert_eq!(output.lines().collect::<Vec<_>>(), expected);
        }
        drop(shim);
        assert!(!path.exists());
    }

    #[test]
    fn output_arriving_while_scrolled_keeps_view_and_capacity() {
        let mut p = vt100::Parser::new(4, 12, 3);
        p.process(b"one\r\ntwo\r\nthree\r\nCOMPOSER\x1b[1;3r\x1b[3;1H\r\nfour");
        p.screen_mut().set_scrollback(1);
        let before = p.screen().contents();
        p.process(b"\r\nfive");
        assert_eq!(p.screen().scrollback(), 2);
        assert_eq!(p.screen().contents(), before);
        p.process(b"\r\nsix\r\nseven");
        p.screen_mut().set_scrollback(99);
        assert_eq!(p.screen().scrollback(), 3);
    }

    // Codex keeps a composer below its history and scrolls only rows 1..3.
    #[test]
    fn top_anchored_partial_scroll_retains_output_and_footer() {
        let mut p = vt100::Parser::new(5, 16, 2000);
        p.process(b"oldest\r\nsecond\r\nthird\r\nCOMPOSER\r\nSTATUS");
        p.process(b"\x1b[1;3r\x1b[3;1H\r\nnewest");
        assert!(p.screen().contents().contains("COMPOSER"));
        assert!(p.screen().contents().contains("STATUS"));
        p.screen_mut().set_scrollback(1);
        assert_eq!(p.screen().scrollback(), 1, "top row lost by partial scroll");
        assert!(p.screen().contents().starts_with("oldest"));
        p.screen_mut().set_scrollback(0);
        assert!(p.screen().contents().starts_with("second"));
    }

    #[test]
    fn interior_region_and_alternate_screen_do_not_pollute_history() {
        let mut p = vt100::Parser::new(5, 16, 2000);
        p.process(b"HEADER\r\none\r\ntwo\r\nthree\r\nFOOTER");
        p.process(b"\x1b[2;4r\x1b[4;1H\r\nnew");
        p.screen_mut().set_scrollback(99);
        assert_eq!(p.screen().scrollback(), 0);
        p.process(b"\x1b[?1049h\x1b[1;3r\x1b[3;1H\r\nalt");
        p.screen_mut().set_scrollback(99);
        assert_eq!(p.screen().scrollback(), 0);
    }

    #[test]
    fn scrollback_and_alternate_screen_restore() {
        let mut p = vt100::Parser::new(3, 12, 2000);
        p.process(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
        p.screen_mut().set_scrollback(2);
        assert_eq!(p.screen().scrollback(), 2);
        assert!(p.screen().contents().contains("one"));
        p.screen_mut().set_scrollback(0);
        p.process(b"\x1b[?1049hALT");
        p.screen_mut().set_scrollback(20);
        assert_eq!(p.screen().scrollback(), 0);
        p.process(b"\x1b[?1049l");
        assert!(p.screen().contents().contains("five"));
    }
}
