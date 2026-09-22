//! Construct mode: synthetic feeds with no network. Prices random-walk, weather
//! drifts, quakes scatter, and ICE / TRACE are injected now and then so every link
//! state shows up. All names and headlines here are made up.

use std::f64::consts::TAU;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{Days, Utc};
use fastrand::Rng;

use super::{Feed, FetchError};
use crate::config::{Config, Units};
use crate::geo;
use crate::reading::{
    Contact, PrecipHour, Quake, Quote, Reading, Satellite, Scales, SpaceWeather, Story, Vuln,
    Weather,
};
use crate::source::SourceId;

const SPARK_LEN: usize = 48;
const ICE_CHANCE: f64 = 0.05;
const TRACE_CHANCE: f64 = 0.015;

const ICE: &[&str] = &[
    "connection reset by peer",
    "TLS handshake stalled",
    "upstream returned HTTP 502",
    "payload failed to parse",
    "DNS lookup timed out",
];

const HN_TITLES: &[&str] = &[
    "Show HN: A terminal dashboard that thinks it's 2077",
    "Why your database is slower on Tuesdays",
    "How GPS spoofing works, with diagrams",
    "Rewriting our billing system in Rust: one year later",
    "Ask HN: What's the most over-engineered thing you've built?",
    "A 20-year-old bug in a common compression library",
    "Understanding memory ordering from first principles",
    "Show HN: An offline-first note app in one HTML file",
    "The economics of undersea cables",
    "How one DNS record took down a city's transit signs",
    "Launch HN: Privacy-preserving analytics for small sites",
    "Writing keyboard firmware from scratch",
    "Why every map app gets my street wrong",
    "A visual guide to attention in transformers",
    "The last pay phone in the city still works",
];

const KEV_TARGETS: &[(&str, &str)] = &[
    ("Ivory Systems", "EdgeGate VPN"),
    ("Northwind", "FileRelay"),
    ("Helix Networks", "RouterOS"),
    ("Parallax", "MailCore"),
    ("Quanta Labs", "Build Agent"),
    ("Sable", "Hypervisor"),
];

const KEV_FLAWS: &[&str] = &[
    "Command Injection Vulnerability",
    "Authentication Bypass Vulnerability",
    "Path Traversal Vulnerability",
    "Use-After-Free Vulnerability",
    "Deserialization of Untrusted Data Vulnerability",
];

const FAR_PLACES: &[&str] = &[
    "Tonga Islands",
    "Kuril Islands",
    "off the coast of Chile",
    "Alaska Peninsula",
    "Banda Sea",
    "Vanuatu",
    "Hindu Kush, Afghanistan",
    "Mid-Atlantic Ridge",
    "south of the Fiji Islands",
    "Kermadec Islands",
];

const AIRLINES: &[&str] = &[
    "UAL", "DAL", "AAL", "SWA", "JBU", "ASA", "FFT", "BAW", "DLH", "AFR", "KLM", "UAE",
];

pub struct DemoFeed {
    source: SourceId,
    interval: Duration,
    stocks: Vec<String>,
    coins: Vec<String>,
    units: Units,
    radius_km: f64,
    flight_radius_km: f64,
    last: Mutex<Option<Reading>>,
}

impl DemoFeed {
    pub fn new(source: SourceId, config: &Config) -> Self {
        Self {
            source,
            interval: Duration::from_millis(fastrand::u64(5_000..11_000)),
            stocks: config.zaibatsu.stocks.clone(),
            coins: config.zaibatsu.coins.clone(),
            units: config.sector.units,
            radius_km: config.sector.radius_km,
            flight_radius_km: config.sector.flight_radius_km,
            last: Mutex::new(None),
        }
    }

