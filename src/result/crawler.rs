//! Crawler result objects — port of `sdk/go/result_crawler.go`.

use std::collections::BTreeMap;

use bytes::Bytes;
use serde::Deserialize;

use crate::enums::CrawlerWebhookEvent;

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
    /// Search index state. `None` unless the crawl was started with
    /// `search: true`; polling it is the webhook-free way to learn when the
    /// index became queryable.
    #[serde(default)]
    pub search: Option<CrawlerSearchState>,
    /// Auto-refresh state. `None` unless the crawl re-scrapes itself on a
    /// period.
    #[serde(default)]
    pub refresh: Option<CrawlerRefreshState>,
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

/// Auto-refresh states carried by [`CrawlerRefreshState::status`].
pub mod refresh_status {
    /// The crawl does not re-scrape itself.
    pub const DISABLED: &str = "DISABLED";
    /// A refresh run is due at `next_run_at`.
    pub const SCHEDULED: &str = "SCHEDULED";
    /// A refresh run is in flight.
    pub const RUNNING: &str = "RUNNING";
    /// The last run failed; see `error`.
    pub const FAILED: &str = "FAILED";
}

/// One row of a crawl's refresh timeline.
///
/// `sample_updated` / `sample_removed` carry at most ten URLs each. The full
/// lists are never inlined: a 5,000-page crawl would otherwise put 5,000
/// strings into every status poll.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerRefreshEntry {
    /// ISO-8601 timestamp of the run.
    #[serde(default)]
    pub at: Option<String>,
    /// Refresh generation this run produced, 1 for the first.
    #[serde(default)]
    pub generation: u32,
    /// URLs this run discovered that the crawl did not hold.
    #[serde(default)]
    pub added: u64,
    /// Known URLs whose content fingerprint changed.
    #[serde(default)]
    pub updated: u64,
    /// Known URLs that no longer exist and were dropped.
    #[serde(default)]
    pub removed: u64,
    /// Re-scraped with an identical fingerprint: no embedding, no index write.
    #[serde(default)]
    pub unchanged: u64,
    /// URLs the run could not fetch. They keep their previous content.
    #[serde(default)]
    pub failed: u64,
    /// Wall time of the run.
    #[serde(default)]
    pub duration_ms: u64,
    /// Index status after the run, `None` when the crawl has no search index.
    #[serde(default)]
    pub search_status: Option<String>,
    /// Failure reason when the run itself failed.
    #[serde(default)]
    pub error: Option<String>,
    /// Up to ten re-indexed URLs.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub sample_updated: Vec<String>,
    /// Up to ten dropped URLs.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub sample_removed: Vec<String>,
}

impl CrawlerRefreshEntry {
    /// Pages this run actually touched. Zero means the site stood still and
    /// the run cost no re-indexing.
    pub fn changed(&self) -> u64 {
        self.added + self.updated + self.removed
    }
}

/// Coerce JSON `null | absent | array` into a plain `Vec<T>`, where null and
/// absent both collapse to empty. Every list on this surface is rendered from
/// a Go slice declared without `omitempty`, so a nil slice reaches the wire as
/// `null`; `serde(default)` alone covers only the absent key and would reject
/// the whole envelope. Mirrors Go's nil-slice-is-an-empty-slice semantics.
fn null_as_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

/// The `refresh` block of a crawl, carried by `GET /crawl/{uuid}/status` and
/// returned by the three refresh calls.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerRefreshState {
    /// Whether the crawl re-scrapes itself on a period.
    #[serde(default)]
    pub enabled: bool,
    /// Period between runs, 0 when disabled.
    #[serde(default)]
    pub interval_seconds: u32,
    /// One of [`refresh_status`].
    #[serde(default)]
    pub status: String,
    /// Refresh runs completed so far.
    #[serde(default)]
    pub generation: u32,
    /// ISO-8601 timestamp of the last completed run.
    #[serde(default)]
    pub last_run_at: Option<String>,
    /// ISO-8601 timestamp of the next due run, `None` when disabled.
    #[serde(default)]
    pub next_run_at: Option<String>,
    /// ISO-8601 start of the run in flight, `None` unless `status` is
    /// `RUNNING`. Only `GET /crawl/{uuid}/status` reports it: the three
    /// refresh calls render a typed block that omits the key, where it reads
    /// `None` whatever the schedule is doing.
    #[serde(default)]
    pub started_at: Option<String>,
    /// Failure reason when `status` is `FAILED`.
    #[serde(default)]
    pub error: Option<String>,
    /// Failed runs since the last success. Route-scoped like `started_at`, so
    /// a zero read off a refresh call means "not reported", not "no failures".
    #[serde(default)]
    pub consecutive_failures: u32,
    /// Newest last, capped at the 50 most recent runs.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub history: Vec<CrawlerRefreshEntry>,
}

