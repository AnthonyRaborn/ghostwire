//! A dive: one node takes over the grid with everything it knows.

use std::collections::HashSet;
use std::time::Instant;

use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph, Sparkline};

use super::nodes::{aqi_color, awaiting, kp_color, label, mag_color, status, value};
use super::text::{ago, bar, distance, fit, fit_series, price, row, width_of};
use super::{bigtext, radar};
use crate::app::App;
use crate::config::Units;
use crate::fx::Fx;
use crate::reading::{Quake, Quote, Reading, xray_class};
use crate::source::{NodeId, SourceId};
use crate::{geo, lexicon, theme};

/// Contacts and quakes that get a name tag on the scope.
const RADAR_LABELS: usize = 6;

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    node: NodeId,
    now: DateTime<Utc>,
    instant: Instant,
    fx: &mut Fx,
) {
    let link = app.node_link(node);
    let mut right = status(app, node, link.as_ref(), now);
    let timer = match (app.dive.held(), app.dive.next_in(instant)) {
        (true, _) => " HELD ".to_string(),
        (false, Some(left)) => format!(" SURFACE {}s ", left.as_secs_f32().ceil() as u64),
        (false, None) => String::new(),
    };
    right
        .spans
        .push(Span::styled(timer, Style::new().fg(theme::CYAN)));
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(theme::CYAN))
        .title(Line::styled(
            lexicon::dive_title(&lexicon::node_title(node, &app.config.sector.name)),
            Style::new().fg(theme::CYAN).add_modifier(Modifier::BOLD),
        ))
        .title(right.right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let body = inner.inner(ratatui::layout::Margin::new(1, 0));
    match node {
        NodeId::Zaibatsu => zaibatsu(frame, body, app),
        NodeId::Atmos => atmos(frame, body, app),
        NodeId::Intercepts => intercepts(frame, body, app, now),
        NodeId::Seismic => seismic(frame, body, app, now),
        NodeId::Helios => helios(frame, body, app),
        NodeId::Sky => sky(frame, body, app),
    }
    let decay = app.node_decay(node, now);
    let buf = frame.buffer_mut();
    super::nodes::fade_for(buf, inner, decay, link.as_ref());
    fx.node(buf, area, inner, node, instant, decay);
}

fn header(text: impl Into<String>) -> Line<'static> {
    Line::styled(
        text.into(),
        Style::new().fg(theme::MAGENTA).add_modifier(Modifier::BOLD),
    )
}

/// A bar chart of `values` across the full width of `area`.
fn chart(frame: &mut Frame, area: Rect, values: &[f64], range: Option<(f64, f64)>, color: Color) {
    if area.is_empty() || values.is_empty() {
        return;
    }
    let points = fit_series(values, area.width as usize);
    let (lo, hi) = range.unwrap_or_else(|| {
        points
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            })
    });
    // Everything gets at least a sliver so the line never has gaps.
    let scaled: Vec<u64> = points
        .iter()
        .map(|&v| {
            let t = if hi > lo {
                ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
            } else {
                0.5
            };
            1 + (t * 99.0).round() as u64
        })
        .collect();
    frame.render_widget(
        Sparkline::default()
            .data(scaled.iter().copied())
            .max(100)
            .style(Style::new().fg(color)),
        area,
    );
}

/// Labels under a gauge, each at its value on a `0..=max` scale.
fn axis_at(labels: &[(f64, &str)], max: f64, width: usize) -> Line<'static> {
    let mut text = vec![' '; width];
    for (at, label) in labels {
        let len = label.chars().count();
        let start = ((at / max) * width as f64).round() as usize;
        let start = start.saturating_sub(len / 2).min(width.saturating_sub(len));
        for (j, c) in label.chars().enumerate() {
            if let Some(slot) = text.get_mut(start + j) {
                *slot = c;
            }
        }
    }
    Line::styled(
        text.into_iter().collect::<String>(),
        Style::new().fg(theme::MUTED),
    )
}