    pub(crate) fn evolve(&self, prev: Option<&Reading>, rng: &mut Rng) -> Reading {
        match (self.source, prev) {
            (SourceId::Stocks, prev) => {
                let prev = match prev {
                    Some(Reading::Stocks(q)) => Some(q.as_slice()),
                    _ => None,
                };
                Reading::Stocks(quotes(prev, &self.stocks, false, rng))
            }
            (SourceId::Crypto, prev) => {
                let prev = match prev {
                    Some(Reading::Crypto(q)) => Some(q.as_slice()),
                    _ => None,
                };
                Reading::Crypto(quotes(prev, &self.coins, true, rng))
            }
            (SourceId::Weather, Some(Reading::Weather(w))) => {
                Reading::Weather(weather(w.clone(), rng))
            }
            (SourceId::Weather, _) => Reading::Weather(weather(seed_weather(self.units), rng)),
            (SourceId::Hn, Some(Reading::Hn(s))) => Reading::Hn(hn(s.clone(), rng)),
            (SourceId::Hn, _) => Reading::Hn(hn(Vec::new(), rng)),
            (SourceId::Kev, Some(Reading::Kev(v))) => Reading::Kev(kev(v.clone(), rng)),
            (SourceId::Kev, _) => Reading::Kev(kev(Vec::new(), rng)),
            (SourceId::Quakes, Some(Reading::Quakes(q))) => {
                Reading::Quakes(quakes(q.clone(), self.radius_km, rng))
            }
            (SourceId::Quakes, _) => Reading::Quakes(quakes(Vec::new(), self.radius_km, rng)),
            (SourceId::Swpc, Some(Reading::Swpc(s))) => Reading::Swpc(swpc(s.clone(), rng)),
            (SourceId::Swpc, _) => Reading::Swpc(swpc(seed_swpc(), rng)),
            (SourceId::OpenSky, Some(Reading::OpenSky(c))) => {
                Reading::OpenSky(sky(c.clone(), self.flight_radius_km, rng))
            }
            (SourceId::OpenSky, _) => Reading::OpenSky(sky(Vec::new(), self.flight_radius_km, rng)),
            (SourceId::Orbit, Some(Reading::Orbit(s))) => {
                Reading::Orbit(orbit(s.first().cloned(), rng))
            }
            (SourceId::Orbit, _) => Reading::Orbit(orbit(None, rng)),
        }
    }
}

impl Feed for DemoFeed {
    fn source(&self) -> SourceId {
        self.source
    }

    fn interval(&self) -> Duration {
        self.interval
    }

    fn retry_base(&self) -> Duration {
        Duration::from_secs(3)
    }

    async fn fetch(&self, _http: &reqwest::Client) -> Result<Reading, FetchError> {
        tokio::time::sleep(Duration::from_millis(fastrand::u64(250..1_500))).await;
        let mut rng = Rng::new();
        let roll = rng.f64();
        if roll < ICE_CHANCE {
            return Err(FetchError::Failed(pick(&mut rng, ICE).to_string()));
        }
        if roll < ICE_CHANCE + TRACE_CHANCE {
            return Err(FetchError::RateLimited(Some(Duration::from_secs(20))));
        }
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let next = self.evolve(last.as_ref(), &mut rng);
        *last = Some(next.clone());
        Ok(next)
    }
}

fn pick<'a, T>(rng: &mut Rng, items: &'a [T]) -> &'a T {
    &items[rng.usize(..items.len())]
}

/// Uniform in `-1.0..1.0`.
fn jitter(rng: &mut Rng) -> f64 {
    rng.f64() * 2.0 - 1.0
}

fn push_capped(values: &mut Vec<f64>, value: f64, cap: usize) {
    values.push(value);
    if values.len() > cap {
        values.drain(..values.len() - cap);
    }
}

fn quotes(prev: Option<&[Quote]>, ids: &[String], crypto: bool, rng: &mut Rng) -> Vec<Quote> {
    let volatility = if crypto { 0.008 } else { 0.004 };
    ids.iter()
        .map(|id| {
            let symbol = if crypto {
                coin_symbol(id)
            } else {
                id.to_uppercase()
            };
            let mut q = prev
                .and_then(|p| p.iter().find(|q| q.symbol == symbol))
                .cloned()
                .unwrap_or_else(|| seed_quote(symbol, id, rng));
            let open = q.price / (1.0 + q.change_pct / 100.0);
            q.price *= 1.0 + jitter(rng) * volatility;
            q.change_pct = (q.price / open - 1.0) * 100.0;
            push_capped(&mut q.spark, q.price, SPARK_LEN);
            q
        })
        .collect()
}

