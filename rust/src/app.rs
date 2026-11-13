//! 앱 상태와 화면. 「공간 × 세션」 구조다.
//!
//! 화면 모드
//!   Panes    — 기본 화면. 선택된 공간의 세션들. 그 안은 자식의 실제 TUI 다.
//!   NewSpace — 새 공간의 경로를 입력하는 중
//!   Broadcast— 보이는 모든 에이전트에게 한 번에 묻는 중
//!
//! **시작하자마자 Panes 다.** 예전엔 질문을 먼저 받는 Idle 화면을 거쳤는데,
//! 그 화면은 에이전트를 띄우기 위한 관문일 뿐이라 한 번 쓰고 다시 볼 일이
//! 없었다. 지금은 기동과 동시에 셸 칸 하나가 뜨고, `[+]` 는 묻지 않고 칸을
//! 하나 더 붙인다. 에이전트는 그 칸에서 `claude`·`codex` 처럼 직접 친다.
//! 질문은 Ctrl+] 다음 n 으로 언제든 넣는다.
//!
//! 키는 기본적으로 **포커스된 세션의 자식에게 그대로** 간다. 그래야 권한 승인·
//! 슬래시 자동완성·esc 중단이 동작한다. 우리 조작은 tmux 처럼 프리픽스로 뺀다.
//!
//! 세션이 넷 이상이면 나란히 두지 않고 **탭**으로 바꾼다. 좁은 칸에 에이전트
//! TUI 를 밀어 넣으면 자식이 자기 화면을 접어버려 읽을 수 없기 때문이다.

