//! Scrape result — port of `sdk/go/result_scrape.go`.
//!
//! Only the fields a typical caller cares about are strongly typed; the
//! rest land in `serde_json::Value` to survive API contract drift.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Response envelope from `POST /scrape`.
#[derive(Debug, Clone, Deserialize)]
pub struct ScrapeResult {
    /// UUID of the scrape.
    #[serde(default)]
    pub uuid: String,
    /// Echo of the config the server used.
    #[serde(default)]
    pub config: serde_json::Value,
    /// Execution context (cost, proxy, cache…).
    #[serde(default)]
    pub context: serde_json::Value,
    /// Result data.
    pub result: ResultData,
}

/// Body of the `result` field.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResultData {
    /// Scraped content (HTML, text, markdown…).
    #[serde(default)]
    pub content: String,
    /// Content encoding.
    #[serde(default)]
    pub content_encoding: String,
    /// Content type.
    #[serde(default)]
    pub content_type: String,
    /// Final URL.
    #[serde(default)]
    pub url: String,
    /// HTTP status code from the target.
    #[serde(default, deserialize_with = "null_as_zero_u16")]
    pub status_code: u16,
    /// Status string (`DONE`, etc.).
    #[serde(default)]
    pub status: String,
    /// Scrape success marker.
    #[serde(default)]
    pub success: bool,
    /// Scrape duration (seconds).
    #[serde(default)]
    pub duration: f64,
    /// Format marker (`raw`, `text`, `clob`, `blob`…).
    #[serde(default)]
    pub format: String,
    /// Error envelope for server-side scrape failures.
    #[serde(default)]
    pub error: Option<ScrapeErrorDetails>,
    /// Log URL for the dashboard.
    #[serde(default)]
    pub log_url: String,
    /// Response headers.
    #[serde(default)]
    pub response_headers: serde_json::Value,
    /// Request headers.
    #[serde(default)]
    pub request_headers: BTreeMap<String, String>,
    /// Screenshots captured during this scrape.
    #[serde(default)]
    pub screenshots: BTreeMap<String, serde_json::Value>,
    /// Extracted data (if any).
    #[serde(default)]
    pub extracted_data: Option<serde_json::Value>,
    /// Browser data (local/session storage, attachments…).
    #[serde(default)]
    pub browser_data: serde_json::Value,
    /// Iframes discovered during a rendered scrape. Each entry has at least
    /// a `url` field pointing at the embedded document. Stored as
    /// `serde_json::Value` because the exact shape varies with the engine
    /// version (url, url_discovered_as, navigation_id, …).
    #[serde(default)]
    pub iframes: serde_json::Value,
}

/// Inner error details.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScrapeErrorDetails {
    /// Error code.
    #[serde(default)]
    pub code: String,
    /// HTTP code.
    #[serde(default, deserialize_with = "null_as_zero_u16")]
    pub http_code: u16,
    /// Error message.
    #[serde(default)]
    pub message: String,
    /// Documentation URL.
    #[serde(default)]
    pub doc_url: String,
    /// Retryable flag.
    #[serde(default)]
    pub retryable: bool,
}

/// Coerce JSON `null | absent | number` into a plain `u16`, where null and
/// absent both collapse to zero. The server emits `status_code: null` and
/// `http_code: null` on large-object (blob/clob) offload responses, where
/// the real status lives in the signed-URL payload rather than the envelope.
/// Without this, the plain `u16` deserializer rejects `null` and the SDK
/// can't decode a successful large_object batch part.
fn null_as_zero_u16<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u16>::deserialize(deserializer)?.unwrap_or(0))
}