impl CrawlerRefreshState {
    /// Decode a refresh envelope.
    ///
    /// The three refresh calls answer with the state at the top level;
    /// `GET /crawl/{uuid}/status` nests it under `refresh`. Accepting both
    /// means the SDK never has to guess which call produced the bytes.
    pub fn from_envelope(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        let mut envelope: serde_json::Value = serde_json::from_slice(bytes)?;
        let nested = envelope
            .get_mut("refresh")
            .filter(|block| block.is_object())
            .map(serde_json::Value::take);
        serde_json::from_value(nested.unwrap_or(envelope))
    }

    /// True while a refresh run is in flight.
    pub fn is_running(&self) -> bool {
        self.status == refresh_status::RUNNING
    }

    /// Most recent timeline row, `None` before the first run.
    pub fn last_run(&self) -> Option<&CrawlerRefreshEntry> {
        self.history.last()
    }
}

/// Search index states carried by [`CrawlerSearchState::status`]. Only `READY`
/// and `PARTIAL` can answer a query.
pub mod search_status {
    /// Search was not requested on this crawl.
    pub const DISABLED: &str = "DISABLED";
    /// The index is still being built.
    pub const BUILDING: &str = "BUILDING";
    /// The index is complete and queryable.
    pub const READY: &str = "READY";
    /// The index covers only part of the crawl but is queryable.
    pub const PARTIAL: &str = "PARTIAL";
    /// The build failed; see `error`.
    pub const FAILED: &str = "FAILED";
}

/// The `search` block describing a crawl's index, carried by
/// `GET /crawl/{uuid}/status` and by the two search webhooks.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerSearchState {
    /// One of [`search_status`].
    #[serde(default)]
    pub status: String,
    /// Storage path of the index manifest, absent until published.
    #[serde(default)]
    pub manifest: Option<String>,
    /// Crawled pages represented in the index.
    #[serde(default)]
    pub documents: u64,
    /// Embedded chunks those pages were split into.
    #[serde(default)]
    pub vectors: u64,
    /// Chunks discarded during the build (embedding failures, oversized rows).
    #[serde(default)]
    pub dropped: u64,
    /// Chunks still waiting to be embedded at snapshot time.
    #[serde(default)]
    pub queue_depth: u64,
    /// Published Lance fragments.
    #[serde(default)]
    pub fragments: u64,
    /// Failure reason when `status` is `FAILED`.
    #[serde(default)]
    pub error: Option<String>,
    /// ISO-8601 timestamp of the terminal publish.
    #[serde(default)]
    pub built_at: Option<String>,
    /// Vector index type (e.g. `IVF_PQ`), absent below the index threshold.
    #[serde(default)]
    pub index: Option<String>,
    /// Bumped when a paused crawl resumes and rebuilds. Results from
    /// different generations are not comparable.
    #[serde(default)]
    pub generation: Option<u32>,
}

impl CrawlerSearchState {
    /// True when the index can answer a query right now.
    pub fn is_searchable(&self) -> bool {
        self.status == search_status::READY || self.status == search_status::PARTIAL
    }
}

/// Per-leg scores behind a search result. Which fields are populated depends
/// on the mode.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerSearchScores {
    /// Vector-leg similarity.
    #[serde(default)]
    pub vector: Option<f64>,
    /// Full-text-leg score.
    #[serde(default)]
    pub fts: Option<f64>,
    /// Reciprocal rank fusion score (hybrid mode).
    #[serde(default)]
    pub rrf: Option<f64>,
}

