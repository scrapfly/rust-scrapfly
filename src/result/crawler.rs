//! Crawler result objects — port of `sdk/go/result_crawler.go`.

use std::collections::BTreeMap;

use bytes::Bytes;
use serde::Deserialize;

/// Crawler status constants.
pub mod status {
    /// Pending — not yet picked up.
    pub const PENDING: &str = "PENDING";
    /// Running.
    pub const RUNNING: &str = "RUNNING";
    /// Done (check `is_success` for success/failure).
    pub const DONE: &str = "DONE";
    /// Cancelled by the user.
    pub const CANCELLED: &str = "CANCELLED";
}

/// Response from `POST /crawl`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerStartResponse {
    /// Crawler job UUID.
    #[serde(default)]
    pub crawler_uuid: String,
    /// Initial status.
    #[serde(default)]
    pub status: String,
}

/// Inner `state` block of [`CrawlerStatus`].
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerState {
    /// URLs visited.
    #[serde(default)]
    pub urls_visited: u64,
    /// URLs extracted.
    #[serde(default)]
    pub urls_extracted: u64,
    /// URLs failed.
    #[serde(default)]
    pub urls_failed: u64,
    /// URLs skipped.
    #[serde(default)]
    pub urls_skipped: u64,
    /// URLs queued.
    #[serde(default)]
    pub urls_to_crawl: u64,
    /// API credit used.
    #[serde(default)]
    pub api_credit_used: u64,
    /// Duration (seconds).
    #[serde(default)]
    pub duration: u64,
    /// Start time (Unix seconds, null while PENDING).
    #[serde(default)]
    pub start_time: Option<i64>,
    /// Stop time (Unix seconds, null until terminal).
    #[serde(default)]
    pub stop_time: Option<i64>,
    /// Documented stop reason (null while running).
    #[serde(default)]
    pub stop_reason: Option<String>,
}

/// Response from `GET /crawl/{uuid}/status`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerStatus {
    /// Crawler UUID.
    #[serde(default)]
    pub crawler_uuid: String,
    /// Status enum (`PENDING`, `RUNNING`, `DONE`, `CANCELLED`).
    #[serde(default)]
    pub status: String,
    /// Whether the crawler reached a terminal state.
    #[serde(default)]
    pub is_finished: bool,
    /// Success marker (nullable while running).
    #[serde(default)]
    pub is_success: Option<bool>,
    /// Per-job metrics.
    #[serde(default)]
    pub state: CrawlerState,
}

impl CrawlerStatus {
    /// True while still pending or running.
    pub fn is_running(&self) -> bool {
        self.status == status::PENDING || self.status == status::RUNNING
    }
    /// True when terminated successfully.
    pub fn is_complete(&self) -> bool {
        self.status == status::DONE && self.is_success == Some(true)
    }
    /// True when terminated with failure.
    pub fn is_failed(&self) -> bool {
        self.status == status::DONE && self.is_success == Some(false)
    }
    /// True when cancelled by the user.
    pub fn is_cancelled(&self) -> bool {
        self.status == status::CANCELLED
    }
}

/// One entry in the streaming `urls` list.
#[derive(Debug, Clone)]
pub struct CrawlerUrlEntry {
    /// URL.
    pub url: String,
    /// Status (visited/pending/failed/skipped) — echoed from the request.
    pub status: String,
    /// Reason for failure/skip (only set for `failed`/`skipped`).
    pub reason: String,
}

/// Streaming response from `GET /crawl/{uuid}/urls`.
#[derive(Debug, Clone, Default)]
pub struct CrawlerUrls {
    /// URL entries on this page.
    pub urls: Vec<CrawlerUrlEntry>,
    /// Page number.
    pub page: u32,
    /// Page size.
    pub per_page: u32,
}

impl CrawlerUrls {
    /// Parse a `text/plain` body into a [`CrawlerUrls`]. Mirrors
    /// `sdk/go/result_crawler.go::parseCrawlerURLs`.
    pub fn from_text(body: &str, status_hint: &str, page: u32, per_page: u32) -> Self {
        let mut urls = Vec::new();
        for raw_line in body.split('\n') {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            if status_hint == "visited" || status_hint == "pending" {
                urls.push(CrawlerUrlEntry {
                    url: line.to_string(),
                    status: status_hint.to_string(),
                    reason: String::new(),
                });
                continue;
            }
            if let Some(idx) = line.find(',') {
                urls.push(CrawlerUrlEntry {
                    url: line[..idx].to_string(),
                    status: status_hint.to_string(),
                    reason: line[idx + 1..].to_string(),
                });
            } else {
                urls.push(CrawlerUrlEntry {
                    url: line.to_string(),
                    status: status_hint.to_string(),
                    reason: String::new(),
                });
            }
        }
        Self {
            urls,
            page,
            per_page,
        }
    }
}

/// `GET /crawl/{uuid}/contents` bulk-JSON envelope.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerContents {
    /// `url → format → content`. The API can emit `null` for a format that
    /// couldn't be produced for a given URL (e.g. `extracted_data` on a page
    /// that no template matched); the SDK flattens `null → ""` so consumers
    /// always get a string and can check emptiness. Mirrors Go's map[string]string
    /// zero-value semantics.
    #[serde(default, deserialize_with = "deserialize_contents_map")]
    pub contents: BTreeMap<String, BTreeMap<String, String>>,
    /// Pagination links.
    #[serde(default)]
    pub links: CrawlerContentsLinks,
}

/// Deserialize `{url: {format: string|null}}` tolerating `null` inner values
/// by mapping them to the empty string.
fn deserialize_contents_map<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, BTreeMap<String, String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: BTreeMap<String, BTreeMap<String, Option<String>>> =
        BTreeMap::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .map(|(url, by_format)| {
            (
                url,
                by_format
                    .into_iter()
                    .map(|(fmt, body)| (fmt, body.unwrap_or_default()))
                    .collect(),
            )
        })
        .collect())
}

/// Pagination links returned with bulk contents.
///
/// `next`/`prev` arrive as JSON `null` when there is no adjacent page, which
/// would reject under a plain `String` field; [`null_as_empty_string`] maps
/// both null and absent to the empty string so the public API stays typed.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerContentsLinks {
    /// Crawled URLs link.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub crawled_urls: String,
    /// Next-page link (empty when on the last page).
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub next: String,
    /// Previous-page link (empty when on the first page).
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub prev: String,
}

/// Coerce JSON `null | absent | string` into a plain `String`, where null
/// and absent both collapse to the empty string. Mirrors Go's `string`
/// zero-value behavior under `encoding/json`.
fn null_as_empty_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// Typed content wrapper for a single crawled URL (`Crawl::read`).
#[derive(Debug, Clone, Default)]
pub struct CrawlContent {
    /// URL.
    pub url: String,
    /// Content in the requested format.
    pub content: String,
    /// Parent crawler UUID.
    pub crawl_uuid: String,
}

/// Artifact type — `warc` or `har`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrawlerArtifactType {
    /// WARC artifact.
    Warc,
    /// HAR artifact.
    Har,
}

impl CrawlerArtifactType {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Warc => "warc",
            Self::Har => "har",
        }
    }
}

/// WARC or HAR artifact downloaded from the crawler endpoint.
#[derive(Debug, Clone)]
pub struct CrawlerArtifact {
    /// Artifact type.
    pub artifact_type: CrawlerArtifactType,
    /// Raw bytes.
    pub data: Bytes,
}

impl CrawlerArtifact {
    /// Write the artifact to disk.
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, &self.data)
    }
    /// Byte length of the artifact.
    pub fn len(&self) -> usize {
        self.data.len()
    }
    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
