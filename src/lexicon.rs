//! Every in-fiction string lives here, so the rig's voice stays consistent.

use crate::source::{Link, NodeId};
use crate::sun::{Event, Light};
use crate::ui::text::fit;

/// The most an error takes up in the footer ticker, which every troubled source shares.
const TICKER_ERROR_WIDTH: usize = 32;

pub const RIG: &str = "GHOSTWIRE";
/// This release's codename — hacker slang for "day one," which v1.0.0 is.
pub const CODENAME: &str = "ZERO-DAY";
pub const CONSTRUCT: &str = "CONSTRUCT // SIMULATED FEEDS";
pub const KEYS_GRID: &str = "[1-6] dive  [space] next  [p] hold  [r] re-breach  [q] jack out";
pub const KEYS_DIVE: &str = "[esc] surface  [space] next  [p] hold  [q] jack out";
pub const JACKING_IN: &str = "JACKING IN";
pub const DIVE_HELD: &str = "» dive cycle held";
pub const NO_FIX_RADAR: &str = "NO SECTOR FIX";
/// Shown in place of the latency chart until the uplink has more than one sample.
pub const SAMPLING_LATENCY: &str = "sampling // history builds after the next few pings";
pub const NOMINAL: &str = "» all links nominal";
/// Shown when the config lists nothing for any of a node's sources.
pub const NOT_WIRED: &str = "NO UPLINK // no sources enabled in config";
pub const NO_CONFIG: &str = "NO CONFIG // run ghostwire --init-config";
pub const NO_FINNHUB_KEY: &str = "no key: add finnhub to keys.toml";
pub const KEYS_EXPOSED: &str = "KEYS EXPOSED // chmod 600 keys.toml";

pub fn node_title(node: NodeId, sector: &str) -> String {
    match node {
        NodeId::Zaibatsu => "ZAIBATSU INDEX".into(),
        NodeId::Atmos => format!("ATMOS // {sector}"),
        NodeId::Intercepts => "INTERCEPTS".into(),
        NodeId::Seismic => "SEISMIC // HELIOS".into(),
        NodeId::Sky => "SKYTRAFFIC".into(),
        NodeId::Netstatus => "NETSTATUS".into(),
    }
}

pub fn link_label(link: &Link) -> &'static str {
    match link {
        Link::Pending => "HANDSHAKE",
        Link::Live => "LIVE",
        Link::Ghost => "GHOST",
        Link::Ice => "ICE",
        Link::Trace => "TRACE",
        Link::Flatlined => "FLATLINED",
        Link::Offline(_) => "OFFLINE",
    }
}

fn retry_suffix(verb: &str, secs: Option<u64>) -> String {
    secs.map(|s| format!(", {verb} {s}s")).unwrap_or_default()
}

/// The most specific part of a fetch error: the last cause in the `: `-joined chain
/// the feeds report, minus any OS error code. The log keeps the whole chain.
pub fn error_brief(error: &str) -> &str {
    let last = error.rsplit(": ").next().unwrap_or(error).trim();
    match last.rfind(" (os error ") {
        Some(i) if last.ends_with(')') => last[..i].trim_end(),
        _ => last,
    }
}

/// A footer-ticker entry for a source in trouble; `None` when it's fine. `error` is
/// the last fetch failure, shown for ICE and FLATLINED.
pub fn trouble(
    handle: &str,
    link: &Link,
    retry_secs: Option<u64>,
    error: Option<&str>,
) -> Option<String> {
    let reason = || {
        error
            .map(|e| format!(" — {}", fit(error_brief(e), TICKER_ERROR_WIDTH)))
            .unwrap_or_default()
    };
    match link {
        Link::Ice => Some(format!(
            "{handle}: ICE{}{}",
            reason(),
            retry_suffix("retry", retry_secs)
        )),
        Link::Trace => Some(format!(
            "{handle}: TRACE ACTIVE{}",
            retry_suffix("dark", retry_secs)
        )),
        Link::Flatlined => Some(format!(
            "{handle}: FLATLINED{}{}",
            reason(),
            retry_suffix("retry", retry_secs)
        )),
        Link::Offline(reason) => Some(format!("{handle}: OFFLINE — {reason}")),
        Link::Live | Link::Ghost | Link::Pending => None,
    }
}