/// One matched chunk from `POST /crawl/search`.
///
/// A result is a chunk, not a page: `chunk_id` orders chunks within one
/// crawled document and `text` is only the matched slice. Expand a hit back to
/// the whole document through `contents_url`, or through
/// `warc_offset`/`warc_end` against the crawl's WARC artifact.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerSearchResult {
    /// 1-based position in the merged ranking.
    #[serde(default)]
    pub rank: u32,
    /// The score used for ordering (RRF in hybrid mode).
    #[serde(default)]
    pub score: f64,
    /// Per-leg scores.
    #[serde(default)]
    pub scores: CrawlerSearchScores,
    /// The crawl this chunk came from.
    #[serde(default)]
    pub crawler_uuid: String,
    /// The crawled URL.
    #[serde(default)]
    pub url: String,
    /// Document title, absent when the page had none.
    #[serde(default)]
    pub title: Option<String>,
    /// Which stored format was indexed.
    #[serde(default)]
    pub source_format: Option<String>,
    /// Content type of the stored document.
    #[serde(default)]
    pub content_type: Option<String>,
    /// Chunk index within the document.
    #[serde(default)]
    pub chunk_id: u32,
    /// The matched chunk text.
    #[serde(default)]
    pub text: String,
    /// Byte offset of the document record in the crawl WARC.
    #[serde(default)]
    pub warc_offset: Option<u64>,
    /// End byte offset of that record.
    #[serde(default)]
    pub warc_end: Option<u64>,
    /// Ready-made `/crawl/{uuid}/contents` URL for the whole document.
    #[serde(default)]
    pub contents_url: Option<String>,
}

/// A crawl that was opened and searched.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerSearchCrawl {
    /// Crawl UUID.
    #[serde(default)]
    pub crawler_uuid: String,
    /// Documents in that crawl's index.
    #[serde(default)]
    pub documents: u64,
    /// Vectors in that crawl's index.
    #[serde(default)]
    pub vectors: u64,
    /// Vector index type.
    #[serde(default)]
    pub index: Option<String>,
}

/// Reasons a requested crawl contributed nothing.
pub mod skip_reason {
    /// The crawl was not started with `search: true`.
    pub const SEARCH_NOT_ENABLED: &str = "search_not_enabled";
    /// The index is still building.
    pub const SEARCH_NOT_READY: &str = "search_not_ready";
    /// The index build failed.
    pub const SEARCH_FAILED: &str = "search_failed";
    /// Search is turned off for this crawl.
    pub const SEARCH_DISABLED: &str = "search_disabled";
    /// The index was built with an incompatible embedding contract.
    pub const INCOMPATIBLE_INDEX: &str = "incompatible_index";
    /// The fan-out deadline hit before this crawl was opened.
    pub const DEADLINE: &str = "deadline";
}

/// A requested crawl that contributed nothing, and why.
///
/// A skip is never fatal: the search still answers from the remaining crawls.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerSearchSkipped {
    /// Crawl UUID.
    #[serde(default)]
    pub crawler_uuid: String,
    /// One of [`skip_reason`].
    #[serde(default)]
    pub reason: String,
    /// The index status at the time, when the reason has one.
    #[serde(default)]
    pub status: Option<String>,
}

/// Fan-out timing and IO counters.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerSearchStats {
    /// Wall time of the whole fan-out.
    #[serde(default)]
    pub duration_ms: u64,
    /// Crawls actually opened.
    #[serde(default)]
    pub crawls_searched: u32,
    /// Candidate rows considered before the final cut.
    #[serde(default)]
    pub candidates: u64,
    /// Object-store reads performed.
    #[serde(default)]
    pub gcs_gets: u64,
}

/// Response from `POST /crawl/search`.
///
/// The envelope states its own completeness: `exact` with most crawls unopened
/// is the normal outcome, because the fan-out proves via an admissible bound
/// that the unopened crawls held nothing better. `partial` means the deadline
/// cut the fan-out short.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerSearchResponse {
    /// The query as the server understood it.
    #[serde(default)]
    pub query: String,
    /// The mode that ran.
    #[serde(default)]
    pub mode: String,
    /// The effective result cap.
    #[serde(default)]
    pub limit: u32,
    /// `exact` or `partial`.
    #[serde(default)]
    pub completeness: String,
    /// Crawls that were opened and searched.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub crawls: Vec<CrawlerSearchCrawl>,
    /// Requested crawls that contributed nothing, with a reason.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub skipped: Vec<CrawlerSearchSkipped>,
    /// The merged ranking.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub results: Vec<CrawlerSearchResult>,
    /// Timing and IO counters.
    #[serde(default)]
    pub stats: CrawlerSearchStats,

    /// Crawls in the request.
    #[serde(default)]
    pub crawls_requested: u32,
    /// Crawls opened.
    #[serde(default)]
    pub crawls_searched: u32,
    /// Crawls the bound proved could not contribute.
    #[serde(default)]
    pub crawls_pruned_exact: u32,
    /// Crawls the deadline left unopened, by uuid. The wire names the crawls
    /// rather than counting them, because a caller told "3 skipped" cannot
    /// retry them. `serde(default)` covers an absent key, not a wrong type, so
    /// a scalar here rejects every real response.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub crawls_skipped_deadline: Vec<String>,
    /// Crawls whose leg errored, each with the reason it carried. Same
    /// list-not-count shape as `crawls_skipped_deadline`.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub crawls_failed: Vec<CrawlerSearchSkipped>,
    /// Score threshold at the cut.
    #[serde(default)]
    pub theta: Option<f64>,
    /// Best possible score among unopened crawls.
    #[serde(default)]
    pub max_ub_unsearched: Option<f64>,

    /// Opaque token for the next page, absent on the last one. Paging is
    /// cursor-based: an offset over a partial fan-out would re-run the legs
    /// and shift ranks.
    #[serde(default)]
    pub cursor: Option<String>,
}

