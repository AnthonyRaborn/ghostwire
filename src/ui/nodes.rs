use std::time::Instant;

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::text::{ago, bar, fit, price, row, spark, spinner, width_of};
use crate::app::App;
use crate::config::Units;
use crate::reading::{Quote, Reading, xray_class};
use crate::source::{Link, NodeId, SourceId};
use crate::{fx, geo, lexicon, theme};

/// How far fully decayed data fades toward the background.
const MAX_FADE: f32 = 0.7;
/// Ghost data is always at least this faded, however recent it is.
const GHOST_FADE: f32 = 0.4;

pub fn draw(frame: &mut Frame, area: Rect, app: &App, node: NodeId, now: DateTime<Utc>) {
    let link = app.node_link(node);
    let title = Span::styled(
        format!(" {} ", lexicon::node_title(node, &app.config.sector.name)),
        Style::new().fg(theme::CYAN).add_modifier(Modifier::BOLD),
    );
    let block = Block::bordered()
        .border_style(Style::new().fg(theme::border_color(link.as_ref())))
        .title(Line::from(title))
        .title(status(app, node, link.as_ref(), now).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(body(app, node, inner.width as usize, now)),
        inner,
    );

    let mut fade = app.node_decay(node, now) * MAX_FADE;
    if link == Some(Link::Ghost) {
        fade = fade.max(GHOST_FADE);
    }
    if fade > 0.0 {
        fx::fade(frame.buffer_mut(), inner, fade);
    }
}

fn status(app: &App, node: NodeId, link: Option<&Link>, now: DateTime<Utc>) -> Line<'static> {
    let Some(link) = link else {
        return Line::default();
    };
    let mut text = format!(" {}", lexicon::link_label(link));
    if matches!(link, Link::Live | Link::Ghost)
        && let Some(t) = app.node_last_ok(node)
    {
        text.push(' ');
        text.push_str(&ago(t, now));
    }
    if app.node_busy(node) {
        text.push(' ');
        text.push(spinner());
    }
    text.push(' ');
    Line::styled(text, Style::new().fg(theme::link_color(link)))
}

fn body(app: &App, node: NodeId, width: usize, now: DateTime<Utc>) -> Vec<Line<'static>> {
    let tracked: Vec<SourceId> = app.node_sources(node).map(|(id, _)| id).collect();
    if tracked.is_empty() {
        return vec![Line::styled(lexicon::NOT_WIRED, style(theme::MUTED))];
    }
    if tracked.iter().all(|id| !app.readings.contains_key(id)) {
        return tracked.iter().map(|id| awaiting(app, *id)).collect();
    }
    match node {
        NodeId::Zaibatsu => zaibatsu(app, width),
        NodeId::Atmos => atmos(app, width),
        NodeId::Intercepts => intercepts(app, width, now),
        NodeId::Seismic => seismic(app, width, now),
        NodeId::Helios => helios(app, width),
        NodeId::Sky => sky(app, width),
    }
}

fn style(fg: Color) -> Style {
    Style::new().fg(fg)
}

fn label(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), style(theme::MUTED))
}

fn value(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), style(theme::TEXT))
}

/// Placeholder line for a source with nothing to show yet.
fn awaiting(app: &App, id: SourceId) -> Line<'static> {
    let state = &app.sources[&id];
    let mut text = lexicon::awaiting(
        id.handle(),
        &state.link,
        state.in_flight,
        state.retry_in(Instant::now()),
    );
    if state.in_flight {
        text.push(' ');
        text.push(spinner());
    }
    let fg = if state.in_flight {
        theme::CYAN
    } else {
        theme::link_color(&state.link)
    };
    Line::styled(text, style(fg))
}

fn zaibatsu(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for id in [SourceId::Stocks, SourceId::Crypto] {
        if !app.sources.contains_key(&id) {
            continue;
        }
        match app.readings.get(&id) {
            Some(Reading::Stocks(quotes) | Reading::Crypto(quotes)) => {
                lines.extend(quotes.iter().map(|q| quote_line(q, width)));
            }
            _ => lines.push(awaiting(app, id)),
        }
    }
    lines
}

