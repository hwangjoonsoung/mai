//! 왼쪽 사이드바 — spaces(작업 공간)와 agents(그 공간의 세션).
//!
//! 두 칸 모두 **클릭 가능**하고 **휠로 스크롤**된다. 공간을 클릭하면 아래
//! agents 칸이 그 공간에서 도는 세션으로 바뀐다.
//!
//! 상태는 추측하지 않고 **관측 가능한 사실만** 쓴다 — 프로세스 생존, 최근 출력
//! 여부. "무엇을 하는 중인지"는 우리가 알 수 없다.

use crate::app::App;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

pub const WIDTH: u16 = 26;

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        // spaces 와 agents 를 반반으로 나눈다.
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(area);

    draw_spaces(f, rows[0], app);
    draw_agents(f, rows[1], app);
}

fn draw_spaces(f: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" spaces ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.set_spaces_area(inner);

    // 스크롤 오프셋만큼 건너뛰고 보이는 만큼만 그린다. 그래야 클릭 좌표와
    // 화면이 어긋나지 않는다.
    let skip = app.space_scroll as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut y = inner.y;

    let mut row = 0usize;
    // 맨 위는 «새 공간» 버튼이다.
    if row >= skip && y < inner.bottom() {
        lines.push(Line::from(Span::styled(
            "+ 새 공간",
            Style::default().fg(Color::Green),
        )));
        app.set_add_space_hit(Rect { x: inner.x, y, width: inner.width, height: 1 });
        y += 1;
    }
    row += 1;

    let active = app.active_space;
    let names: Vec<(usize, String, String)> = app
        .spaces
        .iter()
        .enumerate()
        .map(|(i, s)| (i, s.name.clone(), s.path.to_string_lossy().into_owned()))
        .collect();

    for (i, name, path) in names {
        if row < skip {
            row += 1;
            continue;
        }
        if y >= inner.bottom() {
            break;
        }
        let sel = i == active;
        let style = if sel {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(vec![
            Span::styled(if sel { "▸ " } else { "  " }, Style::default().fg(Color::Cyan)),
            Span::styled(shorten(&name, inner.width.saturating_sub(2) as usize), style),
        ]));
        app.push_space_hit(i, Rect { x: inner.x, y, width: inner.width, height: 1 });
        y += 1;
        row += 1;

        // 선택된 공간만 경로를 한 줄 더 보여준다. 전부 보여주면 목록이 길어진다.
        if sel && y < inner.bottom() {
            lines.push(Line::from(Span::styled(
                shorten_tail(&path, inner.width as usize),
                Style::default().fg(Color::DarkGray),
            )));
            y += 1;
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_agents(f: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" agents ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.set_agents_area(inner);

    let vis = app.visible();
    if vis.is_empty() {
        f.render_widget(
            Paragraph::new("세션 없음").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let focus = app.focus;
    let skip = app.agent_scroll as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut y = inner.y;

    #[allow(clippy::type_complexity)]
    let rows: Vec<(usize, String, Color, Vec<(String, bool)>)> = vis
        .iter()
        .map(|&i| {
            let s = &app.sessions[i];
            let subs = s
                .subs
                .iter()
                .map(|b| {
                    // 좁은 칸이라 설명은 짧게. 종류가 더 중요하다.
                    let d = shorten(&b.desc, 14);
                    (if d.is_empty() { b.kind.clone() } else { format!("{} {}", b.kind, d) }, b.running)
                })
                .collect();
            (i, s.title.clone(), s.state().color(), subs)
        })
        .collect();

    let mut n = 0usize;
    for (i, title, color, subs) in rows {
        if n >= skip && y < inner.bottom() {
            let style = if i == focus {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            // 상태는 색 점으로만. 글자로 적으면 좁은 칸에서 시끄럽다.
            lines.push(Line::from(vec![
                Span::styled("● ", Style::default().fg(color)),
                Span::styled(title, style),
            ]));
            app.push_agent_hit(i, Rect { x: inner.x, y, width: inner.width, height: 1 });
            y += 1;
        }
        n += 1;

        // 서브에이전트 — 에이전트가 자기 화면에 그린 명부에서 읽은 것이다.
        for (k, (label, running)) in subs.iter().enumerate() {
            if n < skip {
                n += 1;
                continue;
            }
            if y >= inner.bottom() {
                break;
            }
            let last = k + 1 == subs.len();
            lines.push(Line::from(vec![
                Span::styled(
                    if last { "  └ " } else { "  ├ " },
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    label.clone(),
                    Style::default().fg(if *running { Color::Yellow } else { Color::DarkGray }),
                ),
            ]));
            y += 1;
            n += 1;
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

/// 이름이 길면 뒤를 자른다.
fn shorten(s: &str, width: usize) -> String {
    if width == 0 || s.chars().count() <= width {
        return s.to_string();
    }
    let keep: String = s.chars().take(width.saturating_sub(1)).collect();
    format!("{keep}…")
}

/// 경로가 길면 앞을 줄여 끝을 보여준다. 끝이 더 유용하다.
fn shorten_tail(s: &str, width: usize) -> String {
    if width == 0 || s.chars().count() <= width {
        return s.to_string();
    }
    let keep: String = s.chars().skip(s.chars().count() - width + 1).collect();
    format!("…{keep}")
}
