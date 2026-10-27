//! `--check-config`: what the rig would run with this config and keys, worked out
//! without starting the display or touching the network.

use std::fmt::Write;
use std::time::Duration;

use crate::config::Config;
use crate::feeds::{self, Feed, Visit, coingecko, finnhub};
use crate::keys::Keys;
use crate::source::SourceId;

/// Where each source stands under the config.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Runs {
        every: Duration,
    },
    /// Runs, but can't do everything: the reason why.
    Limited {
        every: Duration,
        reason: String,
    },
    /// Turned off in the config.
    Off(&'static str),
    /// On, but missing what it needs to run.
    Offline(String),
}

/// What `--check-config` needs to know about the files it read.
pub struct Files<'a> {
    pub config: &'a str,
    pub config_found: bool,
    pub keys: &'a str,
    pub keys_found: bool,
    pub keys_exposed: bool,
}

/// Every source's status, in ticker order.
pub fn statuses(config: &Config, keys: &Keys) -> Vec<(SourceId, Status)> {
    struct Checker(Vec<(SourceId, Status)>);

    impl Visit for Checker {
        fn visit<F: Feed>(&mut self, feed: F) {
            let every = feed.interval();
            let status = match (feed.offline(), feed.limited()) {
                (Some(reason), _) => Status::Offline(reason),
                (None, Some(reason)) => Status::Limited { every, reason },
                (None, None) => Status::Runs { every },
            };
            self.0.push((feed.source(), status));
        }
    }

    let mut checker = Checker(Vec::new());
    feeds::build_all(config, keys, false, None, &mut checker);
    let mut all = checker.0;
    for source in SourceId::ALL {
        if let Some(why) = feeds::disabled(config, source) {
            all.push((source, Status::Off(why)));
        }
    }
    all.sort_by_key(|(id, _)| *id);
    all
}

/// Settings the rig quietly drops or works around, worth fixing.
fn warnings(config: &Config, files: &Files) -> Vec<String> {
    let mut out = Vec::new();
    if !files.config_found {
        out.push(format!(
            "no config at {}: running on defaults (ghostwire --init-config writes one)",
            files.config
        ));
    }
    if files.keys_exposed {
        out.push(format!(
            "{} is readable by other users: chmod 600 it",
            files.keys
        ));
    }
    for symbol in finnhub::symbols(config).1 {
        out.push(format!(
            "ignoring [zaibatsu] stock {symbol:?}: letters, digits, '.' and '-' only"
        ));
    }
    for id in coingecko::coin_ids(config).1 {
        out.push(format!(
            "ignoring [zaibatsu] coin {id:?}: expected a CoinGecko id like \"bitcoin\""
        ));
    }
    out
}

fn every(d: Duration) -> String {
    match d.as_secs() {
        s if s % 3_600 == 0 => format!("every {}h", s / 3_600),
        s if s % 60 == 0 => format!("every {}m", s / 60),
        s => format!("every {s}s"),
    }
}