use crate::{
    model::{Session, Space},
    pty::PtySession,
    theme,
    vtscreen::VtScreen,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthStr;

/// 이보다 많아지면 나란히 두지 않고 탭으로 바꾼다.
pub const MAX_SPLIT: usize = 3;

pub enum Mode {
    Panes,
    NewSpace,
    /// 보이는 모든 에이전트에게 한 번에 묻는 중
    Broadcast,
    /// 포커스된 칸의 마지막 답변을 나머지 칸으로 넘기는 중
    Relay,
    /// 탭 이름을 고쳐 쓰는 중
    RenameTab,
}

/// 마우스 클릭이 무엇을 가리키는가.
pub enum Hit {
    /// 세션에 포커스
    Focus(usize),
    /// 세션 닫기
    Close(usize),
    /// 공간 선택
    Space(usize),
    /// 새 공간 추가
    AddSpace,
    /// 새 칸 추가 (+). 묻지 않고 셸 칸을 하나 붙인다.
    AddSession,
    /// 전체 보기 탭
    ShowAll,
    /// 탭 줄의 칩. 한 번 누르면 포커스, 두 번 누르면 이름 고치기.
    Tab(usize),
}

struct Ask {
    group: u64, verifier: u64, target: crate::ctl::Target, baseline: Option<String>,
    injected_at: Option<Instant>, deadline: Option<Instant>,
    /// 넣은 글에서 공백을 뺀 것. 입력창에 그대로 남아 있는지 화면과 견준다.
    sent: String,
    /// 제출되지 않은 질문에 Enter 를 다시 보낸 횟수.
    enter_retries: u8,
}
/// 한 번의 `mai ctl ask` — 대상이 여럿이면 답을 모아 한 번에 돌려준다.
struct AskGroup {
    id: u64, requester: u64, targets: Vec<crate::ctl::Target>,
    question: String, requester_title: String, cwd: PathBuf,
    /// 답을 받은 뒤에도 검증 칸을 남겨 둔다(`--keep`). 기본은 1회성이라 닫는다.
    keep: bool,
    started: Instant,
    results: Vec<(crate::ctl::Target, crate::ctl::Response)>,
    waiters: Vec<Box<dyn crate::ctl::transport::Conn>>,
}
pub struct App {
    next_id: u64,
    ctl: Option<crate::ctl::server::Server>,
    asks: Vec<Ask>,
    groups: Vec<AskGroup>,
    asks_checked: Instant,
    /// ~/.multi-ai-cli/config.toml. bootstrap 에서 읽는다.
    config: crate::room::Config,
    pub spaces: Vec<Space>,
    pub active_space: usize,
    pub sessions: Vec<Session>,
    /// 포커스된 세션 (sessions 의 전역 인덱스)
    pub focus: usize,

    pub mode: Mode,
    /// 공간 추가·모두에게 묻기에서 타이핑 중인 문자열
    pub input: String,
    pub status: String,
    pub prefix: bool,
    pub quit: bool,

    /// 사이드바 스크롤 오프셋 (줄 단위)
    pub space_scroll: u16,
    pub agent_scroll: u16,
    /// 탭 모드에서도 전부 나란히 본다. [전체] 탭이 켜는 값이다.
    pub show_all: bool,

    // ---- 마지막 렌더의 클릭 대상. 마우스 판정에 쓴다. ----
    pane_hit: Vec<(usize, Rect)>,
    /// 칸의 **안쪽** 사각형. 자식 화면이 그려지는 자리라 휠 좌표를 여기서 뺀다.
    pub(crate) pane_inner: Vec<(usize, Rect)>,
    pub selection: Option<crate::selection::Selection>,
    pub mouse_capture: Option<(usize, Rect)>,
    /// 탭 줄의 칩. 칸 본문과 구분해야 «탭을 두 번 눌렀다»를 알 수 있다.
    tab_hit: Vec<(usize, Rect)>,
    close_hit: Vec<(usize, Rect)>,
    space_hit: Vec<(usize, Rect)>,
    agent_hit: Vec<(usize, Rect)>,
    add_space_hit: Option<Rect>,
    add_session_hit: Option<Rect>,
    all_tab_hit: Option<Rect>,
    /// 사이드바의 두 칸 영역. 휠 스크롤을 어디에 적용할지 가른다.
    spaces_area: Rect,
    agents_area: Rect,
    /// 마지막으로 그린 전체 영역. 키로 칸을 붙일 때 크기 계산에 쓴다.
    last_area: Rect,
    /// 자식의 경로를 물어보는 창구. 필요할 때만 만든다.
    sys: Option<sysinfo::System>,
    /// 셸 칸에서 도는 에이전트를 마지막으로 찾아본 시각.
    agents_checked: Instant,
    /// 인용 전달 상자에 띄울 요약. 무엇을 어디서 몇 자 가져왔는지.
    relay_preview: Option<String>,
    /// 이름을 고치는 중인 칸.
    rename_target: Option<usize>,
    /// 마지막 탭 클릭. 두 번 누름을 가르는 기준이다.
    last_tab_click: Option<(usize, Instant)>,
    /// 넘길 인용문과 그 출처 이름. 문장은 보낼 때 붙인다.
    relay_quote: Option<(String, crate::relay::Quote)>,
    /// 인용을 받을 후보. (세션 인덱스, 표시명, 켜짐)
    relay_targets: Vec<(usize, String, bool)>,
    /// 인용 상자의 포커스. 0 은 문장 입력, 1.. 은 n-1 번째 후보다.
    pub relay_focus: usize,
}

impl App {
    pub fn new() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            next_id: 1,
            ctl: None,
            asks: Vec::new(),
            groups: Vec::new(),
            asks_checked: Instant::now(),
            config: crate::room::Config::default(),
            spaces: vec![Space::new(cwd)],
            active_space: 0,
            sessions: Vec::new(),
            focus: 0,
            mode: Mode::Panes,
            input: String::new(),
            status: String::new(),
            prefix: false,
            quit: false,
            space_scroll: 0,
            agent_scroll: 0,
            show_all: false,
            pane_hit: Vec::new(),
            pane_inner: Vec::new(),
            selection: None,
            mouse_capture: None,
            tab_hit: Vec::new(),
            close_hit: Vec::new(),
            space_hit: Vec::new(),
            agent_hit: Vec::new(),
            add_space_hit: None,
            add_session_hit: None,
            all_tab_hit: None,
            spaces_area: Rect::ZERO,
            agents_area: Rect::ZERO,
            last_area: Rect::ZERO,
            sys: None,
            agents_checked: Instant::now(),
            rename_target: None,
            last_tab_click: None,
            relay_preview: None,
            relay_quote: None,
            relay_targets: Vec::new(),
            relay_focus: 0,
        }
    }

    /// 마지막으로 그린 전체 영역.
    ///
    /// 새 칸의 크기는 전체 영역에서 계산한다. 키 처리에는 터미널 핸들이 없어
    /// 매 프레임 sync_sizes 가 넘겨 주는 값을 기억해 둔다.
    pub fn last_area(&self) -> Rect {
        self.last_area
    }

    pub fn active_path(&self) -> PathBuf {
        self.spaces
            .get(self.active_space)
            .map(|s| s.path.clone())
            .unwrap_or_default()
    }

    /// 지금 보이는 세션들 — 선택된 공간에 속한 것만.
    ///
    /// 공간을 바꾸면 다른 공간의 세션은 **죽이지 않고 숨긴다.** 돌아오면 그대로다.
    pub fn visible(&self) -> Vec<usize> {
        (0..self.sessions.len())
            .filter(|&i| self.sessions[i].space == self.active_space)
            .collect()
    }

    /// 탭 모드인가 — 보이는 세션이 MAX_SPLIT 을 넘는가.
    pub fn tabbed(&self) -> bool {
        self.visible().len() > MAX_SPLIT
    }

    // ---------- 기동 ----------

    /// 기동 직후 **터미널 하나만** 띄운다.
    ///
    /// 예전엔 쓸 수 있는 에이전트를 전부 자동으로 올렸다. 그런데 늘 셋 다
    /// 필요한 것은 아니고, 안 쓸 참여자까지 켜 두면 칸이 좁아지고 각자 로그인·
    /// 업데이트 안내를 띄워 정리부터 해야 했다. 지금은 **빈 작업대에서 시작해
    /// 필요한 것만 사용자가 붙인다** — `[+]` 가 그 자리다.
    ///
    /// 터미널을 기본으로 두는 이유는, 무엇을 붙일지 정하기 전에도 그 경로에서
    /// 뭔가는 해 보게 되기 때문이다.
    pub fn bootstrap(&mut self, area: Rect) {
        self.config = crate::room::Config::load();
        crate::ctl::models::warm_agy_cache();
        if self.ctl.is_none() {
            match crate::ctl::server::Server::start() { Ok(server) => self.ctl = Some(server), Err(e) => self.status = format!("제어 소켓 기동 실패: {e}") }
        }
        self.spawn_terminal(area);
        self.sync_sizes(area);
        self.focus = self.visible().first().copied().unwrap_or(0);
        self.status = HINT.into();
    }

    // ---------- 공간 ----------

    pub fn add_space(&mut self, path: &str) -> Result<(), String> {
        let p = expand_tilde(path.trim());
        if !p.is_dir() {
            return Err(format!("디렉터리가 아니다: {}", p.display()));
        }
        // canonicalize 는 윈도우에서 `\\?\` 접두사를 붙인다. 경로를 지우고
        // 다시 쓰기 위해 벗긴다.
        let p = strip_verbatim(p.canonicalize().unwrap_or_else(|_| p.clone()));
        if let Some(i) = self.spaces.iter().position(|s| s.path == p) {
            self.select_space(i);
            return Ok(());
        }
        self.spaces.push(Space::new(p));
        self.select_space(self.spaces.len() - 1);
        Ok(())
    }

    /// 공간을 고른다. 그 공간의 세션이 보이고, 포커스도 그 안으로 옮긴다.
    pub fn select_space(&mut self, i: usize) {
        if i >= self.spaces.len() {
            return;
        }
        self.active_space = i;
        self.agent_scroll = 0;
        let vis = self.visible();
        if !vis.contains(&self.focus) {
            self.focus = vis.first().copied().unwrap_or(0);
        }
        // 세션이 없는 공간이어도 Panes 다. 빈 칸에 [+] 안내가 나온다.
        let _ = vis;
        self.mode = Mode::Panes;
    }

    // ---------- 세션 ----------

    /// 지금 공간에 **터미널** 칸을 하나 붙인다.
    ///
    /// `[+]` 가 하는 일이다. 예전엔 여기서 무엇을 띄울지(터미널/Claude/Codex/
    /// Gemini) 골랐다. 그런데 고를 것이 실은 하나뿐이었다 — 에이전트도 결국
    /// 셸에서 `claude` 를 치면 뜨고, 그 편이 인자·프로필을 그대로 쓸 수 있어
    /// 낫다. 그래서 상자를 없애고 **누르면 칸이 하나 더 생기는** 것으로 바꿨다.
    /// 셸은 환경이 정한다 — 윈도우는 PowerShell, 그 밖에서는 `$SHELL`(보통 zsh).
    /// 설정의 `shell` 에 경로가 있으면 그 셸이 이긴다.
    pub fn spawn_terminal(&mut self, area: Rect) {
        let n = (self.visible().len() + 1).min(MAX_SPLIT);
        let (h, w) = pane_size(area, n);
        let cwd = self.active_path();

        // 같은 공간에서 몇 번째 터미널인지 세어 이름을 붙인다.
        let used = self
            .visible()
            .iter()
            .filter(|&&i| self.sessions[i].is_terminal)
            .count();
        let title = if used == 0 {
            crate::pty::shell_name()
        } else {
            format!("{} {}", crate::pty::shell_name(), used + 1)
        };

        let mut sess = Session::terminal(&title, self.active_space);
        sess.id = self.next_id;
        self.next_id += 1;
        sess.cwd = Some(cwd.clone());
        let env = self.control_env(sess.id, "main");
        match PtySession::spawn_controlled(None, h, w, &cwd, &env, &[]) {
            Ok(pty) => {
                sess.pty = Some(pty);
                sess.size = (h, w);
                sess.started = Some(Instant::now());
                sess.seen = (0, Instant::now());
                self.sessions.push(sess);
                self.focus = self.sessions.len() - 1;
                self.mode = Mode::Panes;
                self.status = format!("터미널 «{title}» 을 {} 에서 띄웠다", cwd.display());
            }
            Err(e) => self.status = format!("터미널 기동 실패 — {e}"),
        }
    }

    /// 세션을 닫는다. 자식 프로세스도 정리하고 목록에서 뺀다.
    pub fn close_session(&mut self, i: usize) {
        if i >= self.sessions.len() {
            return;
        }
        if let Some(p) = self.sessions[i].pty.as_mut() {
            p.kill();
        }
        self.sessions.remove(i);
        // 뒤 인덱스가 한 칸씩 당겨졌다. 포커스를 보정한다.
        if self.focus > i {
            self.focus -= 1;
        }
        let vis = self.visible();
        if !vis.contains(&self.focus) {
            self.focus = vis.first().copied().unwrap_or(0);
        }
        if vis.is_empty() {
            self.input.clear();
            self.status = "이 공간에 세션이 없다. [+] 로 띄운다.".into();
        }
    }

    /// 보이는 세션 안에서 포커스를 옮긴다.
    pub fn cycle_focus(&mut self, delta: i32) {
        let vis = self.visible();
        if vis.is_empty() {
            return;
        }
        let cur = vis.iter().position(|&i| i == self.focus).unwrap_or(0) as i32;
        let n = vis.len() as i32;
        self.focus = vis[(((cur + delta) % n + n) % n) as usize];
    }

    /// 보이는 세션 중 n 번째로 포커스.
    pub fn focus_nth(&mut self, n: usize) {
        if let Some(&i) = self.visible().get(n) {
            self.focus = i;
        }
    }

    pub fn focused(&mut self) -> Option<&mut PtySession> {
        self.sessions.get_mut(self.focus).and_then(|s| s.pty.as_mut())
    }

    /// 포커스된 터미널이 `cd` 했으면 공간을 그쪽으로 옮긴다.
    ///
    /// **터미널만 공간을 움직인다.** 에이전트는 자기가 뜬 자리에서 경로를 바꾸지
    /// 않고, 바꾼다 해도 그건 도구가 잠깐 들어간 것이지 사용자가 옮겨 간 것이
    /// 아니다. 사용자의 조타는 셸에서 일어난다.
    ///
    /// 이미 떠 있는 칸들은 자기 경로 그대로다 — 남의 프로세스 cwd 를 바꿀 수는
    /// 없다. 바뀌는 것은 «다음에 뜰 칸이 어디서 뜨는가» 와 화면에 적히는 경로다.
    fn follow_cwd(&mut self) {
        let Some(s) = self.sessions.get(self.focus) else { return };
        if !s.is_terminal {
            return;
        }
        // 초당 한 번이면 충분하다. 매 프레임 프로세스를 뒤지는 것은 낭비다.
        if s.cwd_at.is_some_and(|t| t.elapsed() < Duration::from_millis(900)) {
            return;
        }
        let Some(pid) = s.pty.as_ref().and_then(|p| p.pid()) else { return };

        // PowerShell 은 `cd` 해도 프로세스 cwd 가 안 바뀐다. 프롬프트에 적힌
        // 경로가 있으면 그쪽을 믿고, 프롬프트가 안 보이면(에이전트가 화면을
        // 차지한 동안) 마지막으로 읽은 값을 쓴다.
        let from_prompt = s.pty.as_ref().and_then(|p| crate::pty::prompt_cwd(&p.screen()));
        let shell_cwd = from_prompt.or_else(|| s.shell_cwd.clone());

        // 그 밖의 셸은 프로세스 cwd 로 본다. 진단(--probe)과 **같은 경로**를
        // 쓴다. 갈라 두면 진단은 통과하는데 앱은 안 되는 상황이 생긴다.
        let found = match shell_cwd.clone() {
            Some(p) => Some(p),
            None => {
                let sys = self.sys.get_or_insert_with(sysinfo::System::new);
                crate::pty::process_cwd(sys, pid)
            }
        };

        let i = self.focus;
        if let Some(s) = self.sessions.get_mut(i) {
            s.cwd_at = Some(Instant::now());
            s.cwd = found.clone();
            s.shell_cwd = shell_cwd;
        }
        let Some(cwd) = found else { return };
        if let Some(sp) = self.spaces.get_mut(self.active_space) {
            if sp.path != cwd {
                sp.set_path(cwd);
            }
        }
    }

    /// 셸 칸에서 사용자가 직접 띄운 에이전트를 알아본다.
    ///
    /// 칸은 전부 셸로 뜨고 에이전트는 사용자가 그 안에서 친다. 누구의 칸인지
    /// 알아야 색 점을 맞게 칠하고 답 넘기기가 세션 기록을 찾을 수 있다.
    /// 에이전트가 끝나면 다시 셸 칸으로 돌아간다. 우리가 직접 띄운 요청 칸은
    /// 이미 누구인지 알고 있으므로 건드리지 않는다.
    fn detect_agents(&mut self) {
        // 프로세스 목록 전체를 훑는 일이라 자주 하지 않는다.
        if self.agents_checked.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.agents_checked = Instant::now();
        let targets: Vec<(usize, u32)> = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_terminal)
            .filter_map(|(i, s)| Some((i, s.pty.as_ref()?.pid()?)))
            .collect();
        if targets.is_empty() {
            return;
        }
        let pids: Vec<u32> = targets.iter().map(|t| t.1).collect();
        let sys = self.sys.get_or_insert_with(sysinfo::System::new);
        let found = crate::pty::agents_under(sys, &pids);
        for ((i, _), agent) in targets.into_iter().zip(found) {
            let id = agent.unwrap_or("");
            let s = &mut self.sessions[i];
            if s.agent_id == id {
                continue;
            }
            s.agent_id = id.into();
            // 사용자가 이름을 고치지 않은 칸은 지금 도는 에이전트의 이름을 쓴다.
            // 에이전트가 끝나면 셸 이름으로 돌아간다. 같은 셸이 여럿일 때 붙인
            // 번호(`powershell 2`)는 그대로 옮긴다.
            if s.user_titled || s.shell_title.is_empty() {
                continue;
            }
            s.title = if id.is_empty() {
                s.shell_title.clone()
            } else {
                let name = crate::ctl::display(id);
                let number = s
                    .shell_title
                    .rsplit_once(' ')
                    .map(|(_, n)| n)
                    .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
                match number {
                    Some(n) => format!("{name} {n}"),
                    None => name.to_string(),
                }
            };
        }
    }

    /// 살아 있는 세션의 출력을 먹이고, 준비된 세션에 프롬프트를 넣는다.
    pub fn tick(&mut self) {
        self.follow_cwd();
        self.detect_agents();
        self.receive_asks();
        for s in self.sessions.iter_mut() {
            let before = s.pty.as_ref().map(|p| p.rx_bytes).unwrap_or(0);
            if let Some(p) = s.pty.as_mut() {
                p.pump();
            }
            let after = s.pty.as_ref().map(|p| p.rx_bytes).unwrap_or(0);
            s.refresh_subs(after != before);

            // 대화상자가 떠 있으면 주입을 미룬다.
            //
            // 새 경로에서는 신뢰·MCP 확인을 먼저 묻는데, 그때 밀어 넣으면 우리
            // 개행이 사용자 대신 항목을 확정하고 질문은 삼켜진다(실측). 사용자
            // 눈에는 「화살표와 Enter 가 안 먹는」 것으로 보인다 — 누르기도 전에
            // 상자가 닫히기 때문이다.
            let blocked = s
                .pty
                .as_ref()
                .is_some_and(|p| crate::modal::open(&p.screen()));
            let ready = s.ready_to_inject();
            if s.pending.is_some() && ready && !blocked && !s.user_typed {
                let text = s.pending.take().unwrap_or_default();
                if let Some(p) = s.pty.as_mut() {
                    let _ = p.write(text.as_bytes());
                    // 개행은 살짝 뒤에. 붙여 보내면 입력창이 아직 다 안 그려진
                    // 에이전트에서 첫 글자가 잘린다.
                    std::thread::sleep(Duration::from_millis(120));
                    let _ = p.write(&[13]);
                }
            }
        }
        self.finish_asks();
    }

    /// 화면 크기가 바뀌면 각 PTY 도 자기 칸 크기로 맞춘다.
    pub fn sync_sizes(&mut self, area: Rect) {
        self.last_area = area;
        let n = if self.tabbed() && !self.show_all { 1 } else { self.visible().len() };
        let (h, w) = pane_size(area, n);
        for i in self.visible() {
            let s = &mut self.sessions[i];
            if s.size != (h, w) {
                if let Some(p) = s.pty.as_mut() {
                    let _ = p.resize(h, w);
                    s.size = (h, w);
                }
            }
        }
    }

    /// 보이는 모든 에이전트에게 같은 질문을 넣는다.
    ///
    /// 직접 쓰지 않고 **대기 프롬프트로 걸어 둔다.** 그래야 대화상자가 떠 있는
    /// 칸에서는 상자가 닫힐 때까지 기다리는 규칙이 그대로 적용된다.
    pub fn broadcast(&mut self, question: &str) {
        let now = Instant::now();
        let mut n = 0;
        for i in self.visible() {
            let s = &mut self.sessions[i];
            if s.pty.is_none() {
                continue;
            }
            let rx = s.pty.as_ref().map(|p| p.rx_bytes).unwrap_or(0);
            s.pending = Some(question.to_string());
            // 기동 때처럼 잠잠해짐을 다시 잰다. 지금 무언가 그리는 중일 수 있다.
            s.started = Some(now);
            s.seen = (rx, now);
            // 이번 주입은 사용자가 방금 지시한 것이다. 예전 타이핑 기록은 지운다.
            s.user_typed = false;
            n += 1;
        }
        self.status = if n == 0 {
            "보낼 세션이 없다".into()
        } else {
            format!("{n}개 세션에 질문을 넣는다")
        };
    }

    /// 포커스된 칸의 마지막 답변을 꺼내 «넘길 준비»를 한다.
    ///
    /// 상자를 열기 전에 미리 꺼내는 이유는, 넘길 것이 없으면 상자를 띄우는
    /// 것 자체가 헛수고이기 때문이다. 몇 자를 어디서 가져왔는지도 이때 정해져
    /// 상자 제목에 그대로 나온다.
    pub fn prepare_relay(&mut self) {
        self.relay_preview = None;
        self.relay_quote = None;
        self.relay_targets.clear();
        self.relay_focus = 0;

        let Some(src) = self.sessions.get(self.focus) else {
            self.status = "넘길 칸이 없다".into();
            return;
        };
        if self.visible().len() < 2 {
            self.status = "받을 칸이 없다. 칸이 둘 이상이어야 한다.".into();
            return;
        }
        let title = src.title.clone();
        let cwd = self.active_path();

        // 1순위 세션 기록, 2순위 화면. relay 모듈이 그 순서를 안다.
        let quote = crate::relay::last_answer(&src.agent_id, &cwd)
            .or_else(|| src.pty.as_ref().map(|p| crate::relay::from_screen(&p.screen())));
        let Some(quote) = quote else {
            self.status = format!("{title} 에서 넘길 답변을 찾지 못했다");
            return;
        };
        if quote.text.trim().is_empty() {
            self.status = format!("{title} 의 답변이 비어 있다");
            return;
        }

        let n = quote.text.chars().count();
        let cut = if quote.truncated { " · 앞부분 생략" } else { "" };
        self.relay_preview = Some(format!("{title} 의 답 {n}자 ({}{cut})", quote.origin.label()));
        self.relay_quote = Some((title, quote));

        // 출처를 뺀 나머지가 후보다. 처음엔 전부 켜 둔다 — 흔한 쓰임이
        // «나머지 전원에게 물어본다» 이고, 좁히는 건 그다음이다.
        let src_i = self.focus;
        self.relay_targets = self
            .visible()
            .into_iter()
            .filter(|&i| i != src_i && self.sessions[i].pty.is_some())
            .map(|i| (i, self.sessions[i].title.clone(), true))
            .collect();

        self.mode = Mode::Relay;
        self.input.clear();
    }

    /// 인용 상자 안에서 포커스를 옮긴다. 문장 → 후보들 → 다시 문장.
    pub fn cycle_relay_focus(&mut self, delta: i32) {
        let n = self.relay_targets.len() as i32 + 1;
        self.relay_focus = (((self.relay_focus as i32 + delta) % n + n) % n) as usize;
    }

    /// 포커스된 후보를 켜고 끈다.
    pub fn toggle_relay_target(&mut self) {
        if self.relay_focus == 0 {
            return;
        }
        if let Some(t) = self.relay_targets.get_mut(self.relay_focus - 1) {
            t.2 = !t.2;
        }
    }

    /// 지금 켜져 있는 후보 수.
    pub fn relay_selected(&self) -> usize {
        self.relay_targets.iter().filter(|t| t.2).count()
    }

    /// 탭을 두 번 눌렀다. 이름을 고쳐 쓸 준비를 한다.
    ///
    /// 이름은 순전히 사람이 보라고 있는 것이다. 「Claude」 셋을 띄워 놓고
    /// 어느 칸이 무엇을 하는 중인지 구분하려면 제품이 붙인 이름으로는 모자란다.
    pub fn begin_rename(&mut self, i: usize) {
        let Some(s) = self.sessions.get(i) else { return };
        self.input = s.title.clone();
        self.rename_target = Some(i);
        self.mode = Mode::RenameTab;
    }

    /// 고쳐 쓴 이름을 반영한다. 빈 이름은 무시한다 — 지워지면 고를 수가 없다.
    pub fn commit_rename(&mut self, name: &str) {
        let Some(i) = self.rename_target.take() else { return };
        let name = name.trim();
        if !name.is_empty() {
            if let Some(s) = self.sessions.get_mut(i) {
                s.title = name.to_string();
                // 직접 고친 이름은 에이전트가 바뀌어도 우리가 덮어쓰지 않는다.
                s.user_titled = true;
                self.status = format!("이름을 «{name}» 로 바꿨다");
            }
        }
        self.mode = Mode::Panes;
    }

    pub fn cancel_rename(&mut self) {
        self.rename_target = None;
        self.mode = Mode::Panes;
    }

    /// 탭을 눌렀다. 짧은 사이에 같은 탭을 두 번이면 이름 고치기다.
    ///
    /// crossterm 은 «두 번 누름» 을 따로 알려주지 않는다. 시각과 대상을
    /// 우리가 들고 있다가 판정한다. 400ms 는 흔한 데스크톱 기준값이다.
    pub fn tab_click(&mut self, i: usize) {
        let double = matches!(
            self.last_tab_click,
            Some((prev, t)) if prev == i && t.elapsed() < Duration::from_millis(400)
        );
        if double {
            self.last_tab_click = None;
            self.begin_rename(i);
            return;
        }
        self.last_tab_click = Some((i, Instant::now()));
        self.focus = i;
        self.show_all = false;
    }

    /// 인용 전달을 물린다.
    pub fn cancel_relay(&mut self) {
        self.relay_preview = None;
        self.relay_quote = None;
        self.relay_targets.clear();
        self.relay_focus = 0;
        self.mode = Mode::Panes;
    }

    /// 준비된 인용문에 한 문장을 붙여 **나머지 보이는 칸 전부**에 넣는다.
    ///
    /// 출처 칸에는 넣지 않는다. 자기 답을 자기에게 다시 주는 꼴이다.
    ///
    /// 직접 쓰지 않고 대기 프롬프트로 걸어 두는 것은 broadcast 와 같은 이유다 —
    /// 대화상자가 떠 있는 칸에서는 상자가 닫힐 때까지 기다려야 한다.
    pub fn relay(&mut self, sentence: &str) -> bool {
        if self.relay_selected() == 0 {
            self.status = "받을 칸을 하나 이상 골라야 한다 (Tab 이동 · Space 켜기/끄기)".into();
            return false;
        }
        let Some((title, quote)) = self.relay_quote.take() else { return false };
        let body = crate::relay::compose(&title, &quote, sentence);

        let now = Instant::now();
        let targets: Vec<usize> = self
            .relay_targets
            .iter()
            .filter(|t| t.2)
            .map(|t| t.0)
            .collect();
        let mut names = Vec::new();
        for i in targets {
            // 상자가 떠 있는 동안 칸이 닫혔을 수 있다. 인덱스를 다시 본다.
            let Some(s) = self.sessions.get_mut(i) else { continue };
            if s.pty.is_none() {
                continue;
            }
            let rx = s.pty.as_ref().map(|p| p.rx_bytes).unwrap_or(0);
            s.pending = Some(body.clone());
            s.started = Some(now);
            s.seen = (rx, now);
            s.user_typed = false;
            names.push(s.title.clone());
        }
        self.status = if names.is_empty() {
            "받을 칸이 없다".into()
        } else {
            format!("{title} 의 답을 {} 에게 넘겼다", names.join(", "))
        };
        self.relay_preview = None;
        self.relay_targets.clear();
        true
    }

    // ---------- 마우스 ----------

    pub fn hit_test(&self, x: u16, y: u16) -> Option<Hit> {
        if let Some(r) = &self.add_space_hit {
            if contains(r, x, y) {
                return Some(Hit::AddSpace);
            }
        }
        if let Some(r) = &self.add_session_hit {
            if contains(r, x, y) {
                return Some(Hit::AddSession);
            }
        }
        if let Some(r) = &self.all_tab_hit {
            if contains(r, x, y) {
                return Some(Hit::ShowAll);
            }
        }
        for (i, r) in &self.tab_hit {
            if contains(r, x, y) {
                return Some(Hit::Tab(*i));
            }
        }
        // 닫기 버튼이 패널 영역 안에 있으므로 먼저 본다.
        for (i, r) in &self.close_hit {
            if contains(r, x, y) {
                return Some(Hit::Close(*i));
            }
        }
        for (i, r) in &self.space_hit {
            if contains(r, x, y) {
                return Some(Hit::Space(*i));
            }
        }
        for (i, r) in &self.agent_hit {
            if contains(r, x, y) {
                return Some(Hit::Focus(*i));
            }
        }
        for (i, r) in &self.pane_hit {
            if contains(r, x, y) {
                return Some(Hit::Focus(*i));
            }
        }
        None
    }

    /// 휠 스크롤. **커서가 놓인 것만** 움직인다.
    ///
    /// 칸 위라면 자식의 몫이다. 에이전트들은 스스로 마우스 추적을 켜고
    /// (`ESC[?1000h` 등) 자기 화면을 자기가 스크롤한다. 우리가 대신 굴리면
    /// 자식이 그린 것과 어긋난다. 그래서 **좌표만 칸 기준으로 바꿔 넘긴다.**
    ///
    /// 자식이 마우스를 안 받으면(평범한 셸) 그때만 우리 스크롤백을 움직인다.
    /// 칸 밖이면 사이드바 차례다.
    pub fn scroll(&mut self, x: u16, y: u16, delta: i32) {
        if let Some(&(i, inner)) = self.pane_inner.iter().find(|(_, r)| contains(r, x, y)) {
            let (col, row) = (x - inner.x, y - inner.y);
            let up = delta < 0;
            let steps = delta.unsigned_abs().max(1);
            if let Some(p) = self.sessions.get_mut(i).and_then(|s| s.pty.as_mut()) {
                for _ in 0..steps {
                    if !p.mouse_wheel(up, col, row) {
                        p.scroll_view(delta);
                        break;
                    }
                }
            }
            return;
        }

        let target = if contains(&self.spaces_area, x, y) {
            Some((&mut self.space_scroll, self.spaces.len() + 1))
        } else if contains(&self.agents_area, x, y) {
            let n = self.visible().len();
            Some((&mut self.agent_scroll, n))
        } else {
            None
        };
        let Some((off, total)) = target else { return };
        let max = total.saturating_sub(1) as u16;
        *off = ((*off as i32 + delta).clamp(0, max as i32)) as u16;
    }

    // ---------- 렌더 ----------

    pub fn draw(&mut self, f: &mut Frame) {
        match self.mode {
            Mode::Panes => self.draw_panes(f),
            Mode::NewSpace => {
                self.draw_panes(f);
                self.draw_new_space(f);
            }
            Mode::Broadcast => {
                self.draw_panes(f);
                self.draw_prompt_box(f, " 모두에게 묻기 (Enter 보내기 · Esc 취소) ");
            }
            Mode::Relay => {
                self.draw_panes(f);
                self.draw_relay_box(f);
            }
            Mode::RenameTab => {
                self.draw_panes(f);
                self.draw_prompt_box(f, " 탭 이름 (Enter 확정 · Esc 취소) ");
            }
        }
    }

    fn draw_panes(&mut self, f: &mut Frame) {
        // 클릭 대상은 **프레임 시작에 한 번** 비운다.
        //
        // 예전엔 탭 모드에서 비우지 않아 프레임마다 쌓였다. hit_test 는 먼저
        // 맞는 것을 돌려주므로 배치가 달랐던 **옛 프레임의 좌표**가 이겼고,
        // 세션을 닫거나 추가한 뒤로는 엉뚱한 에이전트로 옮겨졌다.
        self.pane_hit.clear();
        self.pane_inner.clear();
        self.tab_hit.clear();
        self.close_hit.clear();
        self.add_session_hit = None;
        self.all_tab_hit = None;

        let area = f.area();
        let vert = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(3)])
            .split(area);

        // 화면 전체를 한 바탕으로 깐다. 이 뒤에 그리는 것들은 색을 따로 주지
        // 않으면 이 바탕과 글자색을 그대로 쓴다.
        f.render_widget(
            Block::default().style(Style::default().bg(theme::bg()).fg(theme::fg())),
            area,
        );

        // 최상단은 첫 질문이 아니라 **현재 공간의 경로**다.
        // 질문은 각 에이전트 화면 안에 이미 남아 있고, 여기서 늘 필요한 정보는
        // 「지금 어디서 돌고 있는가」다.
        f.render_widget(
            Paragraph::new(format!(" {}", self.active_path().to_string_lossy()))
                .style(Style::default().add_modifier(Modifier::BOLD))
                .block(
                    Block::default()
                        .borders(Borders::BOTTOM)
                        .border_style(Style::default().fg(theme::line())),
                ),
            vert[0],
        );
        // 상단 바 오른쪽 끝에 앱 이름을 흐리게 둔다. 세 칸만 덮는다 — 줄 전체에
        // 그리면 경로 글자까지 흐린 색이 된다.
        if vert[0].width >= 4 {
            f.render_widget(
                Paragraph::new("mai").style(Style::default().fg(theme::dim())),
                Rect { x: vert[0].right() - 4, width: 3, height: 1, ..vert[0] },
            );
        }

        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(crate::sidebar::WIDTH), Constraint::Min(20)])
            .split(vert[1]);
        crate::sidebar::draw(f, main[0], self);

        // 탭 줄과 칸들은 사이드바 구분선·화면 끝에서 한 칸씩 띄운다.
        let content = Rect { x: main[1].x + 1, width: main[1].width.saturating_sub(2), ..main[1] };
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(3)])
            .split(content);
        self.draw_tabbar(f, right[0]);
        self.draw_bodies(f, right[1]);
    }

    /// 탭 줄. 세션 칩들과 끝의 `+`.
    ///
    /// 탭 모드가 아니어도 그린다 — `+` 가 늘 같은 자리에 있어야 찾기 쉽다.
    fn draw_tabbar(&mut self, f: &mut Frame, area: Rect) {
        let vis = self.visible();
        let tabbed = vis.len() > MAX_SPLIT;

        let mut spans: Vec<Span> = Vec::new();
        let mut x = area.x;

        // [전체] — 탭 모드에서 모든 에이전트를 한 번에 보는 탭.
        // 탭 모드가 아니면 이미 다 보이므로 그리지 않는다.
        if tabbed {
            let label = " 전체 ";
            let w = UnicodeWidthStr::width(label) as u16;
            let style = if self.show_all {
                Style::default().fg(Color::Black).bg(theme::focus()).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::dim())
            };
            spans.push(Span::styled(label, style));
            spans.push(Span::raw(" "));
            self.all_tab_hit = Some(Rect { x, y: area.y, width: w, height: 1 });
            x += w + 1;
        }

        for &i in &vis {
            let s = &self.sessions[i];
            // 전체 보기 중에는 어떤 칩도 «선택된 탭»이 아니다. [전체] 가 선택돼 있다.
            let focused = i == self.focus && !self.show_all;
            let label = format!(" {} ", s.title);
            let w = UnicodeWidthStr::width(label.as_str()) as u16;
            let style = if focused {
                Style::default().fg(Color::Black).bg(theme::focus()).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::dim())
            };
            spans.push(Span::styled(label, style));
            spans.push(Span::raw(" "));
            // 셋 이하일 때도 칩으로 고를 수 있게 한다. 나란히 다 보이더라도
            // 「지금 키가 어디로 가는지」를 칩으로 바꾸는 편이 빠르다.
            if x + w <= area.right() {
                self.tab_hit.push((i, Rect { x, y: area.y, width: w, height: 1 }));
            }
            x += w + 1;
        }
        // 새 세션 버튼
        if x + 3 <= area.right() {
            spans.push(Span::styled(
                "[+]",
                Style::default().fg(theme::ok()).add_modifier(Modifier::BOLD),
            ));
            self.add_session_hit = Some(Rect { x, y: area.y, width: 3, height: 1 });
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn draw_bodies(&mut self, f: &mut Frame, area: Rect) {
        let vis = self.visible();
        if vis.is_empty() {
            f.render_widget(
                Paragraph::new("이 공간에 세션이 없다. [+] 또는 Ctrl+] 다음 t 로 칸을 붙인다.")
                    .style(Style::default().fg(theme::dim())),
                area,
            );
            return;
        }

        // 넷 이상이면 포커스된 하나만 크게. 셋 이하면, 또는 [전체] 탭이면 나란히.
        let shown: Vec<usize> = if vis.len() > MAX_SPLIT && !self.show_all {
            vec![if vis.contains(&self.focus) { self.focus } else { vis[0] }]
        } else {
            vis.clone()
        };
        let n = shown.len().max(1);
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Ratio(1, n as u32); n])
            .spacing(1)
            .split(area);

        for (slot, &i) in shown.iter().enumerate() {
            let s = &self.sessions[i];
            let a = cols[slot];
            let focused = i == self.focus;
            let border = if focused { theme::focus() } else { theme::line() };
            // 제목줄 — 누구의 칸인지 색 점과 이름으로 보인다. AI 가 `mai ctl ask` 로
            // 연 칸은 요청받은 칸임을 덧붙인다(검증만이 아니라 여러 작업에 쓰인다).
            let mut title = vec![
                Span::styled(" ● ", Style::default().fg(theme::agent(&s.agent_id))),
                Span::raw(format!("{} ", s.title)),
            ];
            if s.verifier_of.is_some() {
                title.push(Span::styled("· 요청받음 ", Style::default().fg(theme::dim())));
            } else if s.is_terminal
                && !s.agent_id.is_empty()
                && !s.title.contains(crate::ctl::display(&s.agent_id))
            {
                // 사용자가 이름을 고쳐 쓴 칸은 이름에 에이전트가 드러나지 않는다.
                // 누가 돌고 있는지 덧붙인다.
                title.push(Span::styled(
                    format!("· {} ", crate::ctl::display(&s.agent_id)),
                    Style::default().fg(theme::dim()),
                ));
            }
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(if focused { BorderType::Thick } else { BorderType::Plain })
                .border_style(Style::default().fg(border));
            let frame = block.inner(a);
            f.render_widget(block, a);

            // 테두리 안쪽 첫 줄이 제목줄이고, 그 아래 구분선을 하나 긋는다.
            // 나머지가 에이전트 화면이 그려지는 본문이다.
            let head = Rect { height: frame.height.min(1), ..frame };
            let rule = Rect { y: frame.y + 1, height: frame.height.saturating_sub(1).min(1), ..frame };
            let inner = Rect { y: frame.y + 2, height: frame.height.saturating_sub(2), ..frame };
            f.render_widget(Paragraph::new(Line::from(title)), head);
            f.render_widget(
                Paragraph::new("─".repeat(rule.width as usize))
                    .style(Style::default().fg(theme::line())),
                rule,
            );

            if head.width >= 6 && head.height > 0 {
                let btn = Rect { x: head.right().saturating_sub(4), y: head.y, width: 3, height: 1 };
                f.render_widget(
                    Paragraph::new("[x]").style(Style::default().fg(theme::dim())),
                    btn,
                );
                self.close_hit.push((i, btn));
            }
            self.pane_hit.push((i, a));
            self.pane_inner.push((i, inner));

            if let Some(p) = s.pty.as_ref() {
                let selection = self.selection.as_ref().filter(|sel| sel.pane == i);
                let screen = selection.map(|sel| sel.screen.clone()).unwrap_or_else(|| p.screen());
                f.render_widget(VtScreen::new(&screen), inner);
                if let Some(sel) = selection { sel.highlight(inner, f.buffer_mut()); }
                if focused && selection.is_none() && screen.scrollback() == 0 && !screen.hide_cursor() {
                    let (r, c) = screen.cursor_position();
                    f.set_cursor_position((
                        (inner.x + c).min(inner.right().saturating_sub(1)),
                        (inner.y + r).min(inner.bottom().saturating_sub(1)),
                    ));
                }
            }
        }
    }

    /// 인용 전달 상자 — 받을 칸을 고르고 한 문장을 적는다.
    ///
    /// 대상 지정을 **Alt+숫자 같은 조합키로 하지 않는다.** macOS 터미널은 기본
    /// 설정에서 Option 을 Meta 로 보내지 않아 그 키가 통째로 안 오는 환경이 있다.
    /// Tab 으로 옮기고 Space 로 켜고 끄는 방식은 어디서나 같게 동작한다.
    fn draw_relay_box(&self, f: &mut Frame) {
        let area = f.area();
        let title = match &self.relay_preview {
            Some(p) => format!(" {p} "),
            None => " 넘길 답변을 찾지 못했다 (Esc) ".to_string(),
        };
        let r = center(area, area.width.min(80), 6);
        f.render_widget(Clear, r);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::focus()))
            .style(Style::default().bg(theme::bg()).fg(theme::fg()))
            .title(title);
        let inner = block.inner(r);
        f.render_widget(block, r);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        // 받을 칸 — 켜진 것만 인용을 받는다.
        let mut spans: Vec<Span> = vec![Span::styled(
            "받을 칸  ",
            Style::default().fg(theme::dim()),
        )];
        for (i, (_, name, on)) in self.relay_targets.iter().enumerate() {
            let focused = self.relay_focus == i + 1;
            let mut st = Style::default().fg(if *on { theme::ok() } else { theme::dim() });
            if focused {
                st = st.add_modifier(Modifier::REVERSED | Modifier::BOLD);
            }
            spans.push(Span::styled(
                format!("[{}] {name}", if *on { "x" } else { " " }),
                st,
            ));
            spans.push(Span::raw("   "));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), rows[0]);

        // 한 문장.
        let on_input = self.relay_focus == 0;
        let mut spans: Vec<Span> = vec![Span::styled(
            "한 마디  ",
            Style::default().fg(if on_input { theme::focus() } else { theme::dim() }),
        )];
        spans.push(Span::raw(self.input.as_str()));
        f.render_widget(Paragraph::new(Line::from(spans)), rows[1]);

        f.render_widget(
            Paragraph::new("Tab 이동 · Space 켜기/끄기 · Enter 보내기 · Esc 취소")
                .style(Style::default().fg(theme::dim())),
            rows[2],
        );

        if on_input {
            // "한 마디  " 만큼 오른쪽에서 시작한다.
            let off = UnicodeWidthStr::width("한 마디  ") as u16;
            let field = Rect { x: rows[1].x + off, width: rows[1].width.saturating_sub(off), ..rows[1] };
            put_cursor(f, field, &self.input);
        }
    }

    /// 새 공간 경로 입력 상자.
    fn draw_new_space(&self, f: &mut Frame) {
        self.draw_prompt_box(f, " 새 공간 경로 (Tab 완성 · Enter 추가 · Esc 취소) ");
    }

    /// 한 줄 입력 상자. 제목만 다르고 나머지는 같다.
    fn draw_prompt_box(&self, f: &mut Frame, title: &str) {
        let area = f.area();
        let r = center(area, area.width.min(70), 3);
        f.render_widget(Clear, r);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::focus()))
            .style(Style::default().bg(theme::bg()).fg(theme::fg()))
            .title(title.to_string());
        let inner = block.inner(r);
        f.render_widget(block, r);
        f.render_widget(Paragraph::new(self.input.as_str()), inner);
        put_cursor(f, inner, &self.input);
    }

    /// 새 공간 입력에서 Tab — 경로를 채운다.
    ///
    /// 디렉터리만 후보로 둔다. 공간은 디렉터리이기 때문이다. 후보가 하나면
    /// 끝까지 채우고 구분자를 붙인다. 여럿이면 **공통 접두사까지만** 채우고
    /// 후보를 상태줄에 보여준다 — 셸과 같은 동작이다.
    pub fn complete_path(&mut self) {
        // `~` 는 먼저 풀어둔다. 그래야 이후 조작이 실제 경로 위에서 이뤄진다.
        let sep = std::path::MAIN_SEPARATOR;
        let mut expanded = expand_tilde(&self.input).to_string_lossy().into_owned();
        // 구분자로 끝났다면 「이 안을 보여달라」는 뜻이다. 확장하면서 사라지므로
        // 되살린다. 안 그러면 `~/` 가 홈의 **이름**을 완성하려 든다.
        if self.input.trim_end().ends_with(['/', '\\']) && !expanded.ends_with(sep) {
            expanded.push(sep);
        }
        let (dir, prefix) = match expanded.rfind(['/', '\\']) {
            Some(i) => (expanded[..=i].to_string(), expanded[i + 1..].to_string()),
            // 구분자가 없으면 현재 디렉터리에서 찾는다.
            None => (format!(".{sep}"), expanded.clone()),
        };

        let Ok(rd) = std::fs::read_dir(&dir) else {
            self.input = expanded;
            self.status = format!("읽을 수 없는 경로: {dir}");
            return;
        };
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| starts_with_ci(n, &prefix))
            .collect();
        names.sort_by_key(|a| a.to_lowercase());

        match names.len() {
            0 => {
                self.input = expanded;
                self.status = "일치하는 디렉터리가 없다".into();
            }
            1 => {
                self.input = format!("{dir}{}{sep}", names[0]);
                self.status = String::new();
            }
            _ => {
                self.input = format!("{dir}{}", common_prefix(&names));
                let shown: Vec<&str> = names.iter().take(6).map(String::as_str).collect();
                self.status = format!(
                    "{}개 후보 — {}{}",
                    names.len(),
                    shown.join(", "),
                    if names.len() > 6 { " …" } else { "" }
                );
            }
        }
    }

    // 사이드바가 자기 영역과 클릭 대상을 등록한다.
    pub(crate) fn set_spaces_area(&mut self, r: Rect) {
        self.spaces_area = r;
        self.space_hit.clear();
        self.add_space_hit = None;
    }
    pub(crate) fn set_agents_area(&mut self, r: Rect) {
        self.agents_area = r;
        self.agent_hit.clear();
    }
    pub(crate) fn push_space_hit(&mut self, i: usize, r: Rect) {
        self.space_hit.push((i, r));
    }
    pub(crate) fn push_agent_hit(&mut self, i: usize, r: Rect) {
        self.agent_hit.push((i, r));
    }
    pub(crate) fn set_add_space_hit(&mut self, r: Rect) {
        self.add_space_hit = Some(r);
    }
}