/// Evenly spaced labels under a chart, e.g. `-72h ... now`.
fn axis(labels: &[&str], width: usize) -> Line<'static> {
    let mut text = vec![' '; width];
    let last = labels.len().saturating_sub(1).max(1);
    for (i, label) in labels.iter().enumerate() {
        let len = label.chars().count();
        let start = (i * width.saturating_sub(len) / last).min(width.saturating_sub(len));
        for (j, c) in label.chars().enumerate() {
            if let Some(slot) = text.get_mut(start + j) {
                *slot = c;
            }
        }
    }
    Line::styled(
        text.into_iter().collect::<String>(),
        Style::new().fg(theme::MUTED),
    )
}

fn zaibatsu(frame: &mut Frame, area: Rect, app: &App) {
    let sections: Vec<(SourceId, &str)> = [
        (SourceId::Stocks, "EQUITIES // SESSION"),
        (SourceId::Crypto, "CRYPTO // 7 DAYS"),
    ]
    .into_iter()
    .filter(|(id, _)| app.sources.contains_key(id))
    .collect();
    let quotes = |id: SourceId| match app.readings.get(&id) {
        Some(Reading::Stocks(q) | Reading::Crypto(q)) => q.as_slice(),
        _ => &[],
    };
    let count: u16 = sections
        .iter()
        .map(|(id, _)| quotes(*id).len().max(1) as u16)
        .sum();
    let spare = area
        .height
        .saturating_sub(count + sections.len() as u16 * 2);
    let chart_rows = (spare / count.max(1)).clamp(0, 4);

    let mut y = area.y;
    let bottom = area.bottom();
    for (id, title) in sections {
        if y >= bottom {
            break;
        }
        frame.render_widget(
            Paragraph::new(header(title)),
            Rect {
                y,
                height: 1,
                ..area
            },
        );
        y += 1;
        let list = quotes(id);
        if list.is_empty() {
            frame.render_widget(
                Paragraph::new(awaiting(app, id)),
                Rect {
                    y,
                    height: 1,
                    ..area
                },
            );
            y += 2;
            continue;
        }
        for q in list {
            if y >= bottom {
                break;
            }
            frame.render_widget(
                Paragraph::new(quote_row(q, area.width as usize)),
                Rect {
                    y,
                    height: 1,
                    ..area
                },
            );
            y += 1;
            let rows = chart_rows.min(bottom - y);
            let color = if q.change_pct >= 0.0 {
                theme::GREEN
            } else {
                theme::MAGENTA
            };
            chart(
                frame,
                Rect {
                    y,
                    height: rows,
                    ..area
                },
                &q.spark,
                None,
                color,
            );
            y += rows;
        }
        y += 1;
    }
}

fn quote_row(q: &Quote, width: usize) -> Line<'static> {
    let up = q.change_pct >= 0.0;
    let color = if up { theme::GREEN } else { theme::MAGENTA };
    let arrow = if up { '▲' } else { '▼' };
    let left = vec![
        Span::styled(
            format!("{:<8}", q.symbol),
            Style::new().fg(theme::TEXT).add_modifier(Modifier::BOLD),
        ),
        value(format!("{:>12}  ", price(q.price))),
        Span::styled(
            format!("{arrow} {:.2}%", q.change_pct.abs()),
            Style::new().fg(color),
        ),
    ];
    let lo = q.spark.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = q.spark.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let right = if q.spark.is_empty() {
        Vec::new()
    } else {
        vec![label(format!("lo {}  hi {}", price(lo), price(hi)))]
    };
    row(left, right, width)
}