fn seed_quote(symbol: String, id: &str, rng: &mut Rng) -> Quote {
    let base = match id {
        "bitcoin" => 61_000.0,
        "ethereum" => 3_100.0,
        "solana" => 140.0,
        _ => 20.0 + (fnv(id) % 480) as f64,
    };
    let mut price = base;
    let spark = (0..SPARK_LEN)
        .map(|_| {
            price *= 1.0 + jitter(rng) * 0.006;
            price
        })
        .collect();
    Quote {
        symbol,
        price,
        change_pct: jitter(rng) * 3.0,
        spark,
    }
}

fn coin_symbol(id: &str) -> String {
    let known = match id {
        "bitcoin" => "BTC",
        "ethereum" => "ETH",
        "solana" => "SOL",
        "dogecoin" => "DOGE",
        "cardano" => "ADA",
        "ripple" => "XRP",
        _ => return id.chars().take(4).collect::<String>().to_uppercase(),
    };
    known.to_string()
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}

fn seed_weather(units: Units) -> Weather {
    let imperial = units == Units::Imperial;
    let temp = if imperial { 63.0 } else { 17.0 };
    let swing = if imperial { 7.0 } else { 4.0 };
    Weather {
        units,
        temp,
        feels_like: temp - 1.5,
        humidity: 64.0,
        precip_prob: 20.0,
        wind_speed: if imperial { 6.0 } else { 9.0 },
        wind_from: 225.0,
        code: 2,
        is_day: true,
        us_aqi: Some(42.0),
        uv_index: Some(3.0),
        next_24h: (0..24)
            .map(|h| temp + swing * (f64::from(h) / 24.0 * TAU).sin())
            .collect(),
        precip_next: (1..=4)
            .map(|h| PrecipHour {
                prob: (20.0 + f64::from(h) * 12.0).min(90.0),
                mm: f64::from(h) * 0.15,
            })
            .collect(),
    }
}

fn weather(mut w: Weather, rng: &mut Rng) -> Weather {
    w.temp += jitter(rng) * 0.3;
    w.feels_like = w.temp - 1.5 + rng.f64();
    w.humidity = (w.humidity + jitter(rng) * 2.0).clamp(20.0, 100.0);
    w.precip_prob = (w.precip_prob + jitter(rng) * 5.0).clamp(0.0, 100.0);
    w.wind_speed = (w.wind_speed + jitter(rng)).max(0.0);
    w.wind_from = (w.wind_from + jitter(rng) * 10.0).rem_euclid(360.0);
    if rng.f64() < 0.1 {
        w.code = *pick(rng, &[0, 1, 2, 3, 45, 61, 63, 80, 95]);
    }
    w.us_aqi = w.us_aqi.map(|a| (a + jitter(rng) * 4.0).clamp(5.0, 220.0));
    w.uv_index = w.uv_index.map(|u| (u + jitter(rng) * 0.3).clamp(0.0, 11.0));
    w.next_24h.rotate_left(1);
    for hour in &mut w.precip_next {
        hour.prob = (hour.prob + jitter(rng) * 5.0).clamp(0.0, 100.0);
        hour.mm = (hour.mm + jitter(rng) * 0.1).max(0.0);
    }
    w
}

fn hn(mut stories: Vec<Story>, rng: &mut Rng) -> Vec<Story> {
    let now = Utc::now();
    if stories.is_empty() {
        for _ in 0..10 {
            let posted = now - chrono::Duration::minutes(rng.i64(10..600));
            let score = rng.u32(20..600);
            let story = new_story(&stories, rng, posted, score);
            stories.push(story);
        }
    }
    for s in &mut stories {
        s.score += rng.u32(0..12);
        s.comments += rng.u32(0..4);
    }
    if rng.f64() < 0.25 {
        stories.sort_by_key(|s| s.score);
        stories.remove(0);
        let score = rng.u32(5..40);
        let story = new_story(&stories, rng, now, score);
        stories.push(story);
    }
    stories.sort_by_key(|s| std::cmp::Reverse(s.score));
    stories
}