/// A node-body line for a source with nothing to show yet. `error` is the last fetch
/// failure, shown for ICE and FLATLINED.
pub fn awaiting(
    handle: &str,
    link: &Link,
    busy: bool,
    retry_secs: Option<u64>,
    error: Option<&str>,
) -> String {
    if busy {
        return format!("{handle} » breaching");
    }
    let secs = retry_secs.unwrap_or(0);
    let reason = || {
        error
            .map(|e| format!(" // {}", error_brief(e)))
            .unwrap_or_default()
    };
    match link {
        Link::Pending => format!("{handle} » handshake queued"),
        Link::Live | Link::Ghost => format!("{handle} » no signal"),
        Link::Ice => format!("{handle} !! ICE // retry {secs}s{}", reason()),
        Link::Trace => format!("{handle} !! TRACE // dark {secs}s"),
        Link::Flatlined => format!("{handle} ×× FLATLINED // retry {secs}s{}", reason()),
        Link::Offline(reason) => format!("{handle} OFFLINE // {reason}"),
    }
}

pub fn light(light: Light) -> &'static str {
    match light {
        Light::Day => "DAY",
        Light::Civil => "CIVIL TWILIGHT",
        Light::Nautical => "NAUTICAL TWILIGHT",
        Light::Astronomical => "ASTRO TWILIGHT",
        Light::Night => "NIGHT",
    }
}

/// The sector's next sunrise or sunset, counted down from `now`.
pub fn sun_event(event: Event, now: chrono::DateTime<chrono::Utc>) -> String {
    let left = |at: chrono::DateTime<chrono::Utc>| {
        let mins = (at - now).num_minutes().max(0);
        match mins {
            0 => "<1m".to_string(),
            m if m < 60 => format!("{m}m"),
            m => format!("{}h{:02}m", m / 60, m % 60),
        }
    };
    match event {
        Event::Rise(at) => format!("rises in {}", left(at)),
        Event::Set(at) => format!("sets in {}", left(at)),
        Event::StaysUp => "sun stays up".into(),
        Event::StaysDown => "sun stays down".into(),
    }
}

/// WMO weather interpretation code, as the rig describes it.
pub fn weather(code: u8) -> &'static str {
    match code {
        0 => "CLEAR",
        1 => "MOSTLY CLEAR",
        2 => "BROKEN CLOUD",
        3 => "CLOUD CANOPY",
        45 | 48 => "FOG BANK",
        51 | 53 | 55 => "DRIZZLE",
        56 | 57 => "FREEZING DRIZZLE",
        61 | 63 => "RAIN",
        65 => "HEAVY RAIN",
        66 | 67 => "FREEZING RAIN",
        71 | 73 | 75 | 77 => "SNOW",
        80 | 81 => "SQUALLS",
        82 => "VIOLENT SQUALLS",
        85 | 86 => "SNOW SQUALLS",
        95 => "ELECTRICAL STORM",
        96 | 99 => "ELECTRICAL STORM + HAIL",
        _ => "UNREADABLE SKY",
    }
}

/// US AQI band.
pub fn aqi(aqi: f64) -> &'static str {
    match aqi {
        a if a <= 50.0 => "CLEAN",
        a if a <= 100.0 => "MODERATE",
        a if a <= 150.0 => "IRRITANT",
        a if a <= 200.0 => "UNHEALTHY",
        a if a <= 300.0 => "TOXIC",
        _ => "HAZARDOUS",
    }
}

pub fn kp(kp: f64) -> &'static str {
    match kp {
        k if k < 4.0 => "QUIET",
        k if k < 5.0 => "UNSETTLED",
        _ => "STORM",
    }
}