/// 윈도우 `\\\\?\\` 접두사를 벗긴다.
///
/// `canonicalize()` 는 길이 제한을 넘는 경로까지 다루려고 이 접두사를 붙여 준다.
/// 우리에겐 해가 된다 — 화면에 그대로 나오고, 이 표기를 못 읽는 프로그램이 있어
/// 자식의 작업 디렉터리로도 위험하다. 접두사만 떼고 나머지는 건드리지 않는다.
///
/// 네트워크 경로는 `\\\\?\\UNC\\` 형태로 오는데, 이때는 `\\\\` 로 되돌린다.
pub fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix("\\\\?\\UNC\\") {
        return PathBuf::from(format!("\\\\{rest}"));
    }
    match s.strip_prefix("\\\\?\\") {
        // 드라이브 경로(C:\…)일 때만 뗀다. 그 밖의 장치 경로는 뜻이 달라진다.
        Some(rest) if rest.len() >= 2 && rest.as_bytes()[1] == b':' => PathBuf::from(rest),
        _ => p,
    }
}

/// 선두의 `~` 를 홈 디렉터리로 바꾼다.
///
/// `~/foo` 는 셸이 풀어주는 표기라 우리 손에는 **문자 그대로** 들어온다.
/// 그대로 `is_dir()` 하면 당연히 거짓이다.
pub fn expand_tilde(s: &str) -> PathBuf {
    let s = s.trim();
    let rest = if s == "~" {
        ""
    } else if let Some(r) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        r
    } else {
        return PathBuf::from(s);
    };
    let Some(home) = home_dir() else { return PathBuf::from(s) };
    if rest.is_empty() {
        return home;
    }
    home.join(rest.replace('/', std::path::MAIN_SEPARATOR_STR))
}

