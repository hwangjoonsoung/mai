//! 앱 상태와 화면. 「공간 × 세션」 구조다.
//!
//! 화면 모드
//!   Panes    — 기본 화면. 선택된 공간의 세션들. 그 안은 에이전트의 실제 TUI 다.
//!   Picker   — `+` 를 눌러 어떤 에이전트를 띄울지 고르는 중
//!   NewSpace — 새 공간의 경로를 입력하는 중
//!   Broadcast— 보이는 모든 에이전트에게 한 번에 묻는 중
//!
//! **시작하자마자 Panes 다.** 예전엔 질문을 먼저 받는 Idle 화면을 거쳤는데,
//! 그 화면은 에이전트를 띄우기 위한 관문일 뿐이라 한 번 쓰고 다시 볼 일이
//! 없었다. 지금은 기동과 동시에 쓸 수 있는 참여자를 전부 띄우고, 각 칸에는
//! 에이전트 자신의 첫 화면이 그대로 보인다. 질문은 Ctrl+] 다음 n 으로 언제든 넣는다.
//!
//! 키는 기본적으로 **포커스된 세션의 자식에게 그대로** 간다. 그래야 권한 승인·
//! 슬래시 자동완성·esc 중단이 동작한다. 우리 조작은 tmux 처럼 프리픽스로 뺀다.
//!
//! 세션이 넷 이상이면 나란히 두지 않고 **탭**으로 바꾼다. 좁은 칸에 에이전트
//! TUI 를 밀어 넣으면 자식이 자기 화면을 접어버려 읽을 수 없기 때문이다.

use crate::{
    model::{Session, Space},
    pty::PtySession,
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
    Picker,
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
    /// 새 세션 추가 (+)
    AddSession,
    /// 전체 보기 탭
    ShowAll,
    /// 탭 줄의 칩. 한 번 누르면 포커스, 두 번 누르면 이름 고치기.
    Tab(usize),
}

