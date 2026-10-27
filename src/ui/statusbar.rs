use std::time::Instant;

use chrono::Local;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::text::{fit, row};
use crate::app::App;
use crate::source::Link;
use crate::{lexicon, theme};

/// Columns held back for the "+N more" count once entries start dropping: " ░ +99 more".
const MORE_WIDTH: usize = 11;

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
            format!(
                " {} // {} // {}",
                lexicon::RIG,
                lexicon::CODENAME,
                app.config.rig.id()
            ),
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
    for (text, link) in trouble_entries(app, now) {
        items.push(Span::styled(
            text,
            Style::new().fg(theme::link_color(&link)),
        ));
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
    let left = pack(items, area.width as usize);
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

/// One entry per kind of trouble. Sources failing the same way for the same reason
/// share an entry, so an outage that takes every link down doesn't flood the ticker.
fn trouble_entries(app: &App, now: Instant) -> Vec<(String, Link)> {
    struct Group<'a> {
        link: &'a Link,
        error: Option<&'a str>,
        handles: Vec<&'static str>,
        /// The soonest retry among the group's sources.
        retry: Option<u64>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for (id, state) in &app.sources {
        if !matches!(
            state.link,
            Link::Ice | Link::Trace | Link::Flatlined | Link::Offline(_)
        ) {
            continue;
        }
        // Only ICE and FLATLINED show an error, so only they group by it.
        let error = match state.link {
            Link::Ice | Link::Flatlined => state.last_error.as_deref(),
            _ => None,
        };
        let retry = if state.in_flight {
            None
        } else {
            state.retry_in(now)
        };
        let brief = error.map(lexicon::error_brief);
        match groups
            .iter_mut()
            .find(|g| *g.link == state.link && g.error.map(lexicon::error_brief) == brief)
        {
            Some(g) => {
                g.handles.push(id.handle());
                g.retry = match (g.retry, retry) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
            }
            None => groups.push(Group {
                link: &state.link,
                error,
                handles: vec![id.handle()],
                retry,
            }),
        }
    }
    groups
        .into_iter()
        .filter_map(|g| {
            let handle = match g.handles.as_slice() {
                [one] => one.to_string(),
                many => lexicon::links(many.len()),
            };
            lexicon::trouble(&handle, g.link, g.retry, g.error).map(|t| (t, g.link.clone()))
        })
        .collect()
}

/// `items` after a leading space, separated. Once they'd run past `width`, whole
/// entries drop off the end and a "+N more" count stands in for them.
fn pack(items: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let sep_width = sep().content.width();
    let full = 1
        + items.iter().map(|s| s.content.width()).sum::<usize>()
        + sep_width * items.len().saturating_sub(1);
    let total = items.len();
    let room = if full <= width {
        width
    } else {
        width.saturating_sub(MORE_WIDTH)
    };
    let mut out = vec![Span::raw(" ")];
    let mut used = 1;
    let mut shown = 0;
    for item in items {
        let gap = if shown > 0 { sep_width } else { 0 };
        let w = item.content.width();
        if used + gap + w > room {
            if shown == 0 {
                // Not even the first entry fits whole, so show as much of it as does.
                out.push(Span::styled(fit(&item.content, room - used), item.style));
                shown = 1;
            }
            break;
        }
        if shown > 0 {
            out.push(sep());
        }
        out.push(item);
        used += gap + w;
        shown += 1;
    }
    if shown < total {
        out.push(sep());
        out.push(Span::styled(
            lexicon::more(total - shown),
            Style::new().fg(theme::MUTED),
        ));
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::text::width_of;

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn items(texts: &[&str]) -> Vec<Span<'static>> {
        texts.iter().map(|t| Span::raw(t.to_string())).collect()
    }

    #[test]
    fn pack_keeps_everything_that_fits() {
        let packed = pack(items(&["alpha", "beta"]), 40);
        assert_eq!(text(&packed), " alpha ░ beta");
    }

    #[test]
    fn pack_drops_whole_entries_and_counts_them() {
        let widest_marker = format!("{}{}", sep().content, lexicon::more(99));
        assert_eq!(MORE_WIDTH, widest_marker.width());
        let packed = pack(
            items(&["a".repeat(20).as_str(), "bbbbbbbbbb", "cccccccccc"]),
            45,
        );
        assert_eq!(
            text(&packed),
            format!(" {} ░ bbbbbbbbbb ░ +1 more", "a".repeat(20))
        );
        assert!(width_of(&packed) <= 45);
    }

    #[test]
    fn pack_cuts_a_lone_oversized_first_entry() {
        let packed = pack(items(&[&"x".repeat(80), "next"]), 40);
        assert!(text(&packed).ends_with("… ░ +1 more"));
        assert!(width_of(&packed) <= 40);
    }
}
