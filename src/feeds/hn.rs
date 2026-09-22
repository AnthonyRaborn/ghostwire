//! INTERCEPTS (Hacker News): the current front page from the official Firebase API.
//! One request for the ranking, then one per story, fetched concurrently.

use std::time::Duration;

use chrono::DateTime;
use futures_util::future::join_all;
use serde::Deserialize;

use super::http::{get_text, parse_json};
use super::{Feed, FetchError};
use crate::reading::{Reading, Story};
use crate::source::SourceId;

const TOP_URL: &str = "https://hacker-news.firebaseio.com/v0/topstories.json";
const STORIES: usize = 10;

pub struct HackerNews;

impl Feed for HackerNews {
    fn source(&self) -> SourceId {
        SourceId::Hn
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(5 * 60)
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        let ids: Vec<u64> =
            parse_json(&get_text(http.get(TOP_URL)).await?).map_err(FetchError::Failed)?;
        let items = join_all(ids.iter().take(STORIES).map(|id| {
            get_text(http.get(format!(
                "https://hacker-news.firebaseio.com/v0/item/{id}.json"
            )))
        }))
        .await;
        let stories: Vec<Story> = items
            .into_iter()
            .filter_map(|item| {
                let text = item
                    .inspect_err(|e| tracing::debug!("hn item: {e:?}"))
                    .ok()?;
                parse_item(&text)
                    .inspect_err(|e| tracing::warn!("hn item: {e}"))
                    .ok()?
            })
            .collect();
        if stories.is_empty() {
            return Err(FetchError::Failed("no stories came back".into()));
        }
        Ok(Reading::Hn(stories))
    }
}

#[derive(Deserialize)]
struct Item {
    id: u64,
    title: Option<String>,
    score: Option<u32>,
    descendants: Option<u32>,
    time: i64,
    #[serde(default)]
    dead: bool,
    #[serde(default)]
    deleted: bool,
}

/// `None` for missing, dead, deleted, or untitled items.
pub fn parse_item(text: &str) -> Result<Option<Story>, String> {
    let Some(item) = parse_json::<Option<Item>>(text)? else {
        return Ok(None);
    };
    if item.dead || item.deleted {
        return Ok(None);
    }
    let (Some(title), Some(posted)) = (item.title, DateTime::from_timestamp(item.time, 0)) else {
        return Ok(None);
    };
    Ok(Some(Story {
        id: item.id,
        title,
        score: item.score.unwrap_or(0),
        comments: item.descendants.unwrap_or(0),
        posted,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOP: &str = include_str!("../../tests/fixtures/hn_top.json");
    const ITEM: &str = include_str!("../../tests/fixtures/hn_item.json");

    #[test]
    fn parses_ranking_and_story() {
        let ids: Vec<u64> = parse_json(TOP).unwrap();
        assert!(ids.len() > STORIES);
        let story = parse_item(ITEM).unwrap().unwrap();
        assert_eq!(story.id, ids[0]);
        assert_eq!(story.title, "Xiaomi MiMo v2.6");
        assert_eq!(story.score, 579);
        assert_eq!(story.comments, 294);
    }

    #[test]
    fn skips_missing_and_dead_items() {
        assert!(parse_item("null").unwrap().is_none());
        assert!(
            parse_item(r#"{"id":1,"title":"x","time":0,"dead":true}"#)
                .unwrap()
                .is_none()
        );
        assert!(parse_item(r#"{"id":1,"time":0}"#).unwrap().is_none());
        let job = parse_item(r#"{"id":1,"title":"Hiring","time":0}"#)
            .unwrap()
            .unwrap();
        assert_eq!((job.score, job.comments), (0, 0));
    }
}