impl CrawlerSearchResponse {
    /// True when the ranking is provably complete for the requested crawls.
    pub fn is_exact(&self) -> bool {
        self.completeness == "exact"
    }
}

/// One retrieved chunk the answer may cite.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerPromptSource {
    /// Citation id referenced by `sources_used`.
    #[serde(default)]
    pub id: u32,
    /// The crawl this source came from.
    #[serde(default)]
    pub crawler_uuid: String,
    /// The crawled URL.
    #[serde(default)]
    pub url: String,
    /// Document title.
    #[serde(default)]
    pub title: Option<String>,
    /// Retrieval score.
    #[serde(default)]
    pub score: Option<f64>,
}

/// Payload of the terminal `done` frame.
///
/// It reports the flat price and nothing about how the answer was produced:
/// which model ran, how many tokens it used and what the provider charged are
/// not the caller's side of the deal.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerPromptDone {
    /// The server-validated citation set.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub sources_used: Vec<u32>,
    /// Retrieved chunks the context budget could not fit. Non-zero means the
    /// answer was written from a subset of what retrieval returned.
    #[serde(default)]
    pub sources_dropped: u32,
    /// True when the model hit its output cap. The answer is still delivered;
    /// whether to use it is the caller's call.
    #[serde(default)]
    pub truncated: bool,
}

/// One decoded frame of the `POST /crawl/prompt` stream.
///
/// Frames arrive as `Source`*, then `Token`*, then one `Done`. An `error`
/// frame is surfaced as [`crate::ScrapflyError`] rather than as a variant,
/// because
/// generation can fail after tokens have already been delivered and a variant
/// is too easy to ignore.
#[derive(Debug, Clone)]
pub enum CrawlerPromptEvent {
    /// A retrieved chunk the answer may cite.
    Source(CrawlerPromptSource),
    /// A text delta of the answer.
    Token(String),
    /// The terminal frame.
    Done(CrawlerPromptDone),
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

// =============================================================================
// Webhook payloads
// =============================================================================

/// Fields every crawler webhook payload carries.
///
/// The envelope has no top-level uuid and no timestamp: `crawler_uuid` is the
/// only handle on the crawl, and the only timing on the delivery is
/// `state.start_time` / `state.stop_time`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerWebhookCommon {
    /// The crawl this delivery is about.
    #[serde(default)]
    pub crawler_uuid: String,
    /// Project the crawl belongs to.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub project: String,
    /// `LIVE` or `TEST`.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub env: String,
    /// Action tag. Empty on the two search events, which are the only crawler
    /// webhooks emitted without one.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub action: String,
    /// Crawl counters as they stood when the event was emitted.
    #[serde(default)]
    pub state: CrawlerState,
}

/// `links` block pointing at the crawl's status route.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerWebhookStatusLink {
    /// `GET /crawl/{uuid}/status` URL.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub status: String,
}

/// `links` block of a `crawler_url_failed` delivery.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerWebhookLogLink {
    /// Scrape log URL, empty when the failure produced no log.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub log: String,
}

/// The scrape behind a `crawler_url_visited` delivery.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerWebhookScrape {
    /// Upstream status code.
    #[serde(default)]
    pub status_code: u16,
    /// Proxy country the page was fetched through.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub country: String,
    /// Scrape log uuid.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub log_uuid: String,
    /// Scrape log URL.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub log_url: String,
    /// `format -> content` for the formats the crawl stores.
    #[serde(default, deserialize_with = "null_as_empty_string_map")]
    pub content: BTreeMap<String, String>,
}

/// Deserialize `{key: string|null}` into a plain map, collapsing a null map
/// and a null value alike to their empty form. Same zero-value contract as
/// [`null_as_empty_string`], one level up.
fn null_as_empty_string_map<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<BTreeMap<String, Option<String>>> = Option::deserialize(deserializer)?;
    Ok(raw
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| (key, value.unwrap_or_default()))
        .collect())
}

