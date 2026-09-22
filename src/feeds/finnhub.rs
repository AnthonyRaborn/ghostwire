//! ZAIBATSU (equities): Finnhub quotes. Needs a free key (`finnhub` in keys.toml), sent as
//! a header so it never appears in a URL. The free tier has no intraday history, so the
//! sparkline is built from successive quotes and seeded from the cache across restarts.
//! Polls every minute while NYSE is open and every 15 minutes otherwise.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveTime, TimeZone, Utc, Weekday};
use chrono_tz::America::New_York;
use chrono_tz::Tz;
use futures_util::future::join_all;
use serde::Deserialize;

use super::http::{get_text_keyed, parse_json};
use super::{Feed, FetchError};
use crate::config::Config;
use crate::keys::Secret;
use crate::lexicon;
use crate::reading::{Quote, Reading};
use crate::source::SourceId;

const SPARK_LEN: usize = 48;
const OPEN_POLL: Duration = Duration::from_secs(60);
const CLOSED_POLL: Duration = Duration::from_secs(15 * 60);

pub struct Finnhub {
    symbols: Vec<String>,
    key: Option<Secret>,
    history: Mutex<HashMap<String, History>>,
}

#[derive(Default)]
struct History {
    spark: Vec<f64>,
    /// Timestamp of the last trade seen, so an idle market doesn't flatten the line.
    last_trade: i64,
}

impl Finnhub {
    /// `cached` is the last stocks reading from disk, used to seed the sparklines.
    pub fn new(config: &Config, key: Option<Secret>, cached: Option<&Reading>) -> Self {
        let mut history = HashMap::new();
        if let Some(Reading::Stocks(quotes)) = cached {
            for q in quotes {
                let seeded = History {
                    spark: q.spark.clone(),
                    last_trade: 0,
                };
                history.insert(q.symbol.clone(), seeded);
            }
        }
        let symbols = config
            .zaibatsu
            .stocks
            .iter()
            .map(|s| s.trim().to_uppercase())
            .filter(|s| {
                let ok = !s.is_empty()
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
                if !ok {
                    tracing::warn!("ignoring stock symbol {s:?}");
                }
                ok
            })
            .collect();
        Self {
            symbols,
            key,
            history: Mutex::new(history),
        }
    }
}

impl Feed for Finnhub {
    fn source(&self) -> SourceId {
        SourceId::Stocks
    }