/// Stands in for the handles of several sources sharing one ticker entry.
pub fn links(count: usize) -> String {
    format!("{count} LINKS")
}

/// Ticker entries that didn't fit.
pub fn more(count: usize) -> String {
    format!("+{count} more")
}

pub fn ghosts(count: usize) -> String {
    match count {
        1 => "1 ghost cached".into(),
        n => format!("{n} ghosts cached"),
    }
}

pub fn dive_title(node_title: &str) -> String {
    format!(" ◢ DIVE // {node_title} ◣ ")
}

pub fn diving_in(node_title: &str, secs: u64) -> String {
    format!("» diving {node_title} in {secs}s")
}

pub fn surfacing_in(secs: u64) -> String {
    format!("» surfacing in {secs}s")
}

pub fn intercept(node_title: &str, text: &str) -> String {
    format!("!! PRIORITY INTERCEPT // {node_title} // {text}")
}

/// NOAA's words for each level of its G, S, and R scales.
pub fn noaa_scale(level: u8) -> &'static str {
    match level {
        0 => "none",
        1 => "minor",
        2 => "moderate",
        3 => "strong",
        4 => "severe",
        _ => "extreme",
    }
}

/// WHO UV index band.
pub fn uv(uv: f64) -> &'static str {
    match uv {
        u if u < 3.0 => "LOW",
        u if u < 6.0 => "MODERATE",
        u if u < 8.0 => "HIGH",
        u if u < 11.0 => "VERY HIGH",
        _ => "EXTREME",
    }
}

pub fn too_small(min_w: u16, min_h: u16) -> String {
    format!("RIG DISPLAY TOO SMALL // need {min_w}×{min_h}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_brief_keeps_the_most_specific_cause() {
        let cases = [
            (
                "error sending request: client error (Connect): tcp connect error: \
                 Connection refused (os error 111)",
                "Connection refused",
            ),
            (
                "error sending request: client error (Connect): dns error: error resolving \
                 DNS: failed to lookup address information: Name or service not known",
                "Name or service not known",
            ),
            ("HTTP 503", "HTTP 503"),
            ("no response in 20s", "no response in 20s"),
            (
                "unexpected payload: missing field `c` at line 1 column 2",
                "missing field `c` at line 1 column 2",
            ),
        ];
        for (error, brief) in cases {
            assert_eq!(error_brief(error), brief);
        }
    }

    #[test]
    fn sun_events_count_down() {
        use chrono::{Duration, Utc};
        let now = Utc::now();
        assert_eq!(
            sun_event(Event::Set(now + Duration::minutes(192)), now),
            "sets in 3h12m"
        );
        assert_eq!(
            sun_event(Event::Rise(now + Duration::minutes(45)), now),
            "rises in 45m"
        );
        assert_eq!(
            sun_event(Event::Rise(now + Duration::seconds(20)), now),
            "rises in <1m"
        );
        assert_eq!(sun_event(Event::StaysDown, now), "sun stays down");
    }

    #[test]
    fn errors_show_only_for_failures() {
        let error = Some("tcp connect error: Connection refused (os error 111)");
        assert_eq!(
            trouble("USGS", &Link::Ice, Some(30), error).unwrap(),
            "USGS: ICE — Connection refused, retry 30s"
        );
        assert_eq!(
            trouble("USGS", &Link::Trace, Some(30), error).unwrap(),
            "USGS: TRACE ACTIVE, dark 30s"
        );
        assert_eq!(
            awaiting("USGS", &Link::Flatlined, false, Some(30), error),
            "USGS ×× FLATLINED // retry 30s // Connection refused"
        );
        let long = "x".repeat(80);
        let entry = trouble("USGS", &Link::Ice, None, Some(&long)).unwrap();
        assert_eq!(entry.chars().count(), "USGS: ICE — ".chars().count() + 32);
    }
}