/// Payload of `crawler_started`, `crawler_stopped`, `crawler_cancelled` and
/// `crawler_finished`, which are one shape on the wire and differ only by
/// event name.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerLifecyclePayload {
    /// Fields shared by every crawler webhook.
    #[serde(flatten)]
    pub common: CrawlerWebhookCommon,
    /// The URL the crawl was started from.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub seed_url: String,
    /// Status route for the crawl.
    #[serde(default)]
    pub links: CrawlerWebhookStatusLink,
}

/// Payload of `crawler_url_visited`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerUrlVisitedPayload {
    /// Fields shared by every crawler webhook.
    #[serde(flatten)]
    pub common: CrawlerWebhookCommon,
    /// The URL that was visited.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub url: String,
    /// The scrape that fetched it.
    #[serde(default)]
    pub scrape: CrawlerWebhookScrape,
}

/// Payload of `crawler_url_skipped`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerUrlSkippedPayload {
    /// Fields shared by every crawler webhook.
    #[serde(flatten)]
    pub common: CrawlerWebhookCommon,
    /// `url -> skip reason` (e.g. `page_limit`).
    #[serde(default, deserialize_with = "null_as_empty_string_map")]
    pub urls: BTreeMap<String, String>,
}

/// Payload of `crawler_url_discovered`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerUrlDiscoveredPayload {
    /// Fields shared by every crawler webhook.
    #[serde(flatten)]
    pub common: CrawlerWebhookCommon,
    /// The page the links were found on.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub origin: String,
    /// The links that page contributed to the frontier.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub discovered_urls: Vec<String>,
}

/// Payload of `crawler_url_failed`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerUrlFailedPayload {
    /// Fields shared by every crawler webhook.
    #[serde(flatten)]
    pub common: CrawlerWebhookCommon,
    /// The URL that could not be fetched.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub url: String,
    /// Scrapfly error code the attempt ended on.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub error: String,
    /// The scrape config the crawl used for that URL. Kept opaque because it
    /// carries whatever the crawl was configured with, which is the full
    /// scrape surface rather than a crawler-specific shape.
    #[serde(default)]
    pub scrape_config: serde_json::Value,
    /// Log route for the failed attempt.
    #[serde(default)]
    pub links: CrawlerWebhookLogLink,
}

/// Payload of `crawler_search_ready` and `crawler_search_failed`.
///
/// The index publishes after the crawl's own success classification and can
/// fail without the crawl failing, which is why these are not lifecycle
/// events and why they carry no `action`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerSearchPayload {
    /// Fields shared by every crawler webhook.
    #[serde(flatten)]
    pub common: CrawlerWebhookCommon,
    /// The URL the crawl was started from.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub seed_url: String,
    /// Status route for the crawl.
    #[serde(default)]
    pub links: CrawlerWebhookStatusLink,
    /// The index as the status route reports it.
    #[serde(default)]
    pub search: CrawlerSearchState,
}

/// The URLs a refresh run touched, as `crawler_updated` reports them.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerUpdatedDocuments {
    /// Re-indexed URLs, added and changed alike. Which of the two a URL was
    /// only survives in the counts on `refresh`.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub updated: Vec<String>,
    /// URLs the run dropped.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub removed: Vec<String>,
    /// Either list was cut at Scrapfly's 100-URL cap. The counts on `refresh`
    /// still describe the whole run.
    #[serde(default)]
    pub truncated: bool,
}

/// Payload of `crawler_updated`, emitted once per auto-refresh run that
/// changed at least one page.
///
/// A run over a site that stood still, and a run that failed outright, change
/// nothing and are not delivered, so receiving this event is by itself proof
/// of a diff.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CrawlerUpdatedPayload {
    /// Fields shared by every crawler webhook.
    #[serde(flatten)]
    pub common: CrawlerWebhookCommon,
    /// The URL the crawl was started from.
    #[serde(default, deserialize_with = "null_as_empty_string")]
    pub seed_url: String,
    /// Status route for the crawl.
    #[serde(default)]
    pub links: CrawlerWebhookStatusLink,
    /// The run as the refresh timeline records it, minus the sample lists:
    /// this event carries the URLs in `documents` instead, at a higher cap.
    #[serde(default)]
    pub refresh: CrawlerRefreshEntry,
    /// The URLs behind the counts on `refresh`.
    #[serde(default)]
    pub documents: CrawlerUpdatedDocuments,
}

