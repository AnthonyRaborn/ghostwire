//! INTERCEPTS (RSS/Atom): headlines from whatever feeds `config.intercepts.rss` lists.
//! `feed_rs` auto-detects the format (RSS 0.9x/1/2, Atom, JSON Feed) from the body, so
//! one parser covers all of them. Every configured URL is fetched concurrently; a feed
//! that fails just drops out, same as air quality alongside Open-Meteo's forecast.

use std::io::Cursor;
use std::time::Duration;

use futures_util::future::join_all;

use super::http::get_text;
use super::{Feed, FetchError};
use crate::config::Config;
use crate::reading::{Headline, Reading};
use crate::source::SourceId;

const KEEP: usize = 8;

pub struct Rss {
    urls: Vec<String>,
}

impl Rss {
    pub fn new(config: &Config) -> Self {
        Self {
            urls: config.intercepts.rss.clone(),
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
        let bodies = join_all(self.urls.iter().map(|url| get_text(http.get(url)))).await;
        let mut headlines: Vec<Headline> = bodies
            .into_iter()
            .zip(&self.urls)
            .filter_map(|(body, url)| {
                let text = body
                    .inspect_err(|e| tracing::warn!(url, "rss fetch: {e:?}"))
                    .ok()?;
                parse_feed(&text)
                    .inspect_err(|e| tracing::warn!(url, "rss parse: {e}"))
                    .ok()
            })
            .flatten()
            .collect();
        if headlines.is_empty() {
            return Err(FetchError::Failed("no feeds returned anything".into()));
        }
        // Undated entries (Option::None) sort after every dated one.
        headlines.sort_by_key(|h| std::cmp::Reverse(h.published));
        headlines.truncate(KEEP);
        Ok(Reading::Rss(headlines))
    }
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
