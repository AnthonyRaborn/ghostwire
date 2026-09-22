//! A dive: one node takes over the grid with everything it knows.

use std::collections::HashSet;
use std::time::Instant;

use chrono::{DateTime, Local, Timelike, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, BorderType, Chart, Dataset, GraphType, Paragraph};

use super::nodes::{
    aqi_color, awaiting, kp_color, label, mag_color, outage_color, status, uplink_color, value,
};
use super::text::{ago, bar, distance, fit, price, row, width_of, wrap};
use super::{bigtext, globe, radar};
use crate::app::App;
use crate::config::Units;
use crate::fx::Fx;
use crate::reading::{LinkHealth, OutageAlert, Quake, Quote, Reading, Satellite, Weather, xray_class};
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
            Style::new()
                .fg(theme::title_color(link.as_ref()))
                .add_modifier(Modifier::BOLD),
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
        NodeId::Sky => sky(frame, body, app),
        NodeId::Netstatus => netstatus(frame, body, app, now),
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

/// A section header with a muted attribution tag naming where the data comes from —
/// only where the header text itself doesn't already say it (KEV, IODA, NOAA SWPC).
fn header_src(text: impl Into<String>, source: &str) -> Line<'static> {
    let mut line = header(text);
    line.spans.push(Span::styled(
        format!("  {source}"),
        Style::new().fg(theme::MUTED),
    ));
    line
}

/// A continuous braille trace of `values` across the full width of `area` — an
/// oscilloscope read rather than a bar chart, since these are all one signal over time.
fn chart(frame: &mut Frame, area: Rect, values: &[f64], range: Option<(f64, f64)>, color: Color) {
    if area.is_empty() || values.len() < 2 {
        return;
    }
    let (lo, hi) = range.unwrap_or_else(|| {
        values
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            })
    });
    // A flat series still needs distinct bounds so the trace doesn't divide by zero.
    let (lo, hi) = if hi > lo { (lo, hi) } else { (lo - 1.0, hi + 1.0) };
    let points: Vec<(f64, f64)> = values.iter().enumerate().map(|(i, &v)| (i as f64, v)).collect();
    let dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::new().fg(color))
        .data(&points);
    frame.render_widget(
        Chart::new(vec![dataset])
            .x_axis(Axis::default().bounds([0.0, (values.len() - 1) as f64]))
            .y_axis(Axis::default().bounds([lo, hi])),
        area,
    );
}