fn quote_line(q: &Quote, width: usize) -> Line<'static> {
    let up = q.change_pct >= 0.0;
    let color = if up { theme::GREEN } else { theme::MAGENTA };
    let arrow = if up { '▲' } else { '▼' };
    let head = format!("{:<6}{:>11} ", fit(&q.symbol, 6), price(q.price));
    let change = format!("{arrow}{:>5.1}% ", q.change_pct.abs());
    let spark_width = width.saturating_sub(head.width() + change.width());
    let mut spans = vec![value(head), Span::styled(change, style(color))];
    if spark_width >= 4 {
        spans.push(Span::styled(
            spark(&q.spark, spark_width, None),
            style(color),
        ));
    }
    Line::from(spans)
}

fn atmos(app: &App, width: usize) -> Vec<Line<'static>> {
    let Some(Reading::Weather(w)) = app.readings.get(&SourceId::Weather) else {
        return Vec::new();
    };
    let (deg, speed) = match w.units {
        Units::Metric => ("°C", "km/h"),
        Units::Imperial => ("°F", "mph"),
    };
    let mut lines = vec![
        row(
            vec![
                Span::styled(
                    format!("{:.0}{deg}", w.temp),
                    Style::new().fg(theme::TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(lexicon::weather(w.code), style(theme::YELLOW)),
            ],
            vec![label(format!("feels {:.0}°", w.feels_like))],
            width,
        ),
        Line::from(vec![
            label("PRECIP "),
            value(format!("{:.0}%", w.precip_prob)),
            label("  WIND "),
            // Arrow shows where the wind is headed, not where it's from.
            value(format!(
                "{:.0}{speed} {}",
                w.wind_speed,
                geo::arrow(w.wind_from + 180.0)
            )),
            label("  HUM "),
            value(format!("{:.0}%", w.humidity)),
        ]),
    ];
    let mut air = Vec::new();
    if let Some(aqi) = w.us_aqi {
        let color = aqi_color(aqi);
        air.extend([
            label("SMOG "),
            Span::styled(format!("AQI {aqi:.0} {} ", lexicon::aqi(aqi)), style(color)),
            Span::styled(bar(aqi / 300.0, 6), style(color)),
        ]);
    }
    if let Some(uv) = w.uv_index {
        air.extend([label("  UV "), value(format!("{uv:.0}"))]);
    }
    if !air.is_empty() {
        lines.push(Line::from(air));
    }
    if !w.next_24h.is_empty() {
        let lo = w.next_24h.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = w.next_24h.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let range = format!(" {lo:.0}–{hi:.0}°");
        let head = "NEXT 24H ";
        let spark_width = width.saturating_sub(head.len() + range.width());
        lines.push(Line::default());
        lines.push(Line::from(vec![
            label(head),
            Span::styled(spark(&w.next_24h, spark_width, None), style(theme::CYAN)),
            label(range),
        ]));
    }
    lines
}

fn aqi_color(aqi: f64) -> Color {
    match aqi {
        a if a <= 50.0 => theme::GREEN,
        a if a <= 100.0 => theme::YELLOW,
        a if a <= 150.0 => theme::ORANGE,
        _ => theme::MAGENTA,
    }
}

fn intercepts(app: &App, width: usize, now: DateTime<Utc>) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if app.sources.contains_key(&SourceId::Kev) {
        match app.readings.get(&SourceId::Kev) {
            Some(Reading::Kev(vulns)) => {
                for v in vulns.iter().take(3) {
                    let days = (now.date_naive() - v.added).num_days();
                    let mut right = Vec::new();
                    if v.ransomware {
                        right.push(Span::styled("RANSOM ", style(theme::RED)));
                    }
                    right.push(label(if days <= 0 {
                        "today".to_string()
                    } else {
                        format!("{days}d")
                    }));
                    let tag = Span::styled("▓ KEV ", style(theme::MAGENTA));
                    let room = width.saturating_sub(width_of(&right) + 7);
                    let detail = format!("{} {} {}", v.cve, v.vendor, v.product);
                    lines.push(row(vec![tag, value(fit(&detail, room))], right, width));
                }
            }
            _ => lines.push(awaiting(app, SourceId::Kev)),
        }
    }
    if app.sources.contains_key(&SourceId::Hn) {
        match app.readings.get(&SourceId::Hn) {
            Some(Reading::Hn(stories)) => {
                for s in stories {
                    let right = vec![label(format!("{}▲", s.score))];
                    let tag = Span::styled("▓ HN  ", style(theme::CYAN));
                    let room = width.saturating_sub(width_of(&right) + 7);
                    lines.push(row(vec![tag, value(fit(&s.title, room))], right, width));
                }
            }
            _ => lines.push(awaiting(app, SourceId::Hn)),
        }
    }
    lines
}

