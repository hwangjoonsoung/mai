//! 작업 공간과 세션. 구조 변경의 뼈대다.
//!
//! 이전에는 「고정 참여자 3인 × 워크스페이스 1개」였다. 이제는
//!
//!   Space (경로)
//!     ├─ Session (에이전트 + PTY)
//!     ├─ Session
//!     └─ …
//!
//! 로 바뀐다. 공간을 고르면 그 공간의 세션만 보이고, 새 세션은 그 공간의
//! 경로에서 뜬다.

use crate::pty::PtySession;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub struct Space {
    pub path: PathBuf,
    pub name: String,
}

impl Space {
    pub fn new(path: PathBuf) -> Self {
        let name = Self::name_of(&path);
        Self { path, name }
    }

    /// 경로를 바꾼다. 이름도 같이 따라간다.
    ///
    /// 터미널 칸에서 `cd` 하면 공간이 그쪽으로 옮겨 간다. 사이드바가 실제와
    /// 다른 경로를 가리키고 있으면 새 칸이 엉뚱한 데서 뜬다.
    pub fn set_path(&mut self, path: PathBuf) {
        self.name = Self::name_of(&path);
        self.path = path;
    }

    fn name_of(path: &Path) -> String {
        path.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
    }
}

/// 세션 상태. **관측 가능한 사실만** 표현한다.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    Starting,
    Working,
    Quiet,
    Exited,
}

impl State {
    pub fn color(self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            State::Starting => Color::Yellow,
            State::Working => Color::Green,
            State::Quiet => Color::Cyan,
            State::Exited => Color::Red,
        }
    }
}

pub struct Session {
    pub agent_id: String,
    pub title: String,
    /// 어느 공간에 속하는가 (spaces 인덱스)
    pub space: usize,
    pub pty: Option<PtySession>,
    /// 마지막으로 맞춘 크기. 달라졌을 때만 resize 한다.
    pub size: (u16, u16),
    /// 아직 넣지 못한 프롬프트
    pub pending: Option<String>,
    pub started: Option<Instant>,
    /// 마지막으로 본 수신 바이트 수와 그 시각
    pub seen: (usize, Instant),
    /// 화면에서 읽어낸 서브에이전트 명부. 화면이 바뀔 때만 갱신한다.
    pub subs: Vec<crate::subagents::Sub>,
    /// 사용자가 이 칸에 직접 글자를 쳤는가.
    ///
    /// 쳤다면 대기 중인 질문은 넣지 않는다. 사용자가 이미 자기 뜻대로 쓰고
    /// 있는데 뒤늦게 끼어드는 것은 방해다. 화살표·Enter 는 세지 않는다 —
    /// 대화상자에 답하는 조작이지 「직접 쓰기」가 아니기 때문이다.
    pub user_typed: bool,
    /// 자식이 실제로 들어가 있는 경로. 셸이 `cd` 하면 여기가 바뀐다.
    ///
    /// 기동할 때의 경로와 다를 수 있어 따로 들고 본다. 매 프레임 조회하면
    /// 비싸므로 마지막으로 확인한 시각을 함께 둔다.
    pub cwd: Option<PathBuf>,
    pub cwd_at: Option<Instant>,
    /// 에이전트가 아니라 사용자의 셸인가.
    ///
    /// 터미널 칸에는 서브에이전트 명부가 있을 리 없고, 화면을 훑어 봐야
    /// 셸 출력에서 헛것을 읽을 뿐이다. 그래서 스캔을 건너뛴다.
    pub is_terminal: bool,
}

impl Session {
    pub fn new(agent_id: &str, title: &str, space: usize) -> Self {
        Self {
            agent_id: agent_id.into(),
            title: title.into(),
            space,
            pty: None,
            size: (0, 0),
            pending: None,
            started: None,
            seen: (0, Instant::now()),
            subs: Vec::new(),
            user_typed: false,
            cwd: None,
            cwd_at: None,
            is_terminal: false,
        }
    }

    /// 터미널 세션. 에이전트가 아니라 사용자의 셸이 도는 칸이다.
    pub fn terminal(title: &str, space: usize) -> Self {
        let mut s = Self::new("", title, space);
        s.is_terminal = true;
        s
    }

    /// 화면에서 서브에이전트 명부를 다시 읽는다.
    ///
    /// 매 프레임 전체 화면을 훑는 것은 낭비라 **새 출력이 있을 때만** 한다.
    pub fn refresh_subs(&mut self, changed: bool) {
        if !changed || self.is_terminal {
            return;
        }
        self.subs = match self.pty.as_ref() {
            Some(p) => crate::subagents::scan(&p.screen()),
            None => Vec::new(),
        };
    }

    pub fn state(&self) -> State {
        match &self.pty {
            None => State::Exited,
            Some(s) if s.finished() => State::Exited,
            Some(_) if self.pending.is_some() => State::Starting,
            Some(_) => {
                if self.seen.1.elapsed() < Duration::from_millis(800) {
                    State::Working
                } else {
                    State::Quiet
                }
            }
        }
    }

    /// 프롬프트를 넣어도 되는 상태인가.
    ///
    /// 에이전트마다 기동 속도가 크게 다르다. 고정 지연으로는 빠른 쪽엔 낭비,
    /// 느린 쪽엔 입력이 삼켜진다. **출력이 한 번 나온 뒤 잠잠해지면** 준비로 본다.
    pub fn ready_to_inject(&mut self) -> bool {
        let Some(s) = self.pty.as_ref() else { return false };
        let Some(started) = self.started else { return false };
        let rx = s.rx_bytes;
        if rx != self.seen.0 {
            self.seen = (rx, Instant::now());
            return false;
        }
        if rx == 0 {
            return started.elapsed() >= Duration::from_secs(20);
        }
        self.seen.1.elapsed() >= Duration::from_millis(600)
            || started.elapsed() >= Duration::from_secs(20)
    }
}