/// Several braille traces sharing one plot, each independently min-max normalized to
/// the same 0..1 scale — their real units may differ wildly (a temperature and a
/// percentage), so overlaying their raw values would flatten whichever has the smaller
/// range. The caller's legend carries the actual ranges; this only needs to keep the
/// *shapes* comparable.
fn overlay_chart(frame: &mut Frame, area: Rect, series: &[(&[f64], Color)]) {
    if area.is_empty() {
        return;
    }
    let normalized: Vec<Vec<(f64, f64)>> = series
        .iter()
        .filter(|(values, _)| values.len() >= 2)
        .map(|(values, _)| {
            let (lo, hi) = values
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
                    (lo.min(v), hi.max(v))
                });
            let (lo, hi) = if hi > lo { (lo, hi) } else { (lo - 1.0, hi + 1.0) };
            values
                .iter()
                .enumerate()
                .map(|(i, &v)| (i as f64, (v - lo) / (hi - lo)))
                .collect()
        })
        .collect();
    if normalized.is_empty() {
        return;
    }
    let longest = normalized.iter().map(Vec::len).max().unwrap_or(0);
    let datasets: Vec<Dataset> = normalized
        .iter()
        .zip(series.iter().filter(|(values, _)| values.len() >= 2))
        .map(|(points, (_, color))| {
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(*color))
                .data(points)
        })
        .collect();
    frame.render_widget(
        Chart::new(datasets)
            .x_axis(Axis::default().bounds([0.0, (longest - 1) as f64]))
            .y_axis(Axis::default().bounds([0.0, 1.0])),
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
    let sections: Vec<(SourceId, &str, &str)> = [
        (SourceId::Stocks, "EQUITIES // SESSION", "finnhub"),
        (SourceId::Crypto, "CRYPTO // 7 DAYS", "coingecko"),
    ]
    .into_iter()
    .filter(|(id, _, _)| app.sources.contains_key(id))
    .collect();
    let quotes = |id: SourceId| match app.readings.get(&id) {
        Some(Reading::Stocks(q) | Reading::Crypto(q)) => q.as_slice(),
        _ => &[],
    };
    let count: u16 = sections
        .iter()
        .map(|(id, _, _)| quotes(*id).len().max(1) as u16)
        .sum();
    let spare = area
        .height
        .saturating_sub(count + sections.len() as u16 * 2);
    let chart_rows = (spare / count.max(1)).clamp(0, 4);

    let mut y = area.y;
    let bottom = area.bottom();
    for (id, title, source) in sections {
        if y >= bottom {
            break;
        }
        frame.render_widget(
            Paragraph::new(header_src(title, source)),
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
    // The precip nowcast (a compact radar-style scope) sits in the upper right,
    // alongside the current-conditions fields, once there's width for all three
    // columns; the forecast chart below always gets the full row regardless.
    let show_nowcast = area.width >= 100 && !w.precip_next.is_empty();
    let top_height = if show_nowcast { 13 } else { 9 };
    let [top, _, bottom] = Layout::vertical([
        Constraint::Length(top_height),
        Constraint::Length(1),
        Constraint::Min(4),
    ])
    .areas(area);

    let big = bigtext::render(&format!("{:.0}", w.temp));
    let big_width = big[0].chars().count() as u16 + 4;
    let (left, details, nowcast_area) = if show_nowcast {
        let [left, details, nowcast_area] = Layout::horizontal([
            Constraint::Length(big_width.max(12)),
            Constraint::Min(30),
            Constraint::Length(34),
        ])
        .areas(top);
        (left, details, Some(nowcast_area))
    } else {
        let [left, details] =
            Layout::horizontal([Constraint::Length(big_width.max(12)), Constraint::Min(20)])
                .areas(top);
        (left, details, None)
    };
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
    let wind_max = match w.units {
        Units::Metric => 80.0,
        Units::Imperial => 50.0,
    };
    let wind_text = format!(
        "{:.0} {speed} from {} {}",
        w.wind_speed,
        geo::compass(w.wind_from),
        geo::arrow(w.wind_from + 180.0)
    );
    // Fields that always have a value, alongside the two meters that read best next to
    // the big temp digit.
    let left_fields = vec![
        field(
            "SKY",
            vec![Span::styled(
                lexicon::weather(w.code),
                Style::new().fg(theme::YELLOW),
            )],
        ),
        field("FEELS", vec![value(format!("{:.0}{deg}", w.feels_like))]),
        field(
            "HUMIDITY",
            vec![
                value(format!("{:.0}%{:<7}", w.humidity, "")),
                Span::styled(bar(w.humidity / 100.0, 20), Style::new().fg(theme::CYAN)),
            ],
        ),
        field(
            "WIND",
            vec![
                value(format!("{wind_text:<22}")),
                Span::styled(
                    bar(w.wind_speed / wind_max, 20),
                    Style::new().fg(theme::CYAN),
                ),
            ],
        ),
    ];
    // The rest of the meters — optional ones land here too, so the second column only
    // appears when there's real content for it.
    let mut right_fields = vec![field(
        "PRECIP",
        vec![
            value(format!("{:.0}%{:<7}", w.precip_prob, "")),
            Span::styled(bar(w.precip_prob / 100.0, 20), Style::new().fg(theme::CYAN)),
        ],
    )];
    if let Some(aqi) = w.us_aqi {
        let color = aqi_color(aqi);
        right_fields.push(field(
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
        right_fields.push(field(
            "UV",
            vec![
                value(format!("{uv:<3.0} {:<10}", lexicon::uv(uv))),
                Span::styled(bar(uv / 11.0, 20), Style::new().fg(theme::YELLOW)),
            ],
        ));
    }
    let mut lines = left_fields;
    lines.extend(right_fields);
    frame.render_widget(Paragraph::new(lines), details);

    if let Some(nowcast_area) = nowcast_area {
        precip_nowcast(frame, nowcast_area, w);
    }

    if w.next_24h.is_empty() {
        return;
    }
    // The forecast chart overlays temp with whatever other hourly series are
    // available, each independently normalized so their shapes stay comparable
    // however different their real units are — the legend carries the actual ranges.
    let minmax = |v: &[f64]| {
        (
            v.iter().copied().fold(f64::INFINITY, f64::min),
            v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    };
    let mut series: Vec<(&[f64], Color)> = vec![(&w.next_24h, theme::CYAN)];
    let (lo_t, hi_t) = minmax(&w.next_24h);
    let mut legend = vec![Span::styled(
        format!("TEMP {lo_t:.0}–{hi_t:.0}{deg}"),
        Style::new().fg(theme::CYAN),
    )];
    if !w.humidity_24h.is_empty() {
        series.push((&w.humidity_24h, theme::GREEN));
        let (lo_h, hi_h) = minmax(&w.humidity_24h);
        legend.push(label(" · "));
        legend.push(Span::styled(
            format!("HUMIDITY {lo_h:.0}–{hi_h:.0}%"),
            Style::new().fg(theme::GREEN),
        ));
    }
    if !w.precip_prob_24h.is_empty() {
        series.push((&w.precip_prob_24h, theme::YELLOW));
        let (lo_p, hi_p) = minmax(&w.precip_prob_24h);
        legend.push(label(" · "));
        legend.push(Span::styled(
            format!("RAIN {lo_p:.0}–{hi_p:.0}%"),
            Style::new().fg(theme::YELLOW),
        ));
    }

    let [title, graph, labels] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(bottom);
    let mut title_spans = vec![Span::styled(
        "NEXT 24H // ",
        Style::new().fg(theme::MAGENTA).add_modifier(Modifier::BOLD),
    )];
    title_spans.extend(legend);
    frame.render_widget(
        Paragraph::new(row(title_spans, vec![label("open-meteo")], title.width as usize)),
        title,
    );
    overlay_chart(frame, graph, &series);
    frame.render_widget(
        Paragraph::new(axis(
            &["now", "+6h", "+12h", "+18h", "+24h"],
            labels.width as usize,
        )),
        labels,
    );
}

/// Precipitation drifting in with the wind: bearing is the direction it's coming from
/// (the surface wind reading), radius is hours until it arrives. Real hourly data, an
/// interpretive layout — there's no spatial radar feed behind ATMOS, just a forecast.
fn precip_nowcast(frame: &mut Frame, area: Rect, w: &Weather) {
    // A legend line rather than on-scope labels: every hour sits on the same bearing
    // (the wind), so labels next to each point would stack on top of one another.
    let [title, legend, scope] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(6),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(header_src(
            format!("PRECIP NOWCAST // NEXT {}H", w.precip_next.len()),
            "open-meteo",
        )),
        title,
    );
    let mut legend_spans = Vec::new();
    for (i, p) in w.precip_next.iter().enumerate() {
        if i > 0 {
            legend_spans.push(label(" · "));
        }
        legend_spans.push(Span::styled(
            format!("+{}h {:.0}% {:.1}mm", i + 1, p.prob, p.mm),
            Style::new().fg(precip_color(p.prob)),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(legend_spans)), legend);

    let hours = w.precip_next.len() as f64;
    // A filled wedge per hour, all on the wind bearing (still the one honest spatial
    // fact available), widening with chance of rain so "how much" reads as a bigger
    // patch of color, not just a bigger dot. Drawn farthest-hour-first, so they stack
    // into concentric bands rather than one hour's ring hiding another's.
    let wedges: Vec<radar::Wedge> = w
        .precip_next
        .iter()
        .enumerate()
        .filter(|(_, p)| p.prob >= 10.0)
        .map(|(i, p)| radar::Wedge {
            r: i as f64 + 1.0,
            bearing: w.wind_from,
            half_width_deg: 6.0 + (p.prob / 100.0) * 34.0,
            color: precip_color(p.prob),
            label: Some(format!("+{}h", i + 1)),
        })
        .collect();
    let note = wedges.is_empty().then_some("NOTHING INCOMING");
    radar::draw(
        frame,
        scope,
        &radar::Scope {
            range: hours,
            blips: &[],
            wedges: &wedges,
            sweep: sweep(),
            range_label: &format!("+{hours:.0}h"),
            empty_note: note,
        },
    );
}

fn precip_color(prob: f64) -> Color {
    match prob {
        p if p < 20.0 => theme::MUTED,
        p if p < 50.0 => theme::CYAN,
        p if p < 75.0 => theme::YELLOW,
        _ => theme::MAGENTA,
    }
}

/// Splits `area` into `n` equal, evenly gapped sections — side by side when there's
/// room, stacked otherwise. Used so a configured RSS feed gets a third column without
/// a separate layout to hand-maintain alongside KEV and HN's.
fn intercept_columns(area: Rect, n: usize) -> Vec<Rect> {
    let n = n.max(1) as u32;
    let mut constraints = Vec::with_capacity(n as usize * 2 - 1);
    for i in 0..n {
        if i > 0 {
            constraints.push(Constraint::Length(2));
        }
        constraints.push(Constraint::Ratio(1, n));
    }
    let split = if area.width >= 110 {
        Layout::horizontal(constraints).split(area)
    } else {
        Layout::vertical(constraints).split(area)
    };
    split.iter().step_by(2).copied().collect()
}

fn intercepts(frame: &mut Frame, area: Rect, app: &App, now: DateTime<Utc>) {
    let show_rss = app.sources.contains_key(&SourceId::Rss);
    let mut columns = intercept_columns(area, if show_rss { 3 } else { 2 }).into_iter();
    let kev_area = columns.next().unwrap_or_default();
    let hn_area = columns.next().unwrap_or_default();
    let width = kev_area.width as usize;
    let mut kev = vec![
        header_src("CISA KEV // KNOWN EXPLOITED", "cisa.gov"),
        Line::default(),
    ];
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
                for line in wrap(&v.name, width.saturating_sub(2), 2) {
                    kev.push(Line::styled(
                        format!("  {line}"),
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
    let mut hn = vec![
        header_src("HACKER NEWS // FRONT PAGE", "firebase"),
        Line::default(),
    ];
    match app.readings.get(&SourceId::Hn) {
        Some(Reading::Hn(stories)) => {
            for (rank, s) in stories.iter().enumerate() {
                for (i, line) in wrap(&s.title, width.saturating_sub(4), 2).into_iter().enumerate() {
                    if i == 0 {
                        hn.push(Line::from(vec![
                            Span::styled(
                                format!("{:>2}. ", rank + 1),
                                Style::new().fg(theme::CYAN),
                            ),
                            value(line),
                        ]));
                    } else {
                        hn.push(Line::from(vec![Span::raw("    "), value(line)]));
                    }
                }
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

    if !show_rss {
        return;
    }
    let rss_area = columns.next().unwrap_or_default();
    let width = rss_area.width as usize;
    let mut rss = vec![header("RSS // INTERCEPTED FEEDS"), Line::default()];
    // Width of the "{source} " column that titles indent under on wrapped lines.
    const RSS_TITLE_COL: usize = 17;
    match app.readings.get(&SourceId::Rss) {
        Some(Reading::Rss(headlines)) => {
            for h in headlines {
                let title_lines = wrap(&h.title, width.saturating_sub(RSS_TITLE_COL), 2);
                for (i, line) in title_lines.into_iter().enumerate() {
                    if i == 0 {
                        rss.push(Line::from(vec![
                            Span::styled(
                                format!("{:<16} ", fit(&h.source, 15)),
                                Style::new().fg(theme::GREEN),
                            ),
                            value(line),
                        ]));
                    } else {
                        rss.push(Line::from(vec![
                            Span::raw(" ".repeat(RSS_TITLE_COL)),
                            value(line),
                        ]));
                    }
                }
                if let Some(t) = h.published {
                    rss.push(Line::styled(
                        format!("                 {} ago", ago(t, now)),
                        Style::new().fg(theme::MUTED),
                    ));
                }
            }
        }
        _ => rss.push(awaiting(app, SourceId::Rss)),
    }
    frame.render_widget(Paragraph::new(rss), rss_area);
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

/// Both feeds are low-density enough to share a dive: quakes get the bulk of the
/// height (a radar scope needs room), space weather gets a compact strip below.
fn seismic(frame: &mut Frame, area: Rect, app: &App, now: DateTime<Utc>) {
    let has_quakes = app.readings.contains_key(&SourceId::Quakes);
    let has_solar = app.readings.contains_key(&SourceId::Swpc);
    if has_quakes && has_solar {
        // Space weather's big Kp digit and chart need a fixed floor to stay legible;
        // quakes get whatever's left (a radar scope degrades gracefully when squeezed).
        let [quakes_area, header_area, solar_area] = Layout::vertical([
            Constraint::Min(10),
            Constraint::Length(1),
            Constraint::Length(16),
        ])
        .areas(area);
        quakes_detail(frame, quakes_area, app, now);
        frame.render_widget(
            Paragraph::new(header("SPACE WEATHER // NOAA SWPC")),
            header_area,
        );
        solar_detail(frame, solar_area, app);
    } else if has_quakes {
        quakes_detail(frame, area, app, now);
    } else if has_solar {
        solar_detail(frame, area, app);
    }
}

fn quakes_detail(frame: &mut Frame, area: Rect, app: &App, now: DateTime<Utc>) {
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
            r: q.distance_km.unwrap_or_default(),
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
        &radar::Scope {
            range: sector.radius_km,
            blips: &blips,
            wedges: &[],
            sweep: sweep(),
            range_label: &distance(sector.radius_km, sector.units),
            empty_note: note,
        },
    );

    let width = list.width as usize;
    let mut lines = vec![
        header_src(
            format!(
                "{} EVENTS // NEARBY 7 DAYS · M4.5+ WORLDWIDE 24H",
                quakes.len()
            ),
            "usgs",
        ),
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

fn solar_detail(frame: &mut Frame, area: Rect, app: &App) {
    let Some(Reading::Swpc(s)) = app.readings.get(&SourceId::Swpc) else {
        return;
    };
    let color = kp_color(s.kp);
    // A narrow left column for the numbers — the gauge only needs to read at a glance,
    // not span the whole dive — frees the rest of the width for a taller, full-height
    // Kp chart on the right (the same meta-column/big-visual split as ATMOS and the
    // quake radar/list).
    let [meta, _, chart_col] = Layout::horizontal([
        Constraint::Length(44),
        Constraint::Length(2),
        Constraint::Min(20),
    ])
    .areas(area);

    let [top, _, bottom] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Min(4),
    ])
    .areas(meta);

    let big = bigtext::render(&format!("{:.1}", s.kp));
    let big_width = big[0].chars().count() as u16 + 3;
    let [left, right] =
        Layout::horizontal([Constraint::Length(big_width), Constraint::Min(10)]).areas(top);
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

    let [chart_header, chart_graph, chart_labels] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(chart_col);
    frame.render_widget(
        Paragraph::new(header("Kp // LAST 72H (3-HOURLY)")),
        chart_header,
    );
    chart(frame, chart_graph, &s.kp_history, Some((0.0, 9.0)), theme::CYAN);
    frame.render_widget(
        Paragraph::new(axis(
            &["-72h", "-48h", "-24h", "now"],
            chart_labels.width as usize,
        )),
        chart_labels,
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
            r: c.distance_km,
            bearing: c.bearing,
            glyph: c.heading.map_or('•', geo::arrow).to_string(),
            color: altitude_color(c.altitude_m),
            label: (i < RADAR_LABELS).then(|| c.callsign.clone()),
        })
        .collect();
    radar::draw(
        frame,
        scope,
        &radar::Scope {
            range: sector.flight_radius_km,
            blips: &blips,
            wedges: &[],
            sweep: sweep(),
            range_label: &distance(sector.flight_radius_km, sector.units),
            empty_note: None,
        },
    );

    let width = list.width as usize;
    let mut lines = vec![
        header_src(
            format!(
                "{} CONTACTS // {}",
                contacts.len(),
                distance(sector.flight_radius_km, sector.units)
            ),
            "opensky",
        ),
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
    if let Some(Reading::Orbit(sats)) = app.readings.get(&SourceId::Orbit)
        && let Some(s) = sats.first()
    {
        lines.push(Line::default());
        lines.push(orbit_line(s, sector, width));
    }
    frame.render_widget(Paragraph::new(lines), list);
}

/// A single detail line for the ISS — wheretheiss.at only tracks the one object, so
/// there's no list to speak of, just its current numbers.
fn orbit_line(s: &Satellite, sector: &crate::config::Sector, width: usize) -> Line<'static> {
    let visible = s.elevation_deg > 0.0;
    let color = if visible { theme::CYAN } else { theme::MUTED };
    let left = vec![
        Span::styled(format!("{:<9}", fit(&s.name, 8)), Style::new().fg(color)),
        value(format!(
            "{:.0}km alt  {:.0}km/h  {}",
            s.altitude_km,
            s.velocity_kmh,
            if s.sunlit { "SUNLIT" } else { "SHADOW" }
        )),
    ];
    let right = vec![Span::styled(
        if visible {
            format!(
                "↑{:.0}° {} {}",
                s.elevation_deg,
                geo::compass(s.bearing),
                distance(s.distance_km, sector.units)
            )
        } else {
            "below horizon".to_string()
        },
        Style::new().fg(color),
    )];
    row(left, right, width)
}

fn altitude_color(altitude_m: Option<f64>) -> Color {
    match altitude_m {
        Some(m) if m < 3_048.0 => theme::YELLOW,
        Some(m) if m < 7_925.0 => theme::CYAN,
        Some(_) => theme::GHOST,
        None => theme::MUTED,
    }
}

fn netstatus(frame: &mut Frame, area: Rect, app: &App, now: DateTime<Utc>) {
    let [left, _, right] = Layout::horizontal([
        Constraint::Percentage(42),
        Constraint::Length(2),
        Constraint::Min(30),
    ])
    .areas(area);

    match app.readings.get(&SourceId::Uplink) {
        Some(Reading::Uplink(link)) => uplink_detail(frame, left, link),
        _ if app.sources.contains_key(&SourceId::Uplink) => {
            frame.render_widget(Paragraph::new(awaiting(app, SourceId::Uplink)), left);
        }
        _ => {}
    }

    let width = right.width as usize;
    let mut lines = vec![header("IODA // COUNTRY OUTAGES"), Line::default()];
    match app.readings.get(&SourceId::Ioda) {
        Some(Reading::Ioda(outages)) => {
            lines.push(Line::from(vec![
                label("SECTOR "),
                value(outages.country.clone()),
            ]));
            lines.push(Line::default());
            if outages.alerts.is_empty() {
                lines.push(Line::styled(
                    "no alerts in the last 3h",
                    Style::new().fg(theme::GREEN),
                ));
            } else {
                for a in &outages.alerts {
                    let color = outage_color(&a.level);
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{:<8} ", a.level.to_uppercase()),
                            Style::new().fg(color).add_modifier(Modifier::BOLD),
                        ),
                        value(a.datasource.clone()),
                        label(format!("   {} ago", ago(a.time, now))),
                    ]));
                }
            }
            // A derived read on the country's overall link health — IODA only gives
            // discrete alert events, not a continuous signal, so this stands in for one:
            // full when nothing's alerting, docked per active alert.
            lines.push(Line::default());
            let integrity = country_integrity(&outages.alerts);
            let color = integrity_color(integrity);
            let gauge_width = width.saturating_sub(20).clamp(10, 30);
            lines.push(Line::from(vec![
                label("LINK INTEGRITY  "),
                Span::styled(
                    format!("{integrity:>3.0}% "),
                    Style::new().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(bar(integrity / 100.0, gauge_width), Style::new().fg(color)),
            ]));
        }
        _ if app.sources.contains_key(&SourceId::Ioda) => lines.push(awaiting(app, SourceId::Ioda)),
        _ => {}
    }
    let text_height = lines.len() as u16;
    let [text_area, _, globe_area] = Layout::vertical([
        Constraint::Length(text_height),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(right);
    frame.render_widget(Paragraph::new(lines), text_area);

    // Whatever's left under the alerts — usually most of the column, since IODA is
    // quiet more often than not. No feed behind this one: it's the sun's position,
    // computed locally, same as the radar sweep.
    if globe_area.height >= 6 && globe_area.width >= 20 {
        let [globe_header, globe_body] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(globe_area);
        frame.render_widget(
            Paragraph::new(header(format!("TERMINATOR // {:02}Z", now.hour()))),
            globe_header,
        );
        let globe_lines = globe::render(
            globe_body.width as usize,
            globe_body.height as usize,
            now,
            app.config.sector.fix(),
        );
        frame.render_widget(Paragraph::new(globe_lines), globe_body);
    }
}

/// A rough 0-100 read on the country's link health: full when nothing's alerting,
/// docked more for a critical alert than a warning.
fn country_integrity(alerts: &[OutageAlert]) -> f64 {
    let penalty: f64 = alerts
        .iter()
        .map(|a| if a.level == "critical" { 35.0 } else { 15.0 })
        .sum();
    (100.0 - penalty).max(0.0)
}

fn integrity_color(pct: f64) -> Color {
    match pct {
        p if p >= 90.0 => theme::GREEN,
        p if p >= 60.0 => theme::YELLOW,
        _ => theme::MAGENTA,
    }
}

fn uplink_detail(frame: &mut Frame, area: Rect, link: &LinkHealth) {
    let color = uplink_color(link.latency_ms);
    let [top, _, chart_area] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Min(4),
    ])
    .areas(area);

    let big = bigtext::render(&format!("{:.0}", link.latency_ms));
    let big_width = big[0].chars().count() as u16 + 4;
    let [big_area, details] =
        Layout::horizontal([Constraint::Length(big_width.max(12)), Constraint::Min(14)]).areas(top);
    let mut big_lines: Vec<Line> = big
        .iter()
        .map(|r| Line::styled(r.clone(), Style::new().fg(color)))
        .collect();
    big_lines[0]
        .spans
        .push(Span::styled(" ms", Style::new().fg(theme::MUTED)));
    frame.render_widget(Paragraph::new(big_lines), big_area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![label("EDGE "), value(link.colo.clone())]),
            Line::from(vec![label("VIA "), value("cloudflare trace")]),
        ]),
        details,
    );

    frame.render_widget(
        Paragraph::new(header("LATENCY // RECENT")),
        Rect {
            height: 1,
            ..chart_area
        },
    );
    let graph = Rect {
        y: chart_area.y + 1,
        height: chart_area.height.saturating_sub(1),
        ..chart_area
    };
    if link.history.len() > 1 {
        chart(frame, graph, &link.history, None, color);
    } else {
        frame.render_widget(
            Paragraph::new(Line::styled(
                lexicon::SAMPLING_LATENCY,
                Style::new().fg(theme::MUTED),
            )),
            graph,
        );
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

    #[test]
    fn country_integrity_docks_more_for_critical_than_warning() {
        let alert = |level: &str| OutageAlert {
            datasource: "bgp".into(),
            level: level.into(),
            time: Utc::now(),
        };
        assert_eq!(country_integrity(&[]), 100.0);
        assert_eq!(country_integrity(&[alert("warning")]), 85.0);
        assert_eq!(country_integrity(&[alert("critical")]), 65.0);
        // Never goes negative even with a pile of critical alerts.
        let pile = vec![alert("critical"); 5];
        assert_eq!(country_integrity(&pile), 0.0);
    }
}
