//! The `?` manual: controls, what the link states and signals mean, and where the rig
//! keeps its files. Drawn over everything else; side by side when there's room.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::nodes::{label, value};
use crate::app::App;
use crate::source::Link;
use crate::{lexicon, theme};

/// Each column's width, and the gap between the two when they sit side by side.
const COLUMN: u16 = 58;
const GAP: u16 = 4;
/// Border plus a column of padding on each side.
const FRAME: u16 = 4;

fn heading(text: &str) -> Line<'static> {
    Line::styled(
        text.to_string(),
        Style::new().fg(theme::MAGENTA).add_modifier(Modifier::BOLD),
    )
}

fn padded(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(text.width())))
}

fn controls(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![
        heading("CONTROLS"),
        Line::from(vec![
            label(padded("", 8)),
            label(padded("GRID", 33)),
            label("DIVE"),
        ]),
    ];
    for (key, grid, dive) in lexicon::HELP_KEYS {
        lines.push(Line::from(vec![
            Span::styled(padded(key, 8), Style::new().fg(theme::CYAN)),
            value(padded(grid, 33)),
            label(*dive),
        ]));
    }
    if !app.files.is_empty() {
        lines.push(Line::default());
        lines.push(heading("FILES"));
        for (what, path) in &app.files {
            lines.push(Line::from(vec![
                label(padded(what, 8)),
                value(path.clone()),
            ]));
        }
    }
    lines
}

fn meanings() -> Vec<Line<'static>> {
    let mut lines = vec![heading("LINK STATES")];
    let links = [
        Link::Live,
        Link::Ghost,
        Link::Pending,
        Link::Ice,
        Link::Trace,
        Link::Flatlined,
        Link::Offline(String::new()),
    ];
    for link in &links {
        lines.push(Line::from(vec![
            Span::styled(
                padded(lexicon::link_label(link), 11),
                Style::new().fg(theme::link_color(link)),
            ),
            value(lexicon::link_meaning(link)),
        ]));
    }
    lines.push(Line::default());
    lines.push(heading("SIGNALS"));
    for (signal, meaning) in lexicon::HELP_SIGNALS {
        lines.push(Span::styled(*signal, Style::new().fg(theme::YELLOW)).into());
        for line in *meaning {
            lines.push(Line::from(vec![label("  "), value(*line)]));
        }
    }
    lines
}

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let (left, right) = (controls(app), meanings());
    let side_by_side = area.width >= COLUMN * 2 + GAP + FRAME;
    let (width, body_height) = if side_by_side {
        (COLUMN * 2 + GAP, left.len().max(right.len()))
    } else {
        (COLUMN, left.len() + 1 + right.len())
    };
    // Room for the border and the closing hint under the body.
    let height = (body_height as u16 + 4).min(area.height);
    let width = (width + FRAME).min(area.width);
    let [_, popup, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, popup, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .areas(popup);

    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(theme::CYAN))
        .style(Style::new().bg(theme::BG))
        .title(Line::styled(
            lexicon::HELP_TITLE,
            Style::new().fg(theme::CYAN).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [_, inner, _] = Layout::horizontal([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(inner);
    let [body, _, hint] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    if side_by_side {
        let [l, _, r] = Layout::horizontal([
            Constraint::Length(COLUMN),
            Constraint::Length(GAP),
            Constraint::Length(COLUMN),
        ])
        .areas(body);
        frame.render_widget(Paragraph::new(left), l);
        frame.render_widget(Paragraph::new(right), r);
    } else {
        let mut lines = left;
        lines.push(Line::default());
        lines.extend(right);
        frame.render_widget(Paragraph::new(lines), body);
    }
    frame.render_widget(
        Paragraph::new(label(lexicon::HELP_CLOSE)).right_aligned(),
        hint,
    );
}
