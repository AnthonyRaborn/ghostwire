//! HTTP plumbing shared by the real feeds. Responses come back as text so parsers stay
//! pure functions that tests can feed from fixtures.

use std::error::Error;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::RETRY_AFTER;
use serde::de::DeserializeOwned;

use super::FetchError;

/// Sends `req` and returns the body, mapping HTTP trouble onto the fiction:
/// 429 → TRACE, anything else that fails → ICE.
pub async fn get_text(req: reqwest::RequestBuilder) -> Result<String, FetchError> {
    let resp = req
        .send()
        .await
        .map_err(|e| FetchError::Failed(describe(e)))?;
    let status = resp.status();
    if status == StatusCode::TOO_MANY_REQUESTS {
        let after = resp
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok())
            .map(Duration::from_secs);
        return Err(FetchError::RateLimited(after));
    }
    if !status.is_success() {
        return Err(FetchError::Failed(format!("HTTP {}", status.as_u16())));
    }
    resp.text()
        .await
        .map_err(|e| FetchError::Failed(describe(e)))
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