fn atmos(frame: &mut Frame, area: Rect, app: &App) {
    let Some(Reading::Weather(w)) = app.readings.get(&SourceId::Weather) else {
        return;
    };
    let (deg, speed) = match w.units {
        Units::Metric => ("°C", "km/h"),
        Units::Imperial => ("°F", "mph"),
    };
    let [top, _, bottom] = Layout::vertical([
        Constraint::Length(9),
        Constraint::Length(1),
        Constraint::Min(4),
    ])
    .areas(area);

    let big = bigtext::render(&format!("{:.0}", w.temp));
    let big_width = big[0].chars().count() as u16 + 4;
    let [left, details] =
        Layout::horizontal([Constraint::Length(big_width.max(12)), Constraint::Min(20)]).areas(top);
    let mut big_lines: Vec<Line> = big
        .iter()
        .map(|r| Line::styled(r.clone(), Style::new().fg(theme::CYAN)))
        .collect();
    big_lines[0].spans.push(Span::styled(
        format!(" {deg}"),
        Style::new().fg(theme::MUTED),
    ));
    big_lines.push(Line::default());
    big_lines.push(Line::styled(
        if w.is_day { "DAY CYCLE" } else { "NIGHT CYCLE" },
        Style::new().fg(theme::MUTED),
    ));
    frame.render_widget(Paragraph::new(big_lines), left);

    let field = |name: &str, spans: Vec<Span<'static>>| {
        let mut line = vec![label(format!("{name:<10}"))];
        line.extend(spans);
        Line::from(line)
    };
    let mut lines = vec![
        field(
            "SKY",
            vec![Span::styled(
                lexicon::weather(w.code),
                Style::new().fg(theme::YELLOW),
            )],
        ),
        field("FEELS", vec![value(format!("{:.0}{deg}", w.feels_like))]),
        field("HUMIDITY", vec![value(format!("{:.0}%", w.humidity))]),
        field(
            "WIND",
            vec![value(format!(
                "{:.0} {speed} from {} {}",
                w.wind_speed,
                geo::compass(w.wind_from),
                geo::arrow(w.wind_from + 180.0)
            ))],
        ),
        field(
            "PRECIP",
            vec![value(format!("{:.0}% this hour", w.precip_prob))],
        ),
    ];
    if let Some(aqi) = w.us_aqi {
        let color = aqi_color(aqi);
        lines.push(field(
            "SMOG",
            vec![
                Span::styled(
                    format!("AQI {aqi:.0} {:<10}", lexicon::aqi(aqi)),
                    Style::new().fg(color),
                ),
                Span::styled(bar(aqi / 300.0, 20), Style::new().fg(color)),
            ],
        ));
    }
    if let Some(uv) = w.uv_index {
        lines.push(field(
            "UV",
            vec![
                value(format!("{uv:<3.0} {:<10}", lexicon::uv(uv))),
                Span::styled(bar(uv / 11.0, 20), Style::new().fg(theme::YELLOW)),
            ],
        ));
    }
    frame.render_widget(Paragraph::new(lines), details);

    if w.next_24h.is_empty() {
        return;
    }
    let lo = w.next_24h.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = w.next_24h.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let [title, graph, labels] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(bottom);
    frame.render_widget(
        Paragraph::new(header(format!("NEXT 24H // {lo:.0}–{hi:.0}{deg}"))),
        title,
    );
    chart(frame, graph, &w.next_24h, None, theme::CYAN);
    frame.render_widget(
        Paragraph::new(axis(
            &["now", "+6h", "+12h", "+18h", "+24h"],
            labels.width as usize,
        )),
        labels,
    );
}

