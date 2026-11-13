//! mai 가 직접 그리는 부분의 색.
//!
//! 상단 바·사이드바·탭 줄·칸 테두리와 제목줄·상태 줄·상자에만 쓴다. 칸 본문은
//! 에이전트 자신의 화면이라 여기 색을 입히지 않는다.
//!
//! 바탕색과 기본 글자색은 예외다. 화면 전체를 한 바탕으로 맞추려면 칸 본문의
//! «기본색» 칸도 같은 색이어야 한다. 그래서 에이전트가 색을 지정하지 않은 칸은
//! [`bg`]·[`fg`] 로 그린다(`vtscreen`). 에이전트가 직접 고른 색은 건드리지 않는다.
//!
//! 24비트 색을 못 그리는 터미널(macOS 기본 Terminal.app 등)에 RGB 를 보내면
//! 엉뚱한 색이 나온다. 그래서 지원이 확인될 때만 RGB 를 쓰고, 아니면 가장 가까운
//! 기본 색으로 떨어진다.

use ratatui::style::Color;
use std::sync::OnceLock;

/// 바깥 터미널이 24비트 색을 그리는가.
///
/// `COLORTERM` 은 iTerm2·Ghostty·kitty·WezTerm 등이 세운다. Windows 는 콘솔
/// 자체(Windows 10 이후의 conhost·ConPTY)가 24비트 색을 그리므로 변수를 보지
/// 않는다 — Windows Terminal 밖(IDE 내장 터미널 등)에서는 표시해 주는 변수가 없다.
fn truecolor() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| {
        let ct = std::env::var("COLORTERM").unwrap_or_default().to_lowercase();
        cfg!(windows) || ct.contains("truecolor") || ct.contains("24bit")
    })
}

fn pick(rgb: (u8, u8, u8), fallback: Color) -> Color {
    if truecolor() {
        Color::Rgb(rgb.0, rgb.1, rgb.2)
    } else {
        fallback
    }
}

/// 화면 바탕. 24비트 색이 안 되면 바깥 터미널의 바탕을 그대로 둔다.
pub fn bg() -> Color {
    pick((0x11, 0x13, 0x18), Color::Reset)
}

/// 기본 글자색. 24비트 색이 안 되면 바깥 터미널의 글자색을 그대로 둔다.
pub fn fg() -> Color {
    pick((0xd5, 0xd9, 0xe0), Color::Reset)
}

/// 테두리와 구분선.
pub fn line() -> Color {
    pick((0x2c, 0x31, 0x3a), Color::DarkGray)
}

/// 강조할 글자 (선택된 항목 등).
pub fn text() -> Color {
    pick((0xd5, 0xd9, 0xe0), Color::White)
}

/// 흐린 글자 — 안내·선택 안 된 항목.
pub fn dim() -> Color {
    pick((0x8b, 0x93, 0xa1), Color::DarkGray)
}

/// 포커스 — 지금 키가 가는 칸.
pub fn focus() -> Color {
    pick((0x5f, 0xd0, 0xd6), Color::Cyan)
}

/// 켜짐·추가 같은 긍정 표시.
pub fn ok() -> Color {
    pick((0x7f, 0xd1, 0x8b), Color::Green)
}

/// 진행 중.
pub fn warn() -> Color {
    pick((0xe6, 0xc3, 0x5c), Color::Yellow)
}

/// 에이전트마다 고정된 색. 누가 말했는지를 색 점과 이름으로 보인다.
///
/// 셸 칸(에이전트 id 가 비어 있다)은 흐린 색이다.
pub fn agent(id: &str) -> Color {
    match id {
        "claude" => pick((0xe8, 0xa0, 0x6c), Color::LightRed),
        "codex" => pick((0x74, 0xb4, 0xf5), Color::LightBlue),
        "agy" => pick((0xb7, 0x9c, 0xf0), Color::LightMagenta),
        _ => dim(),
    }
}