fn seismic(app: &App, width: usize, now: DateTime<Utc>) -> Vec<Line<'static>> {
    let Some(Reading::Quakes(quakes)) = app.readings.get(&SourceId::Quakes) else {
        return Vec::new();
    };
    let radius = app.config.sector.radius_km;
    quakes
        .iter()
        .map(|q| {
            let near = q.distance_km.is_some_and(|d| d <= radius);
            let place = match (near, q.distance_km, q.bearing) {
                (true, Some(d), Some(b)) => format!("{d:.0}km {} · {}", geo::compass(b), q.place),
                _ => q.place.clone(),
            };
            let mut mag_style = style(mag_color(q.mag));
            if q.mag >= 4.5 {
                mag_style = mag_style.add_modifier(Modifier::BOLD);
            }
            let mut right = Vec::new();
            if q.tsunami {
                right.push(Span::styled("TSUNAMI ", style(theme::RED)));
            }
            right.push(label(ago(q.time, now)));
            let mag = Span::styled(format!("M{:.1} ", q.mag), mag_style);
            let room = width.saturating_sub(mag.width() + width_of(&right) + 1);
            let place = Span::styled(
                fit(&place, room),
                style(if near { theme::TEXT } else { theme::MUTED }),
            );
            row(vec![mag, place], right, width)
        })
        .collect()
}

fn mag_color(mag: f64) -> Color {
    match mag {
        m if m < 2.5 => theme::MUTED,
        m if m < 4.5 => theme::TEXT,
        m if m < 6.0 => theme::YELLOW,
        _ => theme::MAGENTA,
    }
}

fn helios(app: &App, width: usize) -> Vec<Line<'static>> {
    let Some(Reading::Swpc(s)) = app.readings.get(&SourceId::Swpc) else {
        return Vec::new();
    };
    let kp_color = match s.kp {
        k if k < 4.0 => theme::GREEN,
        k if k < 5.0 => theme::YELLOW,
        _ => theme::MAGENTA,
    };
    let scale = |letter: char, level: u8| {
        let fg = match level {
            0 => theme::MUTED,
            1 | 2 => theme::YELLOW,
            _ => theme::MAGENTA,
        };
        Span::styled(format!("{letter}{level} "), style(fg))
    };
    vec![
        Line::from(vec![
            label("Kp "),
            Span::styled(
                format!("{:.1} {} ", s.kp, lexicon::kp(s.kp)),
                style(kp_color),
            ),
            Span::styled(bar(s.kp / 9.0, 9), style(kp_color)),
        ]),
        Line::from(vec![
            label("24H "),
            Span::styled(
                spark(&s.kp_history, width.saturating_sub(4), Some((0.0, 9.0))),
                style(theme::CYAN),
            ),
        ]),
        Line::from(vec![
            label("X-RAY "),
            value(s.xray_flux.map_or_else(|| "—".to_string(), xray_class)),
        ]),
        Line::from(vec![
            scale('G', s.scales.g),
            scale('S', s.scales.s),
            scale('R', s.scales.r),
            label("// NOAA SCALES"),
        ]),
    ]
}

fn sky(app: &App, width: usize) -> Vec<Line<'static>> {
    let Some(Reading::OpenSky(contacts)) = app.readings.get(&SourceId::OpenSky) else {
        return Vec::new();
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("{} CONTACTS", contacts.len()),
            Style::new().fg(theme::TEXT).add_modifier(Modifier::BOLD),
        ),
        label(format!(" // {:.0}km", app.config.sector.radius_km)),
    ])];
    for c in contacts {
        let flight_level = c
            .altitude_m
            .map_or_else(|| "FL---".to_string(), |m| format!("FL{:03.0}", m / 30.48));
        let knots = c.speed_ms.map_or_else(
            || "  -kt".to_string(),
            |v| format!("{:>3.0}kt", v * 1.943_84),
        );
        let heading = c.heading.map_or('·', geo::arrow);
        let left = format!(
            "{:<8} {flight_level} {heading} {knots}",
            fit(&c.callsign, 8)
        );
        let right = format!("{:.0}km {}", c.distance_km, geo::compass(c.bearing));
        lines.push(row(vec![value(left)], vec![label(right)], width));
    }
    lines
}