fn new_story(
    existing: &[Story],
    rng: &mut Rng,
    posted: chrono::DateTime<Utc>,
    score: u32,
) -> Story {
    let unused: Vec<&str> = HN_TITLES
        .iter()
        .copied()
        .filter(|t| !existing.iter().any(|s| s.title == *t))
        .collect();
    let title = if unused.is_empty() {
        pick(rng, HN_TITLES)
    } else {
        pick(rng, &unused)
    };
    Story {
        id: rng.u64(30_000_000..40_000_000),
        title: title.to_string(),
        score,
        comments: rng.u32(0..300),
        posted,
    }
}

fn kev(mut vulns: Vec<Vuln>, rng: &mut Rng) -> Vec<Vuln> {
    let today = chrono::Local::now().date_naive();
    if vulns.is_empty() {
        vulns = (0..4)
            .map(|i| {
                let added = today
                    .checked_sub_days(Days::new(i * 2 + 1))
                    .unwrap_or(today);
                new_vuln(rng, added)
            })
            .collect();
    }
    if rng.f64() < 0.08 {
        vulns.insert(0, new_vuln(rng, today));
        vulns.truncate(8);
    }
    vulns
}

fn new_vuln(rng: &mut Rng, added: chrono::NaiveDate) -> Vuln {
    let (vendor, product) = pick(rng, KEV_TARGETS);
    Vuln {
        cve: format!("CVE-2026-{}", rng.u32(10_000..60_000)),
        vendor: vendor.to_string(),
        product: product.to_string(),
        name: format!("{vendor} {product} {}", pick(rng, KEV_FLAWS)),
        added,
        ransomware: rng.f64() < 0.2,
    }
}

fn quakes(mut quakes: Vec<Quake>, radius_km: f64, rng: &mut Rng) -> Vec<Quake> {
    let now = Utc::now();
    if quakes.is_empty() {
        quakes = (0..6)
            .map(|_| {
                let time = now - chrono::Duration::minutes(rng.i64(5..720));
                new_quake(rng, radius_km, time)
            })
            .collect();
    }
    if rng.f64() < 0.3 {
        quakes.push(new_quake(rng, radius_km, now));
    }
    quakes.sort_by_key(|q| std::cmp::Reverse(q.time));
    quakes.truncate(12);
    quakes
}

fn new_quake(rng: &mut Rng, radius_km: f64, time: chrono::DateTime<Utc>) -> Quake {
    let id = format!("demo{}", rng.u32(..));
    if rng.f64() < 0.7 {
        let distance = rng.f64() * radius_km;
        let bearing = rng.f64() * 360.0;
        let mag = if rng.f64() < 0.05 {
            4.0 + rng.f64() * 0.8
        } else {
            0.8 + rng.f64() * 2.8
        };
        Quake {
            id,
            mag,
            place: format!("{distance:.0} km {} of sector", geo::compass(bearing)),
            time,
            depth_km: rng.f64() * 15.0,
            distance_km: Some(distance),
            bearing: Some(bearing),
            tsunami: false,
        }
    } else {
        let mag = 4.5 + rng.f64().powi(2) * 2.8;
        Quake {
            id,
            mag,
            place: pick(rng, FAR_PLACES).to_string(),
            time,
            depth_km: 10.0 + rng.f64() * 200.0,
            distance_km: Some(4_000.0 + rng.f64() * 12_000.0),
            bearing: Some(rng.f64() * 360.0),
            tsunami: mag > 6.5 && rng.bool(),
        }
    }
}

fn thirds(x: f64) -> f64 {
    (x * 3.0).round() / 3.0
}

fn seed_swpc() -> SpaceWeather {
    SpaceWeather {
        kp: 2.33,
        kp_history: (0..24)
            .map(|i| thirds(2.0 + (f64::from(i) / 4.0).sin()))
            .collect(),
        xray_flux: Some(2.0e-6),
        scales: Scales::default(),
    }
}

fn swpc(mut s: SpaceWeather, rng: &mut Rng) -> SpaceWeather {
    s.kp = thirds((s.kp + jitter(rng) * 0.7).clamp(0.0, 8.0));
    push_capped(&mut s.kp_history, s.kp, 24);
    s.xray_flux = s
        .xray_flux
        .map(|f| (f * (1.0 + jitter(rng) * 0.3)).clamp(1e-7, 2e-4));
    s.scales = Scales {
        g: storm_level(s.kp),
        s: 0,
        r: s.xray_flux.map_or(0, radio_blackout_level),
    };
    s
}

