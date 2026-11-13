//! 왼쪽 사이드바 — spaces(작업 공간)와 agents(그 공간의 세션).
//!
//! 두 칸 모두 **클릭 가능**하고 **휠로 스크롤**된다. 공간을 클릭하면 아래
//! agents 칸이 그 공간에서 도는 세션으로 바뀐다.
//!
//! 상태는 추측하지 않고 **관측 가능한 사실만** 쓴다 — 프로세스 생존, 최근 출력
//! 여부. "무엇을 하는 중인지"는 우리가 알 수 없다.

use crate::{app::App, model::State, theme};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub const WIDTH: u16 = 26;

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    // 상자는 두르지 않는다. 오른쪽에 구분선 하나만 긋고 좌우로 한 칸씩 띄운다.
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(theme::line()));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };

    // 공간 목록은 필요한 만큼만 차지한다(제목 + 공간들 + «새 공간»). 다만 절반을
    // 넘기지 않는다 — 넘치면 휠로 굴린다. 한 줄 띄고 나머지가 에이전트다.
    let need = app.spaces.len() as u16 + 2;
    let top_h = need.min(inner.height / 2).max(inner.height.min(3));
    let top = Rect { height: top_h, ..inner };
    let rest = top.bottom() + 1;
    let bottom = Rect {
        y: rest.min(inner.bottom()),
        height: inner.bottom().saturating_sub(rest),
        ..inner
    };

    draw_spaces(f, top, app);
    draw_agents(f, bottom, app);
}

/// 구역의 제목을 흐리게 적고, 그 아래 목록이 들어갈 자리를 돌려준다.
fn section(f: &mut Frame, area: Rect, title: &str) -> Rect {
    f.render_widget(
        Paragraph::new(title).style(Style::default().fg(theme::dim())),
        Rect { height: area.height.min(1), ..area },
    );
    Rect { y: area.y + area.height.min(1), height: area.height.saturating_sub(1), ..area }
}

