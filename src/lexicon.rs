//! Every in-fiction string lives here, so the rig's voice stays consistent.

use crate::source::{Link, NodeId};

pub const RIG: &str = "GHOSTWIRE";
pub const RIG_ID: &str = "RIG-07";
pub const CONSTRUCT: &str = "CONSTRUCT // SIMULATED FEEDS";
pub const KEYS: &str = "[r] re-breach  [q] jack out";
pub const NOMINAL: &str = "» all links nominal";
pub const NOT_WIRED: &str = "NO UPLINK // node not wired yet";
pub const NO_CONFIG: &str = "NO CONFIG // run ghostwire --init-config";
pub const NO_FINNHUB_KEY: &str = "no key: add finnhub to keys.toml";
pub const KEYS_EXPOSED: &str = "KEYS EXPOSED // chmod 600 keys.toml";

pub fn node_title(node: NodeId, sector: &str) -> String {
    match node {
        NodeId::Zaibatsu => "ZAIBATSU INDEX".into(),
        NodeId::Atmos => format!("ATMOS // {sector}"),
        NodeId::Intercepts => "INTERCEPTS".into(),
        NodeId::Seismic => "SEISMIC".into(),
        NodeId::Helios => "HELIOS".into(),
        NodeId::Sky => "SKYTRAFFIC".into(),
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

/// A footer-ticker entry for a source in trouble; `None` when it's fine.
pub fn trouble(handle: &str, link: &Link, retry_secs: Option<u64>) -> Option<String> {
    match link {
        Link::Ice => Some(format!(
            "{handle}: ICE{}",
            retry_suffix("retry", retry_secs)
        )),
        Link::Trace => Some(format!(
            "{handle}: TRACE ACTIVE{}",
            retry_suffix("dark", retry_secs)
        )),
        Link::Flatlined => Some(format!(
            "{handle}: FLATLINED{}",
            retry_suffix("retry", retry_secs)
        )),
        Link::Offline(reason) => Some(format!("{handle}: OFFLINE — {reason}")),
        Link::Live | Link::Ghost | Link::Pending => None,
    }
}

/// A node-body line for a source with nothing to show yet.
pub fn awaiting(handle: &str, link: &Link, busy: bool, retry_secs: Option<u64>) -> String {
    if busy {
        return format!("{handle} » breaching");
    }
    let secs = retry_secs.unwrap_or(0);
    match link {
        Link::Pending => format!("{handle} » handshake queued"),
        Link::Live | Link::Ghost => format!("{handle} » no signal"),
        Link::Ice => format!("{handle} !! ICE // retry {secs}s"),
        Link::Trace => format!("{handle} !! TRACE // dark {secs}s"),
        Link::Flatlined => format!("{handle} ×× FLATLINED // retry {secs}s"),
        Link::Offline(reason) => format!("{handle} OFFLINE // {reason}"),
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

pub fn ghosts(count: usize) -> String {
    match count {
        1 => "1 ghost cached".into(),
        n => format!("{n} ghosts cached"),
    }
}

pub fn too_small(min_w: u16, min_h: u16) -> String {
    format!("RIG DISPLAY TOO SMALL // need {min_w}×{min_h}")
}