fn intercepts(frame: &mut Frame, area: Rect, app: &App, now: DateTime<Utc>) {
    let (kev_area, hn_area) = if area.width >= 110 {
        let [a, _, b] = Layout::horizontal([
            Constraint::Percentage(48),
            Constraint::Length(2),
            Constraint::Min(20),
        ])
        .areas(area);
        (a, b)
    } else {
        let [a, b] = Layout::vertical([Constraint::Percentage(45), Constraint::Min(4)]).areas(area);
        (a, b)
    };
    let width = kev_area.width as usize;
    let mut kev = vec![header("CISA KEV // KNOWN EXPLOITED"), Line::default()];
    match app.readings.get(&SourceId::Kev) {
        Some(Reading::Kev(vulns)) => {
            let today = now.with_timezone(&Local).date_naive();
            for v in vulns {
                let days = (today - v.added).num_days();
                let when = if days <= 0 {
                    "today".to_string()
                } else {
                    format!("{days}d ago")
                };
                let mut right = Vec::new();
                if v.ransomware {
                    right.push(Span::styled("RANSOMWARE ", Style::new().fg(theme::RED)));
                }
                right.push(label(when));
                let left = vec![
                    Span::styled(format!("{} ", v.cve), Style::new().fg(theme::MAGENTA)),
                    value(fit(
                        &format!("{} {}", v.vendor, v.product),
                        width.saturating_sub(width_of(&right) + 18),
                    )),
                ];
                kev.push(row(left, right, width));
                if !v.name.is_empty() {
                    kev.push(Line::styled(
                        format!("  {}", fit(&v.name, width - 2)),
                        Style::new().fg(theme::MUTED),
                    ));
                }
            }
        }
        _ if app.sources.contains_key(&SourceId::Kev) => kev.push(awaiting(app, SourceId::Kev)),
        _ => {}
    }
    frame.render_widget(Paragraph::new(kev), kev_area);

    let width = hn_area.width as usize;
    let mut hn = vec![header("HACKER NEWS // FRONT PAGE"), Line::default()];
    match app.readings.get(&SourceId::Hn) {
        Some(Reading::Hn(stories)) => {
            for (rank, s) in stories.iter().enumerate() {
                hn.push(Line::from(vec![
                    Span::styled(format!("{:>2}. ", rank + 1), Style::new().fg(theme::CYAN)),
                    value(fit(&s.title, width.saturating_sub(4))),
                ]));
                hn.push(Line::styled(
                    format!(
                        "    {}▲  {} comments  {} ago",
                        s.score,
                        s.comments,
                        ago(s.posted, now)
                    ),
                    Style::new().fg(theme::MUTED),
                ));
            }
        }
        _ if app.sources.contains_key(&SourceId::Hn) => hn.push(awaiting(app, SourceId::Hn)),
        _ => {}
    }
    frame.render_widget(Paragraph::new(hn), hn_area);
}

/// Radar on the left, the list on the right.
fn scope_split(area: Rect) -> (Rect, Rect) {
    let scope_width = (area.height * 2).min(area.width * 11 / 20);
    let [scope, _, list] = Layout::horizontal([
        Constraint::Length(scope_width),
        Constraint::Length(2),
        Constraint::Min(20),
    ])
    .areas(area);
    (scope, list)
}

fn sweep() -> f64 {
    radar::sweep_at(Utc::now().timestamp_millis())
}