    fn interval(&self) -> Duration {
        poll_interval(Utc::now())
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        let Some(key) = &self.key else {
            return Err(FetchError::NotConfigured(lexicon::NO_FINNHUB_KEY.into()));
        };
        let requests = self.symbols.iter().map(|symbol| {
            let url = format!("https://finnhub.io/api/v1/quote?symbol={symbol}");
            get_text_keyed(http.get(url).header("X-Finnhub-Token", key.expose()))
        });
        let responses = join_all(requests).await;

        let mut history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let mut quotes = Vec::new();
        let mut first_error = None;
        for (symbol, response) in self.symbols.iter().zip(responses) {
            match response.and_then(|text| parse(&text).map_err(FetchError::Failed)) {
                Ok(Some(raw)) => {
                    let h = history.entry(symbol.clone()).or_default();
                    if raw.t != h.last_trade {
                        h.spark.push(raw.c);
                        if h.spark.len() > SPARK_LEN {
                            h.spark.remove(0);
                        }
                        h.last_trade = raw.t;
                    }
                    quotes.push(Quote {
                        symbol: symbol.clone(),
                        price: raw.c,
                        change_pct: raw.dp.unwrap_or(0.0),
                        spark: h.spark.clone(),
                    });
                }
                Ok(None) => tracing::warn!("finnhub has no quote for {symbol}"),
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        match first_error {
            // A bad key or a rate limit applies to every symbol, not just one.
            Some(e @ (FetchError::NotConfigured(_) | FetchError::RateLimited(_))) => Err(e),
            Some(e) if quotes.is_empty() => Err(e),
            _ if quotes.is_empty() => Err(FetchError::NotConfigured(
                "no quotes for [zaibatsu] stocks".into(),
            )),
            _ => Ok(Reading::Stocks(quotes)),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RawQuote {
    /// Current price.
    c: f64,
    /// Percent change from the previous close.
    dp: Option<f64>,
    /// Last trade, seconds since the epoch.
    t: i64,
}

/// `None` when Finnhub doesn't know the symbol (it answers with all zeros).
pub fn parse(text: &str) -> Result<Option<RawQuote>, String> {
    let quote: RawQuote = parse_json(text)?;
    Ok((quote.t != 0 || quote.c != 0.0).then_some(quote))
}

fn is_trading_day(day: Weekday) -> bool {
    !matches!(day, Weekday::Sat | Weekday::Sun)
}

fn open_time() -> NaiveTime {
    NaiveTime::from_hms_opt(9, 30, 0).expect("valid time")
}

fn close_time() -> NaiveTime {
    NaiveTime::from_hms_opt(16, 0, 0).expect("valid time")
}

/// Regular NYSE hours. Exchange holidays aren't modeled; they just poll a little often.
fn is_open(et: DateTime<Tz>) -> bool {
    is_trading_day(et.weekday()) && (open_time()..close_time()).contains(&et.time())
}

fn next_open(et: DateTime<Tz>) -> Option<DateTime<Tz>> {
    (0..8).find_map(|days| {
        let date = et.date_naive().checked_add_days(chrono::Days::new(days))?;
        if !is_trading_day(date.weekday()) {
            return None;
        }
        let open = New_York
            .from_local_datetime(&date.and_time(open_time()))
            .single()?;
        (open > et).then_some(open)
    })
}

/// Every minute while the market is open; otherwise every 15 minutes, or sooner if the
/// open is closer than that.
fn poll_interval(now: DateTime<Utc>) -> Duration {
    let et = now.with_timezone(&New_York);
    if is_open(et) {
        return OPEN_POLL;
    }
    next_open(et)
        .and_then(|open| (open.with_timezone(&Utc) - now).to_std().ok())
        .unwrap_or(CLOSED_POLL)
        .clamp(OPEN_POLL, CLOSED_POLL)
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUOTE: &str = include_str!("../../tests/fixtures/finnhub_quote.json");
    const UNKNOWN: &str = include_str!("../../tests/fixtures/finnhub_unknown.json");

    fn utc(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn parses_quote_and_unknown_symbol() {
        let q = parse(QUOTE).unwrap().unwrap();
        assert_eq!(q.c, 182.4);
        assert_eq!(q.dp, Some(2.0934));
        assert!(parse(UNKNOWN).unwrap().is_none());
    }

    #[test]
    fn polls_fast_only_while_open() {
        // Wednesday 9:31 EDT.
        assert_eq!(poll_interval(utc("2026-09-23T13:31:00Z")), OPEN_POLL);
        // Friday 16:10 EDT.
        assert_eq!(poll_interval(utc("2026-09-25T20:10:00Z")), CLOSED_POLL);
        // Saturday midday.
        assert_eq!(poll_interval(utc("2026-09-26T16:00:00Z")), CLOSED_POLL);
        // Monday 9:25 EDT: five minutes to the open.
        assert_eq!(
            poll_interval(utc("2026-09-28T13:25:00Z")),
            Duration::from_secs(300)
        );
    }

    #[test]
    fn follows_daylight_saving() {
        // Monday after DST ends: 9:30 EST is 14:30 UTC, so 14:00 UTC is still closed.
        assert_eq!(poll_interval(utc("2026-11-02T14:00:00Z")), CLOSED_POLL);
        assert_eq!(poll_interval(utc("2026-11-02T14:31:00Z")), OPEN_POLL);
    }

    #[test]
    fn seeds_sparklines_from_cache() {
        let cached = Reading::Stocks(vec![Quote {
            symbol: "NVDA".into(),
            price: 1.0,
            change_pct: 0.0,
            spark: vec![1.0, 2.0],
        }]);
        let feed = Finnhub::new(&Config::default(), None, Some(&cached));
        assert_eq!(feed.history.lock().unwrap()["NVDA"].spark, [1.0, 2.0]);
    }
}