fn draw_spaces(f: &mut Frame, area: Rect, app: &mut App) {
    let inner = section(f, area, "공간");
    app.set_spaces_area(inner);

    // 줄들을 먼저 만든다. 공간들이 먼저 오고 «새 공간» 이 맨 끝이다.
    // None 은 «새 공간» 줄이다.
    let active = app.active_space;
    let width = inner.width.saturating_sub(2) as usize;
    let mut rows: Vec<(Option<usize>, Line)> = app
        .spaces
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let sel = i == active;
            let style = if sel {
                Style::default().fg(theme::text()).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::dim())
            };
            let line = Line::from(vec![
                Span::styled(if sel { "▸ " } else { "  " }, Style::default().fg(theme::focus())),
                Span::styled(shorten(&s.name, width), style),
            ]);
            (Some(i), line)
        })
        .collect();
    rows.push((
        None,
        Line::from(Span::styled("+ 새 공간", Style::default().fg(theme::dim()))),
    ));

    // 스크롤 오프셋만큼 건너뛰고 보이는 만큼만 그린다. 그래야 클릭 좌표와
    // 화면이 어긋나지 않는다.
    let skip = app.space_scroll as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut y = inner.y;
    for (hit, line) in rows.into_iter().skip(skip) {
        if y >= inner.bottom() {
            break;
        }
        let r = Rect { x: inner.x, y, width: inner.width, height: 1 };
        match hit {
            Some(i) => app.push_space_hit(i, r),
            None => app.set_add_space_hit(r),
        }
        lines.push(line);
        y += 1;
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_agents(f: &mut Frame, area: Rect, app: &mut App) {
    let inner = section(f, area, "에이전트");
    app.set_agents_area(inner);

    let vis = app.visible();
    if vis.is_empty() {
        f.render_widget(
            Paragraph::new("세션 없음").style(Style::default().fg(theme::dim())),
            inner,
        );
        return;
    }

    // 검증자는 요청한 칸 바로 아래에 들여 쓴다. 요청자가 이 공간에 안 보이면
    // (닫혔거나 다른 공간) 맨 앞줄에 그대로 둔다.
    let mut order: Vec<(usize, bool)> = Vec::new();
    for &i in &vis {
        let parent = app.sessions[i].verifier_of;
        let nested = parent.is_some_and(|p| vis.iter().any(|&j| app.sessions[j].id == p && j != i));
        if nested {
            continue;
        }
        order.push((i, false));
        let id = app.sessions[i].id;
        for &j in &vis {
            if j != i && app.sessions[j].verifier_of == Some(id) {
                order.push((j, true));
            }
        }
    }

    // 한 칸이 차지하는 줄들을 먼저 만든다. 그려 넣는 쪽은 스크롤만 신경 쓴다.
    // 첫 줄만 클릭 대상이다.
    let focus = app.focus;
    let width = inner.width as usize;
    let mut rows: Vec<(Option<usize>, Line)> = Vec::new();
    for (i, nested) in order {
        let s = &app.sessions[i];
        let indent = if nested { "  " } else { "" };

        // 관측된 상태는 이름 뒤에 기호와 글자로 붙인다. 색만으로 구분하지 않는다.
        let (mark, mark_color) = match s.state() {
            State::Starting | State::Working => (" …", theme::warn()),
            State::Exited => (" – 종료", theme::dim()),
            State::Quiet => ("", theme::dim()),
        };
        // 이름에 남는 폭 — 들여쓰기·가지(`└ `)·점(`● `)·상태 표시를 뺀다.
        // 한글은 두 칸이라 상태 표시는 글자 수의 두 배로 넉넉히 잡는다.
        let head = indent.len() + if nested { 4 } else { 2 };
        let room = width.saturating_sub(head + mark.chars().count() * 2);
        let name_style = if i == focus {
            Style::default().fg(theme::text()).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let mut spans = vec![Span::raw(indent)];
        if nested {
            spans.push(Span::styled("└ ", Style::default().fg(theme::dim())));
        }
        // 점은 상태가 아니라 «누구인가» 다. 에이전트마다 색이 고정돼 있다.
        spans.push(Span::styled("● ", Style::default().fg(theme::agent(&s.agent_id))));
        spans.push(Span::styled(shorten(&s.title, room), name_style));
        spans.push(Span::styled(mark, Style::default().fg(mark_color)));
        rows.push((Some(i), Line::from(spans)));

        // 요청 진행 — 요청한 쪽과 답하는 쪽 양쪽에 적는다. `mai ctl ask` 는 검증만이
        // 아니라 여러 작업에 쓰이므로 «검증» 이라고 적지 않는다.
        let below = format!("{indent}{}", if nested { "    " } else { "  " });
        if let Some(since) = app.verifying_since(s.id) {
            let t = since.elapsed().as_secs();
            rows.push((
                None,
                Line::from(Span::styled(
                    format!("{below}… 작업 중 {}:{:02}", t / 60, t % 60),
                    Style::default().fg(theme::warn()),
                )),
            ));
        } else if app.requesting(s.id) {
            rows.push((
                None,
                Line::from(Span::styled(
                    format!("{below}… 답 대기 중"),
                    Style::default().fg(theme::warn()),
                )),
            ));
        }

        // 서브에이전트 — 에이전트가 자기 화면에 그린 명부에서 읽은 것이다.
        for (k, b) in s.subs.iter().enumerate() {
            // 좁은 칸이라 설명은 짧게. 종류가 더 중요하다.
            let d = shorten(&b.desc, 14);
            let label = if d.is_empty() { b.kind.clone() } else { format!("{} {}", b.kind, d) };
            let last = k + 1 == s.subs.len();
            rows.push((
                None,
                Line::from(vec![
                    Span::styled(
                        format!("{below}{}", if last { "└ " } else { "├ " }),
                        Style::default().fg(theme::dim()),
                    ),
                    Span::styled(
                        label,
                        Style::default().fg(if b.running { theme::warn() } else { theme::dim() }),
                    ),
                ]),
            ));
        }
    }

    // 스크롤 오프셋만큼 건너뛰고 보이는 만큼만 그린다.
    let skip = app.agent_scroll as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut y = inner.y;
    for (hit, line) in rows.into_iter().skip(skip) {
        if y >= inner.bottom() {
            break;
        }
        if let Some(i) = hit {
            app.push_agent_hit(i, Rect { x: inner.x, y, width: inner.width, height: 1 });
        }
        lines.push(line);
        y += 1;
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
