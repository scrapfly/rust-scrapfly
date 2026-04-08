//! Error types — 1:1 port of `sdk/go/errors.go` + `crawler.go::parseAPIError`.

use serde::Deserialize;
use thiserror::Error;

/// Structured API error envelope — the JSON shape returned by Scrapfly when
/// a call fails (both `/scrape` `{result: {error: ...}}` envelopes and the
/// generic `{message, code, error_id, http_code}` shape).
#[derive(Debug, Clone, Default)]
pub struct ApiError {
    /// Human-readable error message.
    pub message: String,
    /// Error code identifier (e.g. `ERR::SCRAPE::NETWORK_ERROR`).
    pub code: String,
    /// HTTP status code from the response.
    pub http_status: u16,
    /// Documentation URL.
    pub documentation_url: String,
    /// Hint text (SDK-supplied, context-sensitive).
    pub hint: String,
    /// Retry-After in milliseconds, parsed from the HTTP header.
    pub retry_after_ms: u64,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "API Error: {} (code: {}, status: {}, docs: {})",
            self.message, self.code, self.http_status, self.documentation_url
        )?;
        if self.retry_after_ms > 0 {
            write!(f, ", retry_after_ms: {}", self.retry_after_ms)?;
        }
        Ok(())
    }
}

/// All errors raised by `scrapfly-sdk`.
#[derive(Debug, Error)]
pub enum ScrapflyError {
    /// Transport-level failure (connect, TLS, timeout …).
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    /// JSON (de)serialization failure.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Invalid configuration (builder validation).
    #[error("config: {0}")]
    Config(String),
    /// Invalid or empty API key.
    #[error("invalid key, must be a non-empty string")]
    BadApiKey,
    /// Structured API error envelope.
    #[error("api error [{}] {}", .0.code, .0.message)]
    Api(ApiError),
    /// 4xx from Scrapfly itself.
    #[error("API http client error: {0}")]
    ApiClient(ApiError),
    /// 5xx from Scrapfly itself.
    #[error("API http server error: {0}")]
    ApiServer(ApiError),
    /// 4xx from the upstream target.
    #[error("upstream http client error: {0}")]
    UpstreamClient(ApiError),
    /// 5xx from the upstream target.
    #[error("upstream http server error: {0}")]
    UpstreamServer(ApiError),
    /// Rate limited (HTTP 429).
    #[error("too many requests: {0}")]
    TooManyRequests(ApiError),
    /// Quota exhausted.
    #[error("quota limit reached: {0}")]
    QuotaLimitReached(ApiError),
    /// Scrape failed with an `ERR::SCRAPE::*` status.
    #[error("scrape failed: {0}")]
    ScrapeFailed(ApiError),
    /// Proxy failure (`ERR::PROXY::*`).
    #[error("proxy error: {0}")]
    ProxyFailed(ApiError),
    /// Anti-bot bypass failure (`ERR::ASP::*`).
    #[error("ASP bypass error: {0}")]
    AspBypassFailed(ApiError),
    /// Schedule error.
    #[error("schedule error: {0}")]
    ScheduleFailed(ApiError),
    /// Webhook delivery error.
    #[error("webhook error: {0}")]
    WebhookFailed(ApiError),
    /// Session error.
    #[error("session error: {0}")]
    SessionFailed(ApiError),
    /// Screenshot API error.
    #[error("screenshot API error: {0}")]
    ScreenshotApiFailed(ApiError),
    /// Extraction API error.
    #[error("extraction API error: {0}")]
    ExtractionApiFailed(ApiError),
    /// Crawler API error.
    #[error("crawler error: {0}")]
    CrawlerFailed(ApiError),
    /// Unhandled API error response.
    #[error("unhandled API error response: {0}")]
    UnhandledApiResponse(ApiError),
    /// `Crawl` helper called before `start()`.
    #[error("crawler not started, call start() first")]
    CrawlerNotStarted,
    /// `Crawl::start()` called twice.
    #[error("crawler already started")]
    CrawlerAlreadyStarted,
    /// `Crawl::wait()` observed CANCELLED terminal state.
    #[error("crawler was cancelled")]
    CrawlerCancelled,
    /// `Crawl::wait()` exceeded the caller's deadline.
    #[error("crawler wait timed out")]
    CrawlerTimeout,
    /// Server returned a content-type the SDK didn't expect.
    #[error("unexpected response format: {0}")]
    UnexpectedResponseFormat(String),
    /// Invalid content type for this operation.
    #[error("invalid content type for this operation: {0}")]
    ContentType(String),
    /// I/O failure (example: save screenshot to disk).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Deserialize, Default)]
struct ErrorEnvelope {
    #[serde(default)]
    message: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    #[allow(dead_code)]
    error_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    http_code: u16,
}

/// Build a [`ScrapflyError`] from a non-2xx HTTP response.
///
/// Ports the switch logic from `sdk/go/client.go::handleAPIErrorResponse` +
/// `sdk/go/crawler.go::handleCrawlerErrorResponse`: categorizes the error based
/// on HTTP status + `code` field in the JSON envelope, surfaces the right
/// sentinel variant, attaches a contextual hint, and parses `Retry-After`.
pub fn from_response(
    status: u16,
    body: &[u8],
    retry_after_ms: u64,
    is_crawler: bool,
) -> ScrapflyError {
    let envelope: ErrorEnvelope = serde_json::from_slice(body).unwrap_or_default();
    let msg = if envelope.message.is_empty() {
        format!("API returned status {}", status)
    } else {
        envelope.message.clone()
    };
    let mut err = ApiError {
        message: msg,
        code: envelope.code.clone(),
        http_status: status,
        documentation_url: String::new(),
        hint: String::new(),
        retry_after_ms,
    };

    // HTTP-status-based hint + early dispatch.
    match status {
        401 => err.hint = "Provide a valid API key via ?key=... or Bearer token.".into(),
        429 => {
            err.hint =
                "Back off and retry after the indicated delay, or reduce concurrency/scope.".into();
            return ScrapflyError::TooManyRequests(err);
        }
        422 => {
            let body_str = String::from_utf8_lossy(body);
            if body_str.contains("SCREENSHOT") {
                err.hint =
                    "Check screenshot parameters (format/capture/resolution) and upstream site readiness."
                        .into();
                return ScrapflyError::ScreenshotApiFailed(err);
            }
            if body_str.contains("EXTRACTION") {
                err.hint =
                    "Check content_type, body encoding, and template/prompt validity.".into();
                return ScrapflyError::ExtractionApiFailed(err);
            }
        }
        _ => {}
    }

    // Crawler-resource errors get their own bucket.
    if is_crawler && envelope.code.contains("::CRAWLER::") {
        return ScrapflyError::CrawlerFailed(err);
    }

    // Code-based dispatch (`ERR::RESOURCE::*`).
    if let Some(resource) = envelope.code.split("::").nth(1) {
        match resource {
            "SCRAPE" => return ScrapflyError::ScrapeFailed(err),
            "PROXY" => return ScrapflyError::ProxyFailed(err),
            "ASP" => return ScrapflyError::AspBypassFailed(err),
            "SCHEDULE" => return ScrapflyError::ScheduleFailed(err),
            "WEBHOOK" => return ScrapflyError::WebhookFailed(err),
            "SESSION" => return ScrapflyError::SessionFailed(err),
            "THROTTLE" => return ScrapflyError::TooManyRequests(err),
            "QUOTA" => return ScrapflyError::QuotaLimitReached(err),
            "CRAWLER" => return ScrapflyError::CrawlerFailed(err),
            _ => {}
        }
    }

    // HTTP-status-based fallback.
    match status {
        400..=499 => ScrapflyError::ApiClient(err),
        500..=599 => ScrapflyError::ApiServer(err),
        _ => ScrapflyError::UnhandledApiResponse(err),
    }
}

/// Parse the `Retry-After` header value into milliseconds.
/// Supports integer seconds; HTTP-date is best-effort not parsed (returns 0).
pub(crate) fn parse_retry_after(value: Option<&str>) -> u64 {
    match value {
        Some(v) => v
            .trim()
            .parse::<u64>()
            .map(|secs| secs.saturating_mul(1000))
            .unwrap_or(0),
        None => 0,
    }
}