fn home_dir() -> Option<PathBuf> {
    home_var().map(PathBuf::from)
}

/// 홈 디렉터리 환경 변수. **그 OS 의 것을 먼저 본다.**
///
/// 유닉스 셸에도 `USERPROFILE` 이 설정돼 있는 경우가 있다(직접 export, 크로스
/// 컴파일 도구, wine). 무조건 그쪽을 먼저 보면 엉뚱한 홈을 잡는다. 반대로
/// Windows 에도 git-bash 등이 `HOME` 을 심어 둔다. 그래서 우선순위를 뒤집는다.
pub fn home_var() -> Option<std::ffi::OsString> {
    if cfg!(windows) {
        std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
    } else {
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    }
}

/// `prefix` 로 시작하는가 — 대소문자를 가리지 않고, **문자 단위**로 본다.
///
/// 바이트로 자르면 한글 디렉터리에서 문자 경계를 깨고 패닉한다(실측).
fn starts_with_ci(name: &str, prefix: &str) -> bool {
    let mut n = name.chars();
    prefix
        .chars()
        .all(|pc| n.next().is_some_and(|nc| nc.eq_ignore_ascii_case(&pc)))
}

/// 후보들의 공통 접두사. 탭 완성에서 어디까지 채울지 정한다.
///
/// 윈도우 경로는 대소문자를 가리지 않으므로 비교도 그렇게 한다.
fn common_prefix(items: &[String]) -> String {
    let Some(first) = items.first() else { return String::new() };
    let mut n = first.chars().count();
    for it in &items[1..] {
        n = n.min(
            first
                .chars()
                .zip(it.chars())
                .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                .count(),
        );
    }
    first.chars().take(n).collect()
}