pub struct App {
    /// 띄울 수 있는 에이전트 (id, 표시명)
    pub agents: Vec<(String, String)>,

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
    pub fn new(agents: &[(&str, &str)]) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            agents: agents.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
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
        self.spawn_terminal(area);
        self.sync_sizes(area);
        self.focus = self.visible().first().copied().unwrap_or(0);
        self.status = HINT.into();
    }

    /// `[+]` 상자에 늘어놓을 것들. 0번이 터미널, 그다음이 에이전트다.
    pub fn picker_items(&self) -> Vec<String> {
        let mut v = vec![format!("터미널 ({})", crate::pty::shell_name())];
        v.extend(self.agents.iter().map(|(_, t)| t.clone()));
        v
    }

    /// `[+]` 상자에서 n 번째를 고른다.
    pub fn pick(&mut self, n: usize, area: Rect) {
        if n == 0 {
            self.spawn_terminal(area);
            return;
        }
        if let Some((id, _)) = self.agents.get(n - 1).cloned() {
            self.mode = Mode::Panes;
            self.spawn_session(&id, area, None);
        }
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

    /// 지금 공간에서 에이전트 하나를 띄운다.
    pub fn spawn_session(&mut self, agent_id: &str, area: Rect, prompt: Option<&str>) {
        let title = self
            .agents
            .iter()
            .find(|(id, _)| id == agent_id)
            .map(|(_, t)| t.clone())
            .unwrap_or_else(|| agent_id.to_string());

        let n = (self.visible().len() + 1).min(MAX_SPLIT);
        let (h, w) = pane_size(area, n);
        let cwd = self.active_path();

        let mut s = Session::new(agent_id, &title, self.active_space);
        match PtySession::spawn_in(agent_id, h, w, &cwd) {
            Ok(pty) => {
                s.pty = Some(pty);
                s.size = (h, w);
                s.pending = prompt.map(String::from);
                s.started = Some(Instant::now());
                s.seen = (0, Instant::now());
                self.sessions.push(s);
                self.focus = self.sessions.len() - 1;
                self.mode = Mode::Panes;
            }
            Err(e) => self.status = format!("{title} 기동 실패 — {e}"),
        }
    }

    /// 지금 공간에 **터미널** 칸을 하나 붙인다.
    ///
    /// `[+]` 가 하는 일이다. 예전엔 여기서 에이전트를 골랐지만, 에이전트는 이미
    /// 기동 때 다 떠 있다. 그 자리에서 실제로 아쉬운 건 «같은 경로에서 명령을
    /// 쳐 볼 셸» 이라 그걸 기본으로 삼는다. 에이전트를 되살리려면 Ctrl+] p.
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
        match PtySession::spawn_shell_in(h, w, &cwd) {
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

        // 진단(--probe)과 **같은 경로**를 쓴다. 갈라 두면 진단은 통과하는데
        // 앱은 안 되는 상황이 생긴다.
        let sys = self.sys.get_or_insert_with(sysinfo::System::new);
        let found = crate::pty::process_cwd(sys, pid);

        let i = self.focus;
        if let Some(s) = self.sessions.get_mut(i) {
            s.cwd_at = Some(Instant::now());
            s.cwd = found.clone();
        }
        let Some(cwd) = found else { return };
        if let Some(sp) = self.spaces.get_mut(self.active_space) {
            if sp.path != cwd {
                sp.set_path(cwd);
            }
        }
    }

    /// 살아 있는 세션의 출력을 먹이고, 준비된 세션에 프롬프트를 넣는다.
    pub fn tick(&mut self) {
        self.follow_cwd();
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
            Mode::Picker => {
                self.draw_panes(f);
                self.draw_picker(f);
            }
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
            .constraints([Constraint::Length(2), Constraint::Min(3), Constraint::Length(1)])
            .split(area);

        // 최상단은 첫 질문이 아니라 **현재 공간의 경로**다.
        // 질문은 각 에이전트 화면 안에 이미 남아 있고, 여기서 늘 필요한 정보는
        // 「지금 어디서 돌고 있는가」다.
        f.render_widget(
            Paragraph::new(self.active_path().to_string_lossy().into_owned())
                .style(Style::default().add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::BOTTOM)),
            vert[0],
        );

        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(crate::sidebar::WIDTH), Constraint::Min(20)])
            .split(vert[1]);
        crate::sidebar::draw(f, main[0], self);

        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(3)])
            .split(main[1]);
        self.draw_tabbar(f, right[0]);
        self.draw_bodies(f, right[1]);

        let hint = if self.prefix {
            "Ctrl+] 눌림 — f 답 넘기기 · t 터미널 · p 에이전트 · n 모두에게 묻기 · a Ctrl+A 를 자식에게 · q 종료 · 1..9 포커스"
                .to_string()
        } else {
            self.status.clone()
        };
        f.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
            vert[2],
        );
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
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
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
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
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
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ));
            self.add_session_hit = Some(Rect { x, y: area.y, width: 3, height: 1 });
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn draw_bodies(&mut self, f: &mut Frame, area: Rect) {
        let vis = self.visible();
        if vis.is_empty() {
            f.render_widget(
                Paragraph::new("이 공간에 세션이 없다. [+] 터미널 · Ctrl+] 다음 p 에이전트.")
                    .style(Style::default().fg(Color::DarkGray)),
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
            .split(area);

        for (slot, &i) in shown.iter().enumerate() {
            let s = &self.sessions[i];
            let a = cols[slot];
            let focused = i == self.focus;
            let border = if focused { Color::Cyan } else { Color::DarkGray };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(if focused { BorderType::Thick } else { BorderType::Plain })
                .border_style(Style::default().fg(border))
                .title(format!(" {} ", s.title));
            let inner = block.inner(a);
            f.render_widget(block, a);

            if a.width >= 6 {
                let btn = Rect { x: a.right().saturating_sub(4), y: a.y, width: 3, height: 1 };
                f.render_widget(
                    Paragraph::new("[x]")
                        .style(Style::default().fg(if focused { Color::Cyan } else { Color::DarkGray })),
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

    /// `+` 를 눌렀을 때의 에이전트 선택 상자.
    fn draw_picker(&self, f: &mut Frame) {
        let area = f.area();
        let h = self.picker_items().len() as u16 + 2;
        let r = center(area, 40, h);
        f.render_widget(Clear, r);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Green))
            .title(" 무엇을 띄울까 (숫자 선택 · Esc 취소) ");
        let inner = block.inner(r);
        f.render_widget(block, r);
        let lines: Vec<Line> = self
            .picker_items()
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                Line::from(vec![
                    Span::styled(format!(" {} ", i + 1), Style::default().fg(Color::Green)),
                    Span::raw(t),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(lines), inner);
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
            .border_style(Style::default().fg(Color::Green))
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
            Style::default().fg(Color::DarkGray),
        )];
        for (i, (_, name, on)) in self.relay_targets.iter().enumerate() {
            let focused = self.relay_focus == i + 1;
            let mut st = Style::default().fg(if *on { Color::Green } else { Color::DarkGray });
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
            Style::default().fg(if on_input { Color::Cyan } else { Color::DarkGray }),
        )];
        spans.push(Span::raw(self.input.as_str()));
        f.render_widget(Paragraph::new(Line::from(spans)), rows[1]);

        f.render_widget(
            Paragraph::new("Tab 이동 · Space 켜기/끄기 · Enter 보내기 · Esc 취소")
                .style(Style::default().fg(Color::DarkGray)),
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
            .border_style(Style::default().fg(Color::Green))
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
    let usable = area.width.saturating_sub(crate::sidebar::WIDTH);
    let w = (usable / n).saturating_sub(2).max(20);
    // 상단 경로 2줄 + 탭 1줄 + 상태 1줄 + 테두리 2줄
    let h = area.height.saturating_sub(6).max(6);
    (h, w)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut app = App::new(&[("claude", "Claude")]);
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
