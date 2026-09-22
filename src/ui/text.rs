//! Small text-shaping helpers shared by every node.

use chrono::{DateTime, Utc};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::Units;

const TICKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Truncates `s` to `max` display columns, ending in `…` if anything was cut.
pub fn fit(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(ch);
        used += w;
    }
    if max > 0 {
        out.push('…');
    }
    out
}

/// A sparkline of the whole series, averaged down to at most `width` cells. With
/// `range`, values are placed on that fixed scale; otherwise the scale stretches to the
/// values shown.
pub fn spark(values: &[f64], width: usize, range: Option<(f64, f64)>) -> String {
    let points = resample(values, width);
    let (lo, hi) = range.unwrap_or_else(|| {
        points
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            })
    });
    points
        .iter()
        .map(|&v| {
            let t = if hi > lo {
                ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
            } else {
                0.5
            };
            TICKS[(t * 7.0).round() as usize]
        })
        .collect()
}

/// Exactly `width` points: bucket means for a long series, repeated values for a short
/// one (so a 24-hour forecast fills a wide chart).
pub fn fit_series(values: &[f64], width: usize) -> Vec<f64> {
    if values.is_empty() || values.len() >= width {
        return resample(values, width);
    }
    (0..width)
        .map(|i| values[i * values.len() / width])
        .collect()
}

/// Bucket means, so a long series fits `width` cells without dropping its start.
fn resample(values: &[f64], width: usize) -> Vec<f64> {
    if values.len() <= width {
        return values.to_vec();
    }
    (0..width)
        .map(|i| {
            let bucket = &values[i * values.len() / width..(i + 1) * values.len() / width];
            bucket.iter().sum::<f64>() / bucket.len() as f64
        })
        .collect()
}

/// A filled/empty bar `width` cells wide.
pub fn bar(frac: f64, width: usize) -> String {
    let filled = (frac.clamp(0.0, 1.0) * width as f64).round() as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// Distance in the configured units: `38km` or `24mi`.
pub fn distance(km: f64, units: Units) -> String {
    match units {
        Units::Metric => format!("{km:.0}km"),
        Units::Imperial => format!("{:.0}mi", km * 0.621_371),
    }
}

/// Compact age: `42s`, `7m`, `3h`, `2d`.
pub fn ago(t: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - t).num_seconds().max(0);
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3_600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3_600),
        s => format!("{}d", s / 86_400),
    }
}

/// Price with precision that suits its size: `61,204`, `3,100.52`, `182.40`, `0.0712`.
pub fn price(p: f64) -> String {
    if p >= 10_000.0 {
        thousands(p.round() as u64)
    } else if p >= 1_000.0 {
        let whole = p.trunc() as u64;
        let cents = ((p - p.trunc()) * 100.0).round() as u64;
        // Rounding 0.995 up carries into the whole part.
        let (whole, cents) = if cents == 100 {
            (whole + 1, 0)
        } else {
            (whole, cents)
        };
        format!("{}.{cents:02}", thousands(whole))
    } else if p >= 1.0 {
        format!("{p:.2}")
    } else {
        format!("{p:.4}")
    }
}

fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub fn width_of(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

/// `left`, then `right` pushed against the far edge. If they don't both fit, `right`
/// is dropped.
pub fn row(mut left: Vec<Span<'static>>, right: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let used = width_of(&left) + width_of(&right);
    if used < width {
        left.push(Span::raw(" ".repeat(width - used)));
        left.extend(right);
    }
    Line::from(left)
}

pub fn spinner() -> char {
    SPINNER[(Utc::now().timestamp_millis() / 100).rem_euclid(10) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_truncates_with_ellipsis() {
        assert_eq!(fit("short", 10), "short");
        assert_eq!(fit("exactly10!", 10), "exactly10!");
        assert_eq!(fit("this is too long", 8), "this is…");
        assert_eq!(fit("abc", 0), "");
    }

    #[test]
    fn spark_scales_to_values_or_fixed_range() {
        assert_eq!(spark(&[1.0, 2.0, 3.0], 10, None), "▁▅█");
        assert_eq!(spark(&[1.0, 2.0, 3.0], 2, None), "▁█");
        assert_eq!(spark(&[5.0, 5.0], 5, None), "▅▅");
        assert_eq!(spark(&[0.0, 9.0], 5, Some((0.0, 9.0))), "▁█");
        assert_eq!(spark(&[], 5, None), "");
        // A long series keeps its shape: rising then falling, squeezed to 4 cells.
        let series: Vec<f64> = (0..100).chain((0..100).rev()).map(f64::from).collect();
        assert_eq!(spark(&series, 4, None), "▁██▁");
    }

    #[test]
    fn prices() {
        assert_eq!(price(61_204.4), "61,204");
        assert_eq!(price(1_234_567.0), "1,234,567");
        assert_eq!(price(3_100.519), "3,100.52");
        assert_eq!(price(1_999.996), "2,000.00");
        assert_eq!(price(182.4), "182.40");
        assert_eq!(price(0.07123), "0.0712");
    }

    #[test]
    fn ages() {
        let now = Utc::now();
        assert_eq!(ago(now - chrono::Duration::seconds(42), now), "42s");
        assert_eq!(ago(now - chrono::Duration::minutes(7), now), "7m");
        assert_eq!(ago(now - chrono::Duration::hours(3), now), "3h");
        assert_eq!(ago(now - chrono::Duration::days(2), now), "2d");
        assert_eq!(ago(now + chrono::Duration::seconds(5), now), "0s");
    }

    #[test]
    fn row_right_aligns() {
        let line = row(vec![Span::raw("ab")], vec![Span::raw("cd")], 8);
        assert_eq!(line.to_string(), "ab    cd");
        let line = row(vec![Span::raw("abcdef")], vec![Span::raw("gh")], 7);
        assert_eq!(line.to_string(), "abcdef");
    }

    #[test]
    fn series_fit_the_width_either_way() {
        assert_eq!(fit_series(&[1.0, 2.0], 4), [1.0, 1.0, 2.0, 2.0]);
        assert_eq!(fit_series(&[1.0, 3.0, 5.0, 7.0], 2), [2.0, 6.0]);
        assert!(fit_series(&[], 3).is_empty());
    }

    #[test]
    fn distances() {
        assert_eq!(distance(38.4, Units::Metric), "38km");
        assert_eq!(distance(100.0, Units::Imperial), "62mi");
    }

    #[test]
    fn bars() {
        assert_eq!(bar(0.5, 4), "██░░");
        assert_eq!(bar(2.0, 3), "███");
    }
}
