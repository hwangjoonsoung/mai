//! vt100 화면 버퍼를 ratatui 버퍼로 옮긴다.
//!
//! 이게 되면 에이전트가 그린 화면이 그대로 보인다. 색·굵기·역상까지 옮겨야
//! "직접 쓰는 것과 구분이 안 된다"는 R1 완료 기준을 만족한다.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

pub struct VtScreen<'a> {
    screen: &'a vt100::Screen,
}

impl<'a> VtScreen<'a> {
    pub fn new(screen: &'a vt100::Screen) -> Self {
        Self { screen }
    }
}

impl<'a> Widget for VtScreen<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (srows, scols) = self.screen.size();
        let rows = area.height.min(srows);
        let cols = area.width.min(scols);

        for r in 0..rows {
            let mut c = 0u16;
            while c < cols {
                let Some(cell) = self.screen.cell(r, c) else {
                    c += 1;
                    continue;
                };
                let x = area.x + c;
                let y = area.y + r;
                let Some(target) = buf.cell_mut((x, y)) else {
                    c += 1;
                    continue;
                };

                let contents = cell.contents();
                if contents.is_empty() {
                    target.set_symbol(" ");
                } else {
                    target.set_symbol(&contents);
                }
                target.set_style(style_of(cell));

                // 와이드 문자(한글 등)는 두 칸을 차지한다. 다음 칸은 비워 둬야
                // 뒤 글자가 밀려 겹치지 않는다.
                if cell.is_wide() {
                    if let Some(next) = buf.cell_mut((x + 1, y)) {
                        next.set_symbol("");
                        next.set_style(style_of(cell));
                    }
                    c += 2;
                } else {
                    c += 1;
                }
            }
        }
    }
}

fn style_of(cell: &vt100::Cell) -> Style {
    let mut s = Style::default()
        .fg(convert(cell.fgcolor()))
        .bg(convert(cell.bgcolor()));
    let mut m = Modifier::empty();
    if cell.bold() {
        m |= Modifier::BOLD;
    }
    if cell.italic() {
        m |= Modifier::ITALIC;
    }
    if cell.underline() {
        m |= Modifier::UNDERLINED;
    }
    if cell.inverse() {
        m |= Modifier::REVERSED;
    }
    if !m.is_empty() {
        s = s.add_modifier(m);
    }
    s
}

fn convert(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}