/// NOAA G-scale: G1 at Kp 5 up to G5 at Kp 9.
fn storm_level(kp: f64) -> u8 {
    if kp >= 5.0 {
        ((kp - 4.0).floor() as u8).min(5)
    } else {
        0
    }
}

/// NOAA R-scale from peak X-ray flux: R1 at M1, R2 at M5, R3 at X1, R4 at X10, R5 at X20.
fn radio_blackout_level(flux: f64) -> u8 {
    [1e-5, 5e-5, 1e-4, 1e-3, 2e-3]
        .iter()
        .filter(|&&threshold| flux >= threshold)
        .count() as u8
}

fn sky(mut contacts: Vec<Contact>, radius_km: f64, rng: &mut Rng) -> Vec<Contact> {
    if contacts.is_empty() {
        contacts = (0..rng.usize(4..10))
            .map(|_| new_contact(rng, radius_km))
            .collect();
    }
    for c in &mut contacts {
        c.distance_km = (c.distance_km + jitter(rng) * 8.0).clamp(1.0, radius_km);
        c.bearing = (c.bearing + jitter(rng) * 3.0).rem_euclid(360.0);
    }
    if rng.f64() < 0.2 && contacts.len() > 2 {
        contacts.remove(rng.usize(..contacts.len()));
    }
    if rng.f64() < 0.25 && contacts.len() < 14 {
        contacts.push(new_contact(rng, radius_km));
    }
    contacts.sort_by(|a, b| a.distance_km.total_cmp(&b.distance_km));
    contacts
}

fn orbit(prev: Option<Satellite>, rng: &mut Rng) -> Vec<Satellite> {
    let mut s = prev.unwrap_or_else(|| Satellite {
        name: "ISS".into(),
        altitude_km: 417.0,
        velocity_kmh: 27_600.0,
        sunlit: true,
        distance_km: rng.f64() * 3_000.0,
        bearing: rng.f64() * 360.0,
        elevation_deg: 0.0,
    });
    s.altitude_km = (s.altitude_km + jitter(rng) * 2.0).clamp(408.0, 425.0);
    s.velocity_kmh = (s.velocity_kmh + jitter(rng) * 20.0).clamp(27_500.0, 27_700.0);
    // The ISS crosses a whole horizon-to-horizon pass in a few minutes.
    s.distance_km = (s.distance_km + jitter(rng) * 350.0).rem_euclid(3_000.0);
    s.bearing = (s.bearing + jitter(rng) * 15.0).rem_euclid(360.0);
    s.elevation_deg = geo::elevation_deg(s.distance_km, s.altitude_km);
    if rng.f64() < 0.05 {
        s.sunlit = !s.sunlit;
    }
    vec![s]
}

fn new_contact(rng: &mut Rng, radius_km: f64) -> Contact {
    Contact {
        callsign: format!("{}{}", pick(rng, AIRLINES), rng.u32(10..3000)),
        altitude_m: Some(1_000.0 + rng.f64() * 11_000.0),
        speed_ms: Some(120.0 + rng.f64() * 140.0),
        heading: Some(rng.f64() * 360.0),
        distance_km: rng.f64() * radius_km,
        bearing: rng.f64() * 360.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noaa_levels() {
        assert_eq!(storm_level(4.67), 0);
        assert_eq!(storm_level(5.0), 1);
        assert_eq!(storm_level(9.0), 5);
        assert_eq!(radio_blackout_level(5e-6), 0);
        assert_eq!(radio_blackout_level(1e-5), 1);
        assert_eq!(radio_blackout_level(2e-4), 3);
    }

    #[test]
    fn every_source_evolves_from_nothing_and_from_itself() {
        let config = Config::default();
        let mut rng = Rng::with_seed(7);
        for source in SourceId::ALL {
            let feed = DemoFeed::new(source, &config);
            let first = feed.evolve(None, &mut rng);
            let _second = feed.evolve(Some(&first), &mut rng);
        }
    }
}
