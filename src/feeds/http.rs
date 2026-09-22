//! HTTP plumbing shared by the real feeds. Responses come back as text so parsers stay
//! pure functions that tests can feed from fixtures.

use std::error::Error;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{HeaderMap, RETRY_AFTER};
use serde::de::DeserializeOwned;

use super::FetchError;

/// OpenSky's rate-limit header; everyone else uses Retry-After.
const OPENSKY_RETRY_AFTER: &str = "x-rate-limit-retry-after-seconds";

/// Sends `req` and returns the body, mapping HTTP trouble onto the fiction:
/// 429 → TRACE, anything else that fails → ICE.
pub async fn get_text(req: reqwest::RequestBuilder) -> Result<String, FetchError> {
    fetch_text(req, false).await
}

/// Like [`get_text`], for requests that carry an API key: 401/403 means the key is bad,
/// which retrying won't fix, so the source goes OFFLINE instead of ICE.
pub async fn get_text_keyed(req: reqwest::RequestBuilder) -> Result<String, FetchError> {
    fetch_text(req, true).await
}

async fn fetch_text(req: reqwest::RequestBuilder, keyed: bool) -> Result<String, FetchError> {
    let resp = req
        .send()
        .await
        .map_err(|e| FetchError::Failed(describe(e)))?;
    let status = resp.status();
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(FetchError::RateLimited(retry_after(resp.headers())));
    }
    if keyed && matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(FetchError::NotConfigured(format!(
            "API key rejected (HTTP {})",
            status.as_u16()
        )));
    }
    if !status.is_success() {
        return Err(FetchError::Failed(format!("HTTP {}", status.as_u16())));
    }
    resp.text()
        .await
        .map_err(|e| FetchError::Failed(describe(e)))
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    [RETRY_AFTER.as_str(), OPENSKY_RETRY_AFTER]
        .iter()
        .find_map(|name| headers.get(*name)?.to_str().ok()?.trim().parse().ok())
        .map(Duration::from_secs)
}

pub fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| format!("unexpected payload: {e}"))
}

/// The error and its causes, without the URL (which can carry coordinates or keys).
fn describe(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderValue;

    use super::*;

    #[test]
    fn reads_either_retry_header() {
        let mut headers = HeaderMap::new();
        assert_eq!(retry_after(&headers), None);
        headers.insert(OPENSKY_RETRY_AFTER, HeaderValue::from_static("3600"));
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(3600)));
        headers.insert(RETRY_AFTER, HeaderValue::from_static(" 30 "));
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(30)));
    }
}