/// One decoded crawler webhook delivery.
///
/// The envelope is always `{"event": ..., "payload": {...}}` and the event
/// name is what decides the payload shape, so the enum is tagged on it: an
/// event this SDK does not know rejects rather than decoding into the wrong
/// variant.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "event", content = "payload", rename_all = "snake_case")]
pub enum CrawlerWebhook {
    /// `crawler_started`.
    CrawlerStarted(CrawlerLifecyclePayload),
    /// `crawler_url_visited`.
    CrawlerUrlVisited(CrawlerUrlVisitedPayload),
    /// `crawler_url_skipped`.
    CrawlerUrlSkipped(CrawlerUrlSkippedPayload),
    /// `crawler_url_discovered`.
    CrawlerUrlDiscovered(CrawlerUrlDiscoveredPayload),
    /// `crawler_url_failed`.
    CrawlerUrlFailed(CrawlerUrlFailedPayload),
    /// `crawler_stopped`.
    CrawlerStopped(CrawlerLifecyclePayload),
    /// `crawler_cancelled`.
    CrawlerCancelled(CrawlerLifecyclePayload),
    /// `crawler_finished`.
    CrawlerFinished(CrawlerLifecyclePayload),
    /// `crawler_search_ready`.
    CrawlerSearchReady(CrawlerSearchPayload),
    /// `crawler_search_failed`.
    CrawlerSearchFailed(CrawlerSearchPayload),
    /// `crawler_updated`.
    CrawlerUpdated(CrawlerUpdatedPayload),
}

impl CrawlerWebhook {
    /// Decode a webhook request body.
    ///
    /// ```no_run
    /// use scrapfly_sdk::result::crawler::CrawlerWebhook;
    ///
    /// # fn handle(body: &[u8]) -> Result<(), serde_json::Error> {
    /// match CrawlerWebhook::from_slice(body)? {
    ///     CrawlerWebhook::CrawlerFinished(payload) => {
    ///         println!("visited {}", payload.common.state.urls_visited);
    ///     }
    ///     CrawlerWebhook::CrawlerUpdated(payload) => {
    ///         println!("re-indexed {}", payload.documents.updated.len());
    ///     }
    ///     other => println!("{:?}", other.event().as_str()),
    /// }
    /// # Ok(()) }
    /// ```
    pub fn from_slice(body: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(body)
    }

    /// The event that produced this delivery.
    pub fn event(&self) -> CrawlerWebhookEvent {
        match self {
            Self::CrawlerStarted(_) => CrawlerWebhookEvent::CrawlerStarted,
            Self::CrawlerUrlVisited(_) => CrawlerWebhookEvent::CrawlerUrlVisited,
            Self::CrawlerUrlSkipped(_) => CrawlerWebhookEvent::CrawlerUrlSkipped,
            Self::CrawlerUrlDiscovered(_) => CrawlerWebhookEvent::CrawlerUrlDiscovered,
            Self::CrawlerUrlFailed(_) => CrawlerWebhookEvent::CrawlerUrlFailed,
            Self::CrawlerStopped(_) => CrawlerWebhookEvent::CrawlerStopped,
            Self::CrawlerCancelled(_) => CrawlerWebhookEvent::CrawlerCancelled,
            Self::CrawlerFinished(_) => CrawlerWebhookEvent::CrawlerFinished,
            Self::CrawlerSearchReady(_) => CrawlerWebhookEvent::CrawlerSearchReady,
            Self::CrawlerSearchFailed(_) => CrawlerWebhookEvent::CrawlerSearchFailed,
            Self::CrawlerUpdated(_) => CrawlerWebhookEvent::CrawlerUpdated,
        }
    }

    /// The fields every payload carries, whatever the event.
    pub fn common(&self) -> &CrawlerWebhookCommon {
        match self {
            Self::CrawlerStarted(payload)
            | Self::CrawlerStopped(payload)
            | Self::CrawlerCancelled(payload)
            | Self::CrawlerFinished(payload) => &payload.common,
            Self::CrawlerUrlVisited(payload) => &payload.common,
            Self::CrawlerUrlSkipped(payload) => &payload.common,
            Self::CrawlerUrlDiscovered(payload) => &payload.common,
            Self::CrawlerUrlFailed(payload) => &payload.common,
            Self::CrawlerSearchReady(payload) | Self::CrawlerSearchFailed(payload) => {
                &payload.common
            }
            Self::CrawlerUpdated(payload) => &payload.common,
        }
    }
}
