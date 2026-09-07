//! A selection owns a snapshot so output arriving during a drag cannot change copied text.
use ratatui::{buffer::Buffer, layout::Rect, style::{Color, Modifier}};

pub struct Selection {
    pub pane: usize,
    pub screen: vt100::Screen,
    pub anchor: (u16, u16),
    pub end: (u16, u16),
    pub dragging: bool,
}
impl Selection {
    fn bounds(&self) -> ((u16, u16), (u16, u16)) {
        (self.anchor.min(self.end), self.anchor.max(self.end))
    }
    fn selected(&self, row: u16, col: u16) -> bool {
        let (start, end) = self.bounds();
        (row, col) >= start && (row, col) <= end
    }
    pub fn text(&self) -> String {
        let (start, end) = self.bounds();
        let (_, cols) = self.screen.size();
        let mut text = String::new();
        for row in start.0..=end.0 {
            let mut line = String::new();
            for col in 0..cols {
                if let Some(cell) = self.screen.cell(row, col) {
                    if cell.is_wide_continuation() { continue; }
                    if self.selected(row, col) || (cell.is_wide() && self.selected(row, col + 1)) {
                        let content = cell.contents();
                        line.push_str(if content.is_empty() { " " } else { &content });
                    }
                }
            }
            if row < end.0 && self.screen.row_wrapped(row) {
                text.push_str(&line);
            } else {
                text.push_str(line.trim_end_matches(' '));
            }
            if row < end.0 && !self.screen.row_wrapped(row) { text.push('\n'); }
        }
        text
    }
    pub fn highlight(&self, area: Rect, buf: &mut Buffer) {
        for row in 0..area.height {
            for col in 0..area.width {
                let wide = self.screen.cell(row, col).is_some_and(|c| c.is_wide());
                let continuation = self.screen.cell(row, col).is_some_and(|c| c.is_wide_continuation());
                if self.selected(row, col) || (wide && self.selected(row, col + 1)) ||
                    (continuation && self.selected(row, col.saturating_sub(1))) {
                    if let Some(cell) = buf.cell_mut((area.x + col, area.y + row)) {
                        cell.set_fg(Color::Black).set_bg(Color::LightCyan);
                        cell.modifier.remove(Modifier::REVERSED);
                    }
                }
            }
        }
    }
}

pub fn copy(text: &str) -> std::io::Result<()> {
    use std::{io::Write, process::{Command, Stdio}};
    #[cfg(target_os = "macos")]
    let candidates: &[(&str, &[&str])] = &[("pbcopy", &[])];
    #[cfg(target_os = "windows")]
    let candidates: &[(&str, &[&str])] = &[("clip.exe", &[])];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let candidates: &[(&str, &[&str])] = &[("wl-copy", &[]), ("xclip", &["-selection", "clipboard"]), ("xsel", &["--clipboard", "--input"])];
    for (program, args) in candidates {
        if let Ok(mut child) = Command::new(program).args(*args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            let result = child.stdin.take().unwrap().write_all(text.as_bytes());
            let status = child.wait()?;
            if result.is_ok() && status.success() { return Ok(()); }
        }
    }
    Err(std::io::Error::other("클립보드 도구를 실행할 수 없습니다"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapped_spaces_and_wide_highlight_are_preserved() {
        let mut p = vt100::Parser::new(2, 4, 0);
        p.process("한  X".as_bytes());
        let s = Selection { pane: 0, screen: p.screen().clone(), anchor: (0, 1), end: (1, 0), dragging: false };
        assert_eq!(s.text(), "한  X");
        let area = Rect::new(5, 2, 4, 2);
        let mut buf = Buffer::empty(area);
        s.highlight(area, &mut buf);
        assert_eq!(buf.cell((5, 2)).unwrap().bg, Color::LightCyan);
        assert_eq!(buf.cell((6, 2)).unwrap().bg, Color::LightCyan);
    }

    #[test]
    fn reverse_wide_and_wrapped_selection() {
        let mut p = vt100::Parser::new(3, 6, 10);
        p.process("ab한글XY\r\n끝".as_bytes());
        let s = Selection { pane: 0, screen: p.screen().clone(), anchor: (1, 1), end: (0, 3), dragging: false };
        assert_eq!(s.text(), "한글XY");
        let s = Selection { anchor: (0, 0), end: (2, 1), ..s };
        assert_eq!(s.text(), "ab한글XY\n끝");
    }
}
