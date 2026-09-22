//! The jack-in sequence: a boot log that types itself out, one line at a time. The
//! lines are facts about this run (config, sector, keys, cache), not decoration.

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::{hash, unit};
use crate::{lexicon, theme};

const TYPE_CHARS_PER_SEC: f32 = 120.0;
/// Pause on the finished log before the grid takes over.
const LINGER: Duration = Duration::from_millis(450);
const LABEL_WIDTH: usize = 16;

pub struct BootLine {
    label: String,
    value: String,
}

impl BootLine {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }

    fn text(&self) -> String {
        format!(
            "> {:.<LABEL_WIDTH$} {}",
            format!("{} ", self.label),
            self.value
        )
    }
}

pub struct Boot {
    start: Instant,
    lines: Vec<BootLine>,
    per_line: Duration,
    skipped_at: Option<Instant>,
}

impl Boot {
    pub fn new(lines: Vec<BootLine>, per_line: Duration, now: Instant) -> Self {
        Self {
            start: now,
            lines,
            per_line,
            skipped_at: None,
        }
    }

    fn length(&self) -> Duration {
        // The header, each log line, then the closing "jacking in" line.
        self.per_line * (self.lines.len() as u32 + 2) + LINGER
    }

    pub fn done(&self, now: Instant) -> bool {
        self.skipped_at.is_some() || now.saturating_duration_since(self.start) >= self.length()
    }

    pub fn skip(&mut self, now: Instant) {
        self.skipped_at.get_or_insert(now);
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, now: Instant, ms: u64) {
        let elapsed = now.saturating_duration_since(self.start);
        let mut texts: Vec<(String, Style)> = vec![(
            format!(
                "{} // {} · v{}",
                lexicon::RIG,
                lexicon::RIG_ID,
                env!("CARGO_PKG_VERSION")
            ),
            Style::new().fg(theme::CYAN).add_modifier(Modifier::BOLD),
        )];
        texts.push((String::new(), Style::new()));
        texts.extend(
            self.lines
                .iter()
                .map(|l| (l.text(), Style::new().fg(theme::TEXT))),
        );
        texts.push((
            format!("> {}", lexicon::JACKING_IN),
            Style::new().fg(theme::MAGENTA).add_modifier(Modifier::BOLD),
        ));

        let width = texts
            .iter()
            .map(|(t, _)| t.chars().count())
            .max()
            .unwrap_or(0) as u16;
        let [_, middle, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(texts.len() as u16),
            Constraint::Fill(1),
        ])
        .areas(area);
        let [_, block, _] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(width.min(area.width)),
            Constraint::Fill(1),
        ])
        .areas(middle);

        let cursor_on = (ms / 400).is_multiple_of(2);
        let mut lines = Vec::new();
        for (i, (text, style)) in texts.iter().enumerate() {
            // The header and blank line count as one step so the log starts promptly.
            let step = i.saturating_sub(1) as u32;
            let Some(since) = elapsed.checked_sub(self.per_line * step) else {
                break;
            };
            let typed = (since.as_secs_f32() * TYPE_CHARS_PER_SEC) as usize;
            let shown: String = text.chars().take(typed).collect();
            let finished = typed >= text.chars().count();
            let mut spans = vec![Span::styled(scramble_tail(&shown, finished, ms), *style)];
            if !finished && cursor_on {
                spans.push(Span::styled("█", Style::new().fg(theme::CYAN)));
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), block);
    }
}

/// While a line is still typing, its last few characters flicker like they're
/// being decoded.
fn scramble_tail(shown: &str, finished: bool, ms: u64) -> String {
    const TAIL: usize = 3;
    const NOISE: &[u8] = b"#$%&*+=<>0123456789ABCDEF";
    if finished {
        return shown.to_string();
    }
    let count = shown.chars().count();
    shown
        .chars()
        .enumerate()
        .map(|(i, c)| {
            let h = hash(&[i as u64, ms / 50, 0xB007]);
            if i + TAIL >= count && c != ' ' && unit(h) < 0.7 {
                char::from(NOISE[(h % NOISE.len() as u64) as usize])
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn screen(boot: &Boot, at: Instant) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal.draw(|f| boot.draw(f, f.area(), at, 0)).unwrap();
        let buf = terminal.backend().buffer();
        (0..12)
            .map(|y| (0..80).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn types_out_line_by_line_then_finishes() {
        let t0 = Instant::now();
        let per_line = Duration::from_millis(260);
        let boot = Boot::new(
            vec![
                BootLine::new("config", "found"),
                BootLine::new("keys", "finnhub"),
            ],
            per_line,
            t0,
        );
        let early = screen(&boot, t0 + Duration::from_millis(300));
        assert!(early.contains("GHOSTWIRE // RIG-07"));
        assert!(!early.contains("keys"));

        let late = screen(&boot, t0 + per_line * 4);
        // Labels are dot-padded to a fixed width so the values line up.
        assert!(late.contains("> config ......... found"), "{late}");
        assert!(late.contains("> keys ........... finnhub"), "{late}");
        assert!(late.contains(lexicon::JACKING_IN));

        assert!(!boot.done(t0 + per_line * 4));
        assert!(boot.done(t0 + per_line * 4 + LINGER));
    }

    #[test]
    fn skipping_ends_it_at_once() {
        let t0 = Instant::now();
        let mut boot = Boot::new(Vec::new(), Duration::from_secs(1), t0);
        assert!(!boot.done(t0));
        boot.skip(t0);
        assert!(boot.done(t0));
    }
}