fn seismic(frame: &mut Frame, area: Rect, app: &App, now: DateTime<Utc>) {
    let Some(Reading::Quakes(quakes)) = app.readings.get(&SourceId::Quakes) else {
        return;
    };
    let sector = &app.config.sector;
    let (scope, list) = scope_split(area);
    // Only what's on the scope competes for the name tags, strongest first.
    let placed: Vec<&Quake> = quakes
        .iter()
        .filter(|q| q.bearing.is_some() && q.distance_km.is_some_and(|d| d <= sector.radius_km))
        .collect();
    let mut by_mag: Vec<usize> = (0..placed.len()).collect();
    by_mag.sort_by(|&a, &b| placed[b].mag.total_cmp(&placed[a].mag));
    let tagged: HashSet<usize> = by_mag.into_iter().take(RADAR_LABELS).collect();
    let blips: Vec<radar::Blip> = placed
        .iter()
        .enumerate()
        .map(|(i, q)| radar::Blip {
            distance_km: q.distance_km.unwrap_or_default(),
            bearing: q.bearing.unwrap_or_default(),
            glyph: quake_glyph(q.mag).into(),
            color: blip_color(q.mag),
            label: tagged.contains(&i).then(|| format!("M{:.1}", q.mag)),
        })
        .collect();
    let note = (sector.fix().is_none() && blips.is_empty()).then_some(lexicon::NO_FIX_RADAR);
    radar::draw(
        frame,
        scope,
        sector.radius_km,
        &blips,
        sweep(),
        sector.units,
        note,
    );

    let width = list.width as usize;
    let mut lines = vec![
        header(format!(
            "{} EVENTS // NEARBY 7 DAYS · M4.5+ WORLDWIDE 24H",
            quakes.len()
        )),
        Line::default(),
    ];
    for q in quakes {
        let near = q.distance_km.is_some_and(|d| d <= sector.radius_km);
        let place = match (near, q.distance_km, q.bearing) {
            (true, Some(d), Some(b)) => format!(
                "{} {} · {}",
                distance(d, sector.units),
                geo::compass(b),
                q.place
            ),
            _ => q.place.clone(),
        };
        let mut right = vec![label(format!(
            "{:>4.0}km deep  {:>3} ago",
            q.depth_km,
            ago(q.time, now)
        ))];
        if q.tsunami {
            right.insert(0, Span::styled("TSUNAMI ", Style::new().fg(theme::RED)));
        }
        let mag = Span::styled(
            format!("M{:<4.1}", q.mag),
            Style::new()
                .fg(mag_color(q.mag))
                .add_modifier(Modifier::BOLD),
        );
        let room = width.saturating_sub(6 + width_of(&right) + 1);
        let place_color = if near { theme::TEXT } else { theme::MUTED };
        lines.push(row(
            vec![
                mag,
                Span::styled(fit(&place, room), Style::new().fg(place_color)),
            ],
            right,
            width,
        ));
    }
    frame.render_widget(Paragraph::new(lines), list);
}

fn quake_glyph(mag: f64) -> &'static str {
    match mag {
        m if m < 2.0 => "·",
        m if m < 3.0 => "•",
        m if m < 4.0 => "●",
        _ => "◉",
    }
}

/// Muted is too dark for a blip on the scope; small quakes get plain text color.
fn blip_color(mag: f64) -> Color {
    match mag_color(mag) {
        c if c == theme::MUTED => theme::TEXT,
        c => c,
    }
}

fn helios(frame: &mut Frame, area: Rect, app: &App) {
    let Some(Reading::Swpc(s)) = app.readings.get(&SourceId::Swpc) else {
        return;
    };
    let color = kp_color(s.kp);
    let [top, _, middle, labels, _, bottom] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(4),
    ])
    .areas(area);

    let big = bigtext::render(&format!("{:.1}", s.kp));
    let big_width = big[0].chars().count() as u16 + 3;
    let [left, right] =
        Layout::horizontal([Constraint::Length(big_width), Constraint::Min(20)]).areas(top);
    frame.render_widget(
        Paragraph::new(
            big.iter()
                .map(|r| Line::styled(r.clone(), Style::new().fg(color)))
                .collect::<Vec<_>>(),
        ),
        left,
    );
    let gauge_width = right.width.saturating_sub(2) as usize;
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                label("PLANETARY Kp  "),
                Span::styled(
                    lexicon::kp(s.kp),
                    Style::new().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::default(),
            Line::styled(bar(s.kp / 9.0, gauge_width), Style::new().fg(color)),
            // Placed at their Kp values: storms start at 5 (G1), G3 at 7.
            axis_at(
                &[(0.0, "0"), (4.0, "4"), (5.0, "G1"), (7.0, "G3"), (9.0, "9")],
                9.0,
                gauge_width,
            ),
        ]),
        right,
    );

    frame.render_widget(
        Paragraph::new(header("Kp // LAST 72H (3-HOURLY)")),
        Rect {
            height: 1,
            ..middle
        },
    );
    let graph = Rect {
        y: middle.y + 1,
        height: middle.height.saturating_sub(1),
        ..middle
    };
    chart(frame, graph, &s.kp_history, Some((0.0, 9.0)), theme::CYAN);
    frame.render_widget(
        Paragraph::new(axis(
            &["-72h", "-48h", "-24h", "now"],
            labels.width as usize,
        )),
        labels,
    );

    let scale = |letter: char, level: u8, name: &str| {
        let fg = match level {
            0 => theme::MUTED,
            1 | 2 => theme::YELLOW,
            _ => theme::MAGENTA,
        };
        Line::from(vec![
            Span::styled(
                format!("{letter}{level} "),
                Style::new().fg(fg).add_modifier(Modifier::BOLD),
            ),
            label(format!("{name:<20}")),
            Span::styled(lexicon::noaa_scale(level), Style::new().fg(fg)),
        ])
    };
    let xray = s.xray_flux.map_or_else(
        || "—".to_string(),
        |f| format!("{}  ({f:.2e} W/m²)", xray_class(f)),
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![label("X-RAY FLUX  "), value(xray)]),
            scale('G', s.scales.g, "geomagnetic storm"),
            scale('S', s.scales.s, "solar radiation"),
            scale('R', s.scales.r, "radio blackout"),
        ]),
        bottom,
    );
}