/// What a running source will track, where the config decides it.
fn detail(config: &Config, source: SourceId) -> String {
    match source {
        SourceId::Stocks => finnhub::symbols(config).0.join(", "),
        SourceId::Crypto => coingecko::coin_ids(config).0.join(", "),
        SourceId::Rss => config
            .intercepts
            .rss
            .iter()
            .map(|f| f.name.clone().unwrap_or_else(|| f.url.clone()))
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

/// The whole report, as printed.
pub fn report(config: &Config, keys: &Keys, files: &Files) -> String {
    let mut out = String::new();
    let sector = &config.sector;
    let found = |found: bool| if found { "" } else { " (not found)" };
    let _ = writeln!(out, "config  {}{}", files.config, found(files.config_found));
    let _ = writeln!(out, "keys    {}{}", files.keys, found(files.keys_found));
    let fix = match sector.fix() {
        Some((lat, lon)) => format!("at {lat:.2}, {lon:.2}"),
        None => "no lat/lon".into(),
    };
    let country = sector.country.as_deref().unwrap_or("no country");
    let units = format!("{:?}", sector.units).to_lowercase();
    let _ = writeln!(out, "sector  {} {fix} · {country} · {units}\n", sector.name);

    let statuses = statuses(config, keys);
    for (id, status) in &statuses {
        let (tag, when, what) = match status {
            Status::Runs { every: e } => ("RUNS", every(*e), detail(config, *id)),
            Status::Limited { every: e, reason } => ("LIMITED", every(*e), reason.clone()),
            Status::Off(why) => ("OFF", String::new(), (*why).to_string()),
            Status::Offline(why) => ("OFFLINE", String::new(), why.clone()),
        };
        let line = format!("  {tag:<8} {:<12} {when:<10} {what}", id.handle());
        let _ = writeln!(out, "{}", line.trim_end());
    }

    let warnings = warnings(config, files);
    if !warnings.is_empty() {
        let _ = writeln!(out, "\nwarnings");
        for w in &warnings {
            let _ = writeln!(out, "  {w}");
        }
    }
    let running = statuses
        .iter()
        .filter(|(_, s)| matches!(s, Status::Runs { .. } | Status::Limited { .. }))
        .count();
    let _ = writeln!(
        out,
        "\n{running} of {} sources will run.",
        SourceId::ALL.len()
    );
    out
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use super::*;
    use crate::feeds::FetchError;

    fn files() -> Files<'static> {
        Files {
            config: "~/.config/ghostwire/config.toml",
            config_found: true,
            keys: "~/.config/ghostwire/keys.toml",
            keys_found: true,
            keys_exposed: false,
        }
    }

    fn status(statuses: &[(SourceId, Status)], id: SourceId) -> &Status {
        &statuses.iter().find(|(s, _)| *s == id).unwrap().1
    }

    #[test]
    fn a_bare_config_runs_what_needs_nothing() {
        let config = Config::parse("[intercepts]\nhn = false").unwrap();
        let statuses = statuses(&config, &Keys::default());
        assert_eq!(statuses.len(), SourceId::ALL.len());
        assert!(matches!(
            status(&statuses, SourceId::Stocks),
            Status::Offline(r) if r.contains("finnhub")
        ));
        assert!(matches!(
            status(&statuses, SourceId::Weather),
            Status::Offline(_)
        ));
        assert!(matches!(
            status(&statuses, SourceId::Ioda),
            Status::Offline(_)
        ));
        assert!(matches!(
            status(&statuses, SourceId::Quakes),
            Status::Limited { .. }
        ));
        assert!(matches!(
            status(&statuses, SourceId::Crypto),
            Status::Runs { .. }
        ));
        assert_eq!(
            status(&statuses, SourceId::Hn),
            &Status::Off("[intercepts] hn = false")
        );
        assert!(matches!(status(&statuses, SourceId::Rss), Status::Off(_)));
    }

    #[test]
    fn the_report_names_problems_and_counts_what_runs() {
        let config = Config::parse(
            "[sector]\nname = \"HOME\"\nlat = 37.77\nlon = -122.42\ncountry = \"US\"\n\
             [zaibatsu]\nstocks = [\"NVDA\", \"BRK B\"]\ncoins = [\"bitcoin\", \"doge coin\"]",
        )
        .unwrap();
        let keys = Keys::parse("finnhub = \"k\"").unwrap();
        let mut files = files();
        files.keys_exposed = true;
        let text = report(&config, &keys, &files);
        println!("{text}");
        assert!(text.contains("sector  HOME at 37.77, -122.42 · US · metric"));
        assert!(text.contains("RUNS     FINNHUB"));
        assert!(text.contains("NVDA"));
        assert!(text.contains("RUNS     COINGECKO    every 2m   bitcoin"));
        assert!(text.contains("OFF      RSS"));
        assert!(text.contains("ignoring [zaibatsu] stock \"BRK B\""));
        assert!(text.contains("ignoring [zaibatsu] coin \"doge coin\""));
        assert!(text.contains("keys.toml is readable by other users"));
        assert!(text.ends_with("11 of 12 sources will run.\n"));
    }

    /// `offline()` is a promise about what `fetch` does with the same config. Check it
    /// against the real thing, through a proxy that refuses every connection so no
    /// request leaves the machine: a feed that isn't offline fails as ICE instead.
    #[tokio::test]
    async fn offline_matches_what_fetch_reports() {
        type Outcome = Pin<Box<dyn Future<Output = (SourceId, bool, bool)> + Send>>;
        struct Probe(reqwest::Client, Vec<Outcome>);

        impl Visit for Probe {
            fn visit<F: Feed>(&mut self, feed: F) {
                let http = self.0.clone();
                self.1.push(Box::pin(async move {
                    let offline = feed.offline().is_some();
                    let result = feed.fetch(&http).await;
                    let not_configured = matches!(result, Err(FetchError::NotConfigured(_)));
                    (feed.source(), offline, not_configured)
                }));
            }
        }

        let http = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .build()
            .unwrap();
        let bare = Config::parse("[intercepts]\nrss = [\"https://a.example/feed\"]").unwrap();
        let full = Config::parse(
            "[sector]\nlat = 37.77\nlon = -122.42\ncountry = \"US\"\n\
             [intercepts]\nrss = [\"https://a.example/feed\"]",
        )
        .unwrap();
        let keyed = Keys::parse("finnhub = \"k\"").unwrap();
        for (config, keys) in [(&bare, Keys::default()), (&full, keyed)] {
            let mut probe = Probe(http.clone(), Vec::new());
            feeds::build_all(config, &keys, false, None, &mut probe);
            assert_eq!(probe.1.len(), SourceId::ALL.len());
            for outcome in probe.1 {
                let (source, offline, not_configured) = outcome.await;
                assert_eq!(offline, not_configured, "{source:?}");
            }
        }
    }
}
