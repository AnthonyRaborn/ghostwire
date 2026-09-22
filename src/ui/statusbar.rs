use std::time::Instant;

use chrono::Local;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use super::text::row;
use crate::app::App;
use crate::source::Link;
use crate::{lexicon, theme};

fn sep() -> Span<'static> {
    Span::styled(" ░ ", Style::new().fg(theme::DIM))
}

fn label(text: &str) -> Span<'static> {
    Span::styled(text.to_string(), Style::new().fg(theme::MUTED))
}

fn value(text: String) -> Span<'static> {
    Span::styled(text, Style::new().fg(theme::TEXT))
}

pub fn draw_top(frame: &mut Frame, area: Rect, app: &App) {
    let (live, total) = app.uplink();
    let mut left = vec![
        Span::styled(
            format!(" {} // {}", lexicon::RIG, lexicon::RIG_ID),
            Style::new().fg(theme::CYAN).add_modifier(Modifier::BOLD),
        ),
        sep(),
        label("uplink "),
        value(format!("{live}/{total}")),
        sep(),
        label("neural load "),
        value(format!("{:.1}%", app.cpu.percent())),
        sep(),
        value(app.config.sector.name.clone()),
    ];
    if app.demo {
        left.push(sep());
        left.push(Span::styled(
            lexicon::CONSTRUCT,
            Style::new().fg(theme::MAGENTA).add_modifier(Modifier::BOLD),
        ));
    }
    let clock = Span::styled(
        Local::now().format("%H:%M:%S NET ").to_string(),
        Style::new().fg(theme::CYAN),
    );
    frame.render_widget(
        Paragraph::new(row(left, vec![clock], area.width as usize)),
        area,
    );
}

pub fn draw_ticker(frame: &mut Frame, area: Rect, app: &App) {
    let now = Instant::now();
    let mut items = Vec::new();
    if let Some(intercept) = app.live_intercept(now) {
        items.push(Span::styled(
            lexicon::intercept(
                &lexicon::node_title(intercept.node, &app.config.sector.name),
                &intercept.text,
            ),
            Style::new().fg(theme::MAGENTA).add_modifier(Modifier::BOLD),
        ));
    }
    if !app.config_found && !app.demo {
        items.push(Span::styled(
            lexicon::NO_CONFIG,
            Style::new().fg(theme::YELLOW),
        ));
    }
    for warning in &app.warnings {
        items.push(Span::styled(
            warning.clone(),
            Style::new().fg(theme::YELLOW),
        ));
    }
    for (id, state) in &app.sources {
        let retry = if state.in_flight {
            None
        } else {
            state.retry_in(now)
        };
        if let Some(text) = lexicon::trouble(id.handle(), &state.link, retry) {
            items.push(Span::styled(
                text,
                Style::new().fg(theme::link_color(&state.link)),
            ));
        }
    }
    let ghosts = app
        .sources
        .values()
        .filter(|s| s.link == Link::Ghost)
        .count();
    if ghosts > 0 {
        items.push(Span::styled(
            lexicon::ghosts(ghosts),
            Style::new().fg(theme::GHOST),
        ));
    }
    if items.is_empty() {
        items.push(Span::styled(
            lexicon::NOMINAL,
            Style::new().fg(theme::GREEN),
        ));
    }
    if let Some(text) = dive_status(app, now) {
        items.push(Span::styled(text, Style::new().fg(theme::CYAN)));
    }
    let mut left = vec![Span::raw(" ")];
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            left.push(sep());
        }
        left.push(item);
    }
    let keys = if app.dive.diving().is_some() {
        lexicon::KEYS_DIVE
    } else {
        lexicon::KEYS_GRID
    };
    let keys = Span::styled(format!("{keys} "), Style::new().fg(theme::MUTED));
    frame.render_widget(
        Paragraph::new(row(left, vec![keys], area.width as usize)),
        area,
    );
}

/// What the dive cycle will do next, if there's anything to dive into.
fn dive_status(app: &App, now: Instant) -> Option<String> {
    if app.dive.held() {
        return Some(lexicon::DIVE_HELD.into());
    }
    let secs = app.dive.next_in(now)?.as_secs_f32().ceil() as u64;
    if app.dive.diving().is_some() {
        return Some(lexicon::surfacing_in(secs));
    }
    let ready = app.ready_nodes();
    let next = app.dive.peek(|n| ready.contains(&n))?;
    let title = lexicon::node_title(next, &app.config.sector.name);
    Some(lexicon::diving_in(&title, secs))
}