fn sky(frame: &mut Frame, area: Rect, app: &App) {
    let Some(Reading::OpenSky(contacts)) = app.readings.get(&SourceId::OpenSky) else {
        return;
    };
    let sector = &app.config.sector;
    let (scope, list) = scope_split(area);
    let blips: Vec<radar::Blip> = contacts
        .iter()
        .enumerate()
        .map(|(i, c)| radar::Blip {
            distance_km: c.distance_km,
            bearing: c.bearing,
            glyph: c.heading.map_or('•', geo::arrow).to_string(),
            color: altitude_color(c.altitude_m),
            label: (i < RADAR_LABELS).then(|| c.callsign.clone()),
        })
        .collect();
    radar::draw(
        frame,
        scope,
        sector.flight_radius_km,
        &blips,
        sweep(),
        sector.units,
        None,
    );

    let width = list.width as usize;
    let mut lines = vec![
        header(format!(
            "{} CONTACTS // {}",
            contacts.len(),
            distance(sector.flight_radius_km, sector.units)
        )),
        Line::from(vec![
            Span::styled("■ ", Style::new().fg(theme::YELLOW)),
            label("below FL100  "),
            Span::styled("■ ", Style::new().fg(theme::CYAN)),
            label("to FL260  "),
            Span::styled("■ ", Style::new().fg(theme::GHOST)),
            label("above"),
        ]),
        Line::default(),
    ];
    for c in contacts {
        let flight_level = c.altitude_m.map_or_else(
            || "FL---".to_string(),
            |m| format!("FL{:03.0}", (m / 30.48).max(0.0)),
        );
        let knots = c.speed_ms.map_or_else(
            || "  -kt".to_string(),
            |v| format!("{:>3.0}kt", v * 1.943_84),
        );
        let heading = c.heading.map_or_else(
            || "  ·   ".to_string(),
            |h| format!("{} {:03.0}°", geo::arrow(h), h),
        );
        let left = vec![
            Span::styled(
                format!("{:<9}", fit(&c.callsign, 8)),
                Style::new().fg(altitude_color(c.altitude_m)),
            ),
            value(format!("{flight_level}  {heading}  {knots}")),
        ];
        let right = vec![label(format!(
            "{} {}",
            distance(c.distance_km, sector.units),
            geo::compass(c.bearing)
        ))];
        lines.push(row(left, right, width));
    }
    frame.render_widget(Paragraph::new(lines), list);
}

fn altitude_color(altitude_m: Option<f64>) -> Color {
    match altitude_m {
        Some(m) if m < 3_048.0 => theme::YELLOW,
        Some(m) if m < 7_925.0 => theme::CYAN,
        Some(_) => theme::GHOST,
        None => theme::MUTED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_labels_sit_at_their_values() {
        let line = axis_at(&[(0.0, "0"), (5.0, "G1"), (9.0, "9")], 9.0, 19).to_string();
        assert_eq!(line.find('0'), Some(0));
        assert_eq!(line.find("G1"), Some(10));
        assert_eq!(line.rfind('9'), Some(18));
    }
}