pub const HINT: &str = "[+] 칸 추가 · 탭 두 번 눌러 이름 바꾸기 · [x] 닫기 · 드래그 복사 · Ctrl+] 다음 n 모두에게 묻기 · Ctrl+] 다음 f 답 넘기기 · q 종료";

fn contains(r: &Rect, x: u16, y: u16) -> bool {
    r.width > 0 && r.height > 0 && x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

fn center(area: Rect, w: u16, h: u16) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w.min(area.width),
        height: h.min(area.height),
    }
}

/// 입력 끝에 커서를 둔다.
///
/// 문자 수가 아니라 **표시 폭**으로 센다. 한글은 한 글자가 두 칸을 차지하므로
/// chars().count() 로 재면 커서가 글자 수만큼만 가서 어긋난다.
fn put_cursor(f: &mut Frame, inner: Rect, text: &str) {
    let len = UnicodeWidthStr::width(text) as u16;
    let width = inner.width.max(1);
    f.set_cursor_position((
        (inner.x + len % width).min(inner.right().saturating_sub(1)),
        (inner.y + len / width).min(inner.bottom().saturating_sub(1)),
    ));
}

/// 패널 하나에 줄 PTY 크기. 테두리와 상하단 줄을 뺀 값이다.
fn pane_size(area: Rect, n: usize) -> (u16, u16) {
    let n = n.max(1) as u16;
    // 사이드바, 좌우 여백 2칸, 칸 사이 간격(n-1 칸)을 뺀 폭을 나눈다.
    let usable = area.width.saturating_sub(crate::sidebar::WIDTH + 2 + (n - 1));
    let w = (usable / n).saturating_sub(2).max(20);
    // 상단 경로 2줄 + 탭 1줄 + 테두리 2줄 + 칸 제목줄과 구분선 2줄.
    // 맨 아래 상태 줄은 없다.
    let h = area.height.saturating_sub(7).max(6);
    (h, w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 입력창에_남은_질문을_알아본다() {
        let sent = squash("[powershell 의 요청]\n\"\"\"\n오늘 뉴스 10건을 가져와 줘.\n조건:\n- 서로 다른 토픽\n\"\"\"");
        // 제출 안 됨 — 입력창 줄에 질문이 그대로. 화면에서는 줄바꿈이 사라져 붙어 보인다(실측).
        let stuck = "codex\n\n› [powershell 의 요청]\"\"\"오늘 뉴스 10건을 가져와 줘.조건:- 서로\n  다른 토픽\"\"\"\n\n  Ready";
        assert!(unsent_question(stuck, &sent));
        // 제출됨 — 본문에 인용은 남지만 맨 아래 입력창은 비어 있다.
        let working = "› [powershell 의 요청]\n\n• Working (3s • Esc to interrupt)\n\n›\n  ? for shortcuts";
        assert!(!unsent_question(working, &sent));
        // 안내문·빈 화면은 질문이 아니다.
        assert!(!unsent_question("› Ask Codex to do anything", &sent));
        assert!(!unsent_question("입력창이 없다", &sent));
    }

    #[test]
    fn 틸데를_홈으로_바꾼다() {
        let home = home_dir().expect("홈을 알아야 한다");
        assert_eq!(expand_tilde("~"), home);
        assert_eq!(expand_tilde("~/Desktop"), home.join("Desktop"));
        // 틸데가 아닌 것은 손대지 않는다.
        assert_eq!(expand_tilde("C:/tmp"), PathBuf::from("C:/tmp"));
        assert_eq!(expand_tilde("~notme"), PathBuf::from("~notme"));
    }

    #[test]
    fn 한글_경로에서_패닉하지_않는다() {
        // 바이트 슬라이스로 자르던 시절 여기서 깨졌다.
        assert!(starts_with_ci("작업폴더", "작업"));
        assert!(!starts_with_ci("작업폴더", "없는"));
        assert!(!starts_with_ci("작", "작업"));
        assert!(starts_with_ci("Desktop", "desk"));
        assert!(starts_with_ci("anything", ""));
    }

    #[test]
    fn verbatim_접두사를_벗긴다() {
        let v = |s: &str| strip_verbatim(PathBuf::from(s)).to_string_lossy().into_owned();
        assert_eq!(v(r"\\?\C:\Users\HJS"), r"C:\Users\HJS");
        assert_eq!(v(r"\\?\UNC\server\share"), r"\\server\share");
        // 원래 멀쩡한 경로는 그대로 둔다.
        assert_eq!(v(r"C:\Users\HJS"), r"C:\Users\HJS");
        // 드라이브가 아닌 장치 경로는 뜻이 달라지므로 손대지 않는다.
        assert_eq!(v(r"\\?\PhysicalDrive0"), r"\\?\PhysicalDrive0");
    }

    #[test]
    fn 새_공간_경로에_verbatim_이_남지_않는다() {
        let mut app = App::new();
        app.add_space(".").expect("현재 디렉터리는 공간이 된다");
        let p = app.active_path().to_string_lossy().into_owned();
        assert!(!p.starts_with(r"\\?\"), "verbatim 이 남았다: {p}");
    }

    #[test]
    fn 공통_접두사() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(common_prefix(&v(&["cricconf", "cric"])), "cric");
        assert_eq!(common_prefix(&v(&["abc"])), "abc");
        assert_eq!(common_prefix(&v(&["abc", "xyz"])), "");
        // 대소문자를 가리지 않는다.
        assert_eq!(common_prefix(&v(&["Desktop", "desktop2"])), "Desktop");
    }
}

/// 공백을 전부 뺀다. 화면은 줄바꿈·들여쓰기가 제멋대로라 글자만 견준다.
fn squash(text: &str) -> String { text.chars().filter(|c| !c.is_whitespace()).collect() }

/// 넣은 질문이 아직 입력창에 남아 있는가.
///
/// 화면에서 **마지막** 입력창 줄(`›` codex · `❯`/`>` claude·agy)을 찾아, 그 뒤 글자가 넣은 글의
/// 일부면 제출되지 않은 것이다. 제출된 뒤의 입력창은 비어 있거나 안내문이라 맞지 않는다.
/// 마지막 줄만 보는 이유는 대화 본문에 인용된 질문이 그 위에 같은 글리프로 보일 수 있어서다.
fn unsent_question(screen: &str, sent: &str) -> bool {
    let Some(line) = screen.lines().rev().map(str::trim).find(|l| l.starts_with(&['›', '❯', '>'][..])) else { return false };
    let glyph = line.chars().next().map_or(0, char::len_utf8);
    let rest: String = squash(&line[glyph..]).chars().take(16).collect();
    rest.chars().count() >= 6 && sent.contains(&rest)
}

impl App {
    pub fn by_id(&self, id: u64) -> Option<usize> { self.sessions.iter().position(|s| s.id == id) }

    /// 이 칸이 검증자로서 답하는 중이면 그 요청이 시작된 시각. 사이드바가 쓴다.
    pub fn verifying_since(&self, id: u64) -> Option<Instant> {
        let ask = self.asks.iter().find(|a| a.verifier == id)?;
        self.groups.iter().find(|g| g.id == ask.group).map(|g| g.started)
    }

    /// 이 칸이 낸 검증 요청이 아직 진행 중인가. 사이드바가 쓴다.
    pub fn requesting(&self, id: u64) -> bool {
        self.groups.iter().any(|g| g.requester == id)
    }
    fn control_env(&self, id: u64, role: &str) -> Vec<(&'static str, std::ffi::OsString)> {
        let mut env = vec![("MAI_PANE", id.to_string().into()), ("MAI_ROLE", role.into())];
        crate::ctl::server::note_pane(id);
        if let Some(server) = &self.ctl {
            env.push(("MAI_CTL", server.endpoint.env_value()));
            env.push(("MAI_CTL_DIR", server.dir.as_os_str().to_owned()));
            if let Some(t) = server.endpoint.token() { env.push(("MAI_CTL_TOKEN", t.into())); }
        }
        env
    }

    fn receive_asks(&mut self) {
        loop {
            let Some((req, stream)) = self.ctl.as_ref().and_then(|s| s.rx.try_recv().ok()) else { break };
            let result = match req.op.as_str() {
                "ask" => self.begin_ask(req, stream),
                "status" => Err((stream, crate::ctl::Response::answer(serde_json::to_string(&self.status()).unwrap_or_default()))),
                "screen" => Err((stream, self.screen_of(req.target_pane))),
                _ => Err((stream, crate::ctl::Response::error("잘못된 요청"))),
            };
            if let Err((stream, response)) = result { crate::ctl::server::reply(stream, &response); }
        }
    }

    /// `mai ctl status` — 칸 목록과 진행 중 요청.
    fn status(&self) -> crate::ctl::Status {
        let panes = self.sessions.iter().map(|s| crate::ctl::PaneStatus {
            id: s.id, title: s.title.clone(), agent: s.agent_id.clone(),
            role: if s.verifier_of.is_some() { "verifier" } else if s.is_terminal { "terminal" } else { "main" }.into(),
            verifier_of: s.verifier_of, model: s.model.clone(),
            cwd: s.cwd.clone().unwrap_or_else(|| self.spaces[s.space].path.clone()).to_string_lossy().into_owned(),
            alive: s.pty.as_ref().is_some_and(|p| !p.finished()), pending: s.pending.is_some(),
        }).collect();
        let asks = self.asks.iter().map(|a| {
            let group = self.groups.iter().find(|g| g.id == a.group);
            crate::ctl::AskStatus {
                requester: group.map_or(0, |g| g.requester), verifier: a.verifier, agent: a.target.agent.clone(), model: a.target.model.clone(),
                elapsed_secs: group.map_or(0, |g| g.started.elapsed().as_secs()),
                remaining_secs: a.deadline.map(|d| d.saturating_duration_since(Instant::now()).as_secs()),
                injected: a.injected_at.is_some(),
            }
        }).collect();
        crate::ctl::Status { panes, asks }
    }

    /// `mai ctl screen <id>` — 그 칸의 화면 글자. 디버깅용.
    fn screen_of(&self, id: Option<u64>) -> crate::ctl::Response {
        let Some(i) = id.and_then(|id| self.by_id(id)) else { return crate::ctl::Response::error("그런 칸이 없다") };
        match self.sessions[i].pty.as_ref() {
            Some(p) => crate::ctl::Response::answer(crate::relay::from_screen(&p.screen()).text),
            None => crate::ctl::Response::error("칸에 프로세스가 없다"),
        }
    }

    fn begin_ask(&mut self, req: crate::ctl::Request, stream: Box<dyn crate::ctl::transport::Conn>) -> Result<(), (Box<dyn crate::ctl::transport::Conn>, crate::ctl::Response)> {
        let fail = |stream, msg: &str| Err((stream, crate::ctl::Response::error(msg)));
        let Some(i) = self.by_id(req.pane) else { return fail(stream, "요청자 칸이 없다") };
        if req.role == "verifier" || self.sessions[i].verifier_of.is_some() { return fail(stream, "요청받은 칸에서는 다시 요청할 수 없다"); }
        if req.question.trim().is_empty() { return fail(stream, "잘못된 요청"); }
        let mut targets = req.targets();
        for t in &mut targets {
            let Some(agent) = crate::ctl::normalize(&t.agent) else { return fail(stream, "알 수 없는 에이전트") };
            t.agent = agent.into();
            // `-m` 이 없으면 설정의 에이전트별 기본 모델. 계열 이름은 최신 버전으로 푼다.
            if t.model.is_none() { t.model = self.config.ctl_model.get(agent).cloned(); }
            t.model = t.model.as_deref().map(|m| crate::ctl::models::resolve(agent, m));
        }
        let timeout = req.timeout_secs.or_else(|| (self.config.ctl_timeout > 0).then_some(self.config.ctl_timeout));
        let deadline = match timeout { Some(t) => match Instant::now().checked_add(Duration::from_secs(t)) { Some(d) => Some(d), None => return fail(stream, "시간 제한이 너무 크다") }, None => None };
        // 같은 요청자의 진행 중 요청: 대상이 같으면 합류, 다르면 거절.
        if let Some(g) = self.groups.iter_mut().find(|g| g.requester == req.pane) {
            if g.targets != targets { return fail(stream, "이미 진행 중") }
            g.keep |= req.keep; g.waiters.push(stream); return Ok(());
        }
        let cwd = self.sessions[i].cwd.clone().unwrap_or_else(|| self.spaces[self.sessions[i].space].path.clone());
        let title = self.sessions[i].title.clone(); let space = self.sessions[i].space;
        let group_id = self.next_id; self.next_id += 1;
        // Windows 에서 표준 입력으로 온 질문은 CRLF 다. 붙여넣기 안의 CR 은 에이전트마다 다르게 읽으므로 LF 로 통일한다.
        let question = req.question.replace("\r\n", "\n").replace('\r', "\n");
        let body = format!("[{title} 의 요청]\n\"\"\"\n{question}\n\"\"\"");
        let mut asks = Vec::new();
        for target in &targets {
            let v = match self.verifier_pane(req.pane, target, &cwd, &title, space) { Ok(v) => v, Err(e) => { for a in asks { self.asks.push(a); } return Err((stream, crate::ctl::Response::error(e))) } };
            let baseline = crate::relay::last_answer_since(&target.agent, &cwd, self.sessions[v].spawned_at).map(|q| q.text);
            self.sessions[v].pending = Some(format!("\x1b[200~{body}\x1b[201~"));
            self.sessions[v].user_typed = false;
            asks.push(Ask { group: group_id, verifier: self.sessions[v].id, target: target.clone(), baseline, injected_at: None, deadline, sent: squash(&body), enter_retries: 0 });
        }
        self.asks.extend(asks);
        self.groups.push(AskGroup { id: group_id, requester: req.pane, targets, question, requester_title: title.clone(), cwd: cwd.clone(), keep: req.keep, started: Instant::now(), results: Vec::new(), waiters: vec![stream] });
        Ok(())
    }

    /// 대상에 맞는 검증 칸을 찾거나 새로 띄운다. 반환은 sessions 인덱스.
    fn verifier_pane(&mut self, requester: u64, target: &crate::ctl::Target, cwd: &PathBuf, title: &str, space: usize) -> Result<usize, String> {
        let agent = target.agent.as_str();
        let existing = self.sessions.iter().position(|s| s.verifier_of == Some(requester) && s.agent_id == agent && s.cwd.as_ref() == Some(cwd) && s.model == target.model && s.pending.is_none() && s.pty.as_ref().is_some_and(|p| !p.finished()));
        if let Some(v) = existing { return Ok(v); }
        let id = self.next_id; self.next_id += 1;
        let env = self.control_env(id, "verifier");
        let (h, w) = pane_size(self.last_area, (self.visible().len()+1).min(MAX_SPLIT));
        let spawned_at = std::time::SystemTime::now();
        // 세 CLI 모두 `--model <이름>` 을 받는다(claude · codex · agy).
        let extra: Vec<String> = target.model.iter().flat_map(|m| ["--model".to_string(), m.clone()]).collect();
        let pty = PtySession::spawn_controlled(Some(agent), h, w, cwd, &env, &extra).map_err(|e| e.to_string())?;
        let display = crate::ctl::display(agent);
        let label = match &target.model { Some(m) => format!("{display}({m}) ← {title}"), None => format!("{display} ← {title}") };
        let mut session = Session::new(agent, &label, space);
        session.id = id; session.verifier_of = Some(requester); session.model = target.model.clone(); session.spawned_at = Some(spawned_at);
        session.cwd = Some(cwd.clone()); session.pty = Some(pty); session.size = (h, w); session.started = Some(Instant::now());
        self.sessions.push(session);
        Ok(self.sessions.len() - 1)
    }

    fn finish_asks(&mut self) {
        if self.asks_checked.elapsed() < Duration::from_millis(500) { return; }
        self.asks_checked = Instant::now();
        let asks = std::mem::take(&mut self.asks);
        for mut ask in asks {
            let requester_gone = self.groups.iter().find(|g| g.id == ask.group).map_or(true, |g| self.by_id(g.requester).is_none());
            let result = if ask.deadline.is_some_and(|d| Instant::now() >= d) { Some(crate::ctl::Response::error("요청 시간 초과")) }
            else if requester_gone { Some(crate::ctl::Response::error("요청자 칸이 닫혔다")) }
            else if let Some(v) = self.by_id(ask.verifier) {
                let s = &mut self.sessions[v];
                if s.pty.as_ref().map_or(true, |p| p.finished()) { Some(crate::ctl::Response::error("요청받은 칸이 종료됐다")) }
                else if s.pending.is_some() { self.status = "요청받은 칸의 준비 또는 사용자 확인을 기다린다".into(); None }
                else {
                    let injected = ask.injected_at.get_or_insert_with(Instant::now);
                    let screen = s.pty.as_ref().unwrap().screen();
                    if injected.elapsed() < Duration::from_millis(2500) || s.seen.1.elapsed() < Duration::from_millis(2500) || crate::modal::open(&screen) { None }
                    else {
                        let quote = if ask.target.agent == "agy" { let mut q = crate::relay::from_screen(&screen); q.text = crate::relay::agy_answer(&q.text); Some(q) } else { crate::relay::last_answer_since(&ask.target.agent, s.cwd.as_ref().unwrap(), s.spawned_at) };
                        let answer = quote.filter(|q| !q.text.trim().is_empty() && Some(&q.text) != ask.baseline.as_ref()).map(|q| crate::ctl::Response { ok: true, answer: Some(q.text), origin: Some(q.origin.label().into()), error: None });
                        if answer.is_some() { answer }
                        // 답이 없는데 질문이 입력창에 그대로면 제출이 안 된 것이다(여러 줄 질문에서 실측 —
                        // CTL_FIX_ENTER.md). Enter 를 다시 보내 보고, 그래도 안 되면 오류로 끝내
                        // 요청자가 영원히 기다리지 않게 한다. 사람이 그 칸에 입력 중이면 손대지 않는다.
                        else if s.user_typed || !unsent_question(&crate::relay::from_screen(&screen).text, &ask.sent) { None }
                        else if ask.enter_retries < 2 {
                            ask.enter_retries += 1; ask.injected_at = Some(Instant::now());
                            if let Some(p) = s.pty.as_mut() { let _ = p.write(&[13]); }
                            self.status = format!("요청받은 칸에 Enter 를 다시 보냈다 ({}/2)", ask.enter_retries); None
                        }
                        else { Some(crate::ctl::Response::error("질문이 전송되지 않았다 — 요청받은 칸에서 Enter 가 필요하다")) }
                    }
                }
            } else { Some(crate::ctl::Response::error("요청받은 칸이 닫혔다")) };
            let Some(response) = result else { self.asks.push(ask); continue };
            let keep = self.groups.iter().find(|g| g.id == ask.group).is_some_and(|g| g.keep);
            if let Some(v) = self.by_id(ask.verifier) {
                if !response.ok { self.sessions[v].pending = None; }
                // 1회성 질문이면 답을 돌려준 뒤 검증 칸을 닫는다. 오류로 끝난 칸은
                // 무슨 일이 있었는지 볼 수 있게 남긴다.
                else if !keep { self.close_session(v); self.status = "요청받은 칸을 닫았다".into(); }
            }
            if let Some(g) = self.groups.iter_mut().find(|g| g.id == ask.group) { g.results.push((ask.target, response)); }
        }
        // 대상 전부의 답이 모이면 합쳐서 돌려준다.
        let done: Vec<AskGroup> = { let (done, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.groups).into_iter().partition(|g| g.results.len() >= g.targets.len()); self.groups = rest; done };
        for mut g in done {
            // 요청 순서대로 정렬한다. 답은 끝난 순서로 쌓였다.
            g.results.sort_by_key(|(t, _)| g.targets.iter().position(|x| x == t).unwrap_or(usize::MAX));
            let response = crate::ctl::combine(&g.results);
            for stream in g.waiters { crate::ctl::server::reply(stream, &response); }
            if self.config.ctl_history {
                let (id, time) = crate::ctl::history::now_id();
                let results = g.results.iter().map(|(t, r)| crate::ctl::history::Outcome { agent: t.agent.clone(), model: t.model.clone(), ok: r.ok, answer: r.answer.clone(), error: r.error.clone(), origin: r.origin.clone() }).collect();
                crate::ctl::history::append(&crate::ctl::history::Record { id, time, cwd: g.cwd.to_string_lossy().into_owned(), requester: g.requester_title, question: g.question, elapsed_secs: g.started.elapsed().as_secs(), keep: g.keep, results });
            }
        }
    }
}
