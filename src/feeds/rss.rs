//! INTERCEPTS (RSS/Atom): headlines from whatever feeds `config.intercepts.rss` lists.
//! `feed_rs` auto-detects the format (RSS 0.9x/1/2, Atom, JSON Feed) from the body, so
//! one parser covers all of them. Every configured URL is fetched concurrently; a feed
//! that fails just drops out, same as air quality alongside Open-Meteo's forecast. Each
//! feed can rename itself and cap how many headlines it puts in the panel, so a busy
//! feed can't push a quiet one out.

use std::io::Cursor;
use std::time::Duration;

use futures_util::future::join_all;

use super::http::get_text;
use super::{Feed, FetchError};
use crate::config::{Config, RssFeed};
use crate::reading::{Headline, Reading};
use crate::source::SourceId;

const KEEP: usize = 8;

pub struct Rss {
    feeds: Vec<RssFeed>,
}

impl Rss {
    pub fn new(config: &Config) -> Self {
        Self {
            feeds: config.intercepts.rss.clone(),
        }
    }
}

impl Feed for Rss {
    fn source(&self) -> SourceId {
        SourceId::Rss
    }

    fn interval(&self) -> Duration {
        // Polls several third-party hosts each cycle, so it goes easier than HN's 5m.
        Duration::from_secs(15 * 60)
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        let bodies = join_all(self.feeds.iter().map(|f| get_text(http.get(&f.url)))).await;
        let per_feed = bodies
            .into_iter()
            .zip(&self.feeds)
            .filter_map(|(body, feed)| {
                let url = &feed.url;
                let text = body
                    .inspect_err(|e| tracing::warn!(url, "rss fetch: {e:?}"))
                    .ok()?;
                parse_feed(&text)
                    .inspect_err(|e| tracing::warn!(url, "rss parse: {e}"))
                    .ok()
                    .map(|headlines| (feed, headlines))
            });
        let headlines = merge(per_feed);
        if headlines.is_empty() {
            return Err(FetchError::Failed("no feeds returned anything".into()));
        }
        Ok(Reading::Rss(headlines))
    }
}

/// Each feed's newest headlines, up to its own `max` and under its own `name`, then
/// the newest `KEEP` of all of them.
fn merge<'a>(per_feed: impl IntoIterator<Item = (&'a RssFeed, Vec<Headline>)>) -> Vec<Headline> {
    let mut all = Vec::new();
    for (feed, mut headlines) in per_feed {
        newest_first(&mut headlines);
        if let Some(max) = feed.max {
            headlines.truncate(max);
        }
        if let Some(name) = &feed.name {
            for h in &mut headlines {
                h.source = name.clone();
            }
        }
        all.extend(headlines);
    }
    newest_first(&mut all);
    all.truncate(KEEP);
    all
}

/// Undated entries (Option::None) sort after every dated one.
fn newest_first(headlines: &mut [Headline]) {
    headlines.sort_by_key(|h| std::cmp::Reverse(h.published));
}

/// One feed's headlines, tagged with the feed's own title.
pub fn parse_feed(text: &str) -> Result<Vec<Headline>, String> {
    let feed = feed_rs::parser::parse(Cursor::new(text.as_bytes()))
        .map_err(|e| format!("unexpected payload: {e}"))?;
    let source = feed
        .title
        .map(|t| t.content)
        .unwrap_or_else(|| "RSS".into());
    Ok(feed
        .entries
        .into_iter()
        .filter_map(|e| {
            Some(Headline {
                title: e.title?.content,
                source: source.clone(),
                link: e.links.first().map(|l| l.href.clone()),
                published: e.published.or(e.updated),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS2: &str = include_str!("../../tests/fixtures/rss_feed.xml");
    const ATOM: &str = include_str!("../../tests/fixtures/atom_feed.xml");

    #[test]
    fn parses_rss2() {
        let headlines = parse_feed(RSS2).unwrap();
        assert_eq!(headlines.len(), 2);
        assert_eq!(headlines[0].source, "Night Wire");
        assert_eq!(
            headlines[0].title,
            "Undersea cable outage traced to a trawler"
        );
        assert!(
            headlines[0]
                .link
                .as_deref()
                .unwrap()
                .starts_with("https://")
        );
        assert!(headlines[0].published.is_some());
    }

    #[test]
    fn parses_atom() {
        let headlines = parse_feed(ATOM).unwrap();
        assert_eq!(headlines.len(), 1);
        assert_eq!(headlines[0].source, "Peripheral Vision");
        assert_eq!(
            headlines[0].title,
            "A field guide to abandoned server farms"
        );
    }

    fn headlines(source: &str, hours_ago: &[i64]) -> Vec<Headline> {
        let now = chrono::Utc::now();
        hours_ago
            .iter()
            .map(|h| Headline {
                title: format!("{source} {h}h"),
                source: source.into(),
                link: None,
                published: Some(now - chrono::Duration::hours(*h)),
            })
            .collect()
    }

    #[test]
    fn caps_and_names_apply_per_feed_before_the_merge() {
        // The busy feed has the eight newest headlines; uncapped, it would fill the panel.
        let busy = RssFeed {
            max: Some(3),
            name: Some("BUSY".into()),
            ..RssFeed::new("https://busy.example/feed")
        };
        let quiet = RssFeed::new("https://quiet.example/feed");
        let merged = merge([
            (
                &busy,
                headlines("Busy Wire - All Stories", &[8, 1, 2, 3, 4, 5, 6, 7]),
            ),
            (&quiet, headlines("Quiet", &[20, 30])),
        ]);
        let titles: Vec<&str> = merged.iter().map(|h| h.title.as_str()).collect();
        // The cap keeps a feed's newest, not its first-listed.
        assert_eq!(
            titles,
            [
                "Busy Wire - All Stories 1h",
                "Busy Wire - All Stories 2h",
                "Busy Wire - All Stories 3h",
                "Quiet 20h",
                "Quiet 30h"
            ]
        );
        assert!(merged[..3].iter().all(|h| h.source == "BUSY"));
        assert!(merged[3..].iter().all(|h| h.source == "Quiet"));
    }

    #[test]
    fn uncapped_feeds_still_share_the_panel_limit() {
        let a = RssFeed::new("https://a.example/feed");
        let b = RssFeed::new("https://b.example/feed");
        let merged = merge([
            (&a, headlines("A", &[1, 3, 5, 7, 9, 11])),
            (&b, headlines("B", &[2, 4, 6, 8, 10, 12])),
        ]);
        assert_eq!(merged.len(), KEEP);
        assert_eq!(merged[0].title, "A 1h");
        assert_eq!(merged[KEEP - 1].title, "B 8h");
    }

    #[test]
    fn rejects_a_broken_payload() {
        assert!(parse_feed("not xml").is_err());
    }

    /// `enabled()` in feeds/mod.rs keeps this from ever launching with an empty list;
    /// this just checks `fetch` itself doesn't panic or hang on it.
    #[tokio::test]
    async fn fails_cleanly_with_no_feeds_configured() {
        let feed = Rss::new(&Config::default());
        let result = feed.fetch(&reqwest::Client::new()).await;
        assert!(matches!(result, Err(FetchError::Failed(_))));
    }
}
