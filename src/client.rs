//! HTTP client for the Scrapfly API.
//!
//! Built on `reqwest` with `rustls`. Single shared [`reqwest::Client`]
//! re-used across every call.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{Stream, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, USER_AGENT};
use reqwest::{Method, Response, Url};

use crate::config::crawler::CrawlerConfig;
use crate::config::crawler::{REFRESH_MAX_INTERVAL, REFRESH_MIN_INTERVAL};
use crate::config::extraction::ExtractionConfig;
use crate::config::scrape::ScrapeConfig;
use crate::config::screenshot::ScreenshotConfig;
use crate::enums::HttpMethod;
use crate::error::{from_response, parse_retry_after, ApiError, ScrapflyError};
use crate::monitoring::{
    CloudBrowserMonitoringOptions, MonitoringDataFormat, MonitoringMetricsOptions,
    MonitoringTargetMetricsOptions,
};
use crate::result::account::{AccountData, VerifyApiKeyResult};
use crate::result::classify::{ClassifyRequest, ClassifyResult};
use crate::result::crawler::{
    CrawlerArtifact, CrawlerArtifactType, CrawlerContents, CrawlerPromptDone, CrawlerPromptEvent,
    CrawlerPromptSource, CrawlerRefreshEntry, CrawlerRefreshState, CrawlerSearchResponse,
    CrawlerStartResponse, CrawlerStatus, CrawlerUrls,
};
use crate::result::extraction::ExtractionResult;
use crate::result::scrape::{ResultData, ScrapeResult};
use crate::result::screenshot::{ScreenshotMetadata, ScreenshotResult};

const DEFAULT_HOST: &str = "https://api.scrapfly.io";
const DEFAULT_CLOUD_BROWSER_HOST: &str = "https://browser.scrapfly.io";
const SDK_USER_AGENT: &str = "Scrapfly-Rust-SDK";
const DEFAULT_RETRIES: usize = 3;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(150);
/// Deadline for the `/crawl/prompt` SSE exchange. It has to clear the API's
/// own 165s ceiling (retrieval plus up to 150s of generation), which
/// [`DEFAULT_TIMEOUT`] does not.
const CRAWL_PROMPT_STREAM_TIMEOUT: Duration = Duration::from_secs(180);

/// Request-inspection callback. Fires right before `send()`.
///
/// Used by the integration harness to record the outgoing method/URL/headers
/// without wrapping the `reqwest::Client` in a middleware layer.
pub type OnRequest = Arc<dyn Fn(&Method, &Url, &HeaderMap) + Send + Sync>;

/// Scrapfly API client. Cheap to `Clone` (the inner `reqwest::Client` is
/// `Arc`'d so all clones share one connection pool).
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    key: String,
    host: String,
    cloud_browser_host: String,
    on_request: Option<OnRequest>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("host", &self.host)
            .field("cloud_browser_host", &self.cloud_browser_host)
            .finish()
    }
}

/// Builder for [`Client`].
#[derive(Default)]
pub struct ClientBuilder {
    api_key: Option<String>,
    host: Option<String>,
    cloud_browser_host: Option<String>,
    timeout: Option<Duration>,
    danger_accept_invalid_certs: bool,
    http_client: Option<reqwest::Client>,
    on_request: Option<OnRequest>,
}

impl ClientBuilder {
    /// Set the API key (required).
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }
    /// Override the API host.
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }
    /// Override the Cloud Browser host (`https://browser.scrapfly.io`).
    pub fn cloud_browser_host(mut self, host: impl Into<String>) -> Self {
        self.cloud_browser_host = Some(host.into());
        self
    }
    /// Override the HTTP timeout (default 150s).
    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = Some(t);
        self
    }
    /// Accept invalid TLS certificates (tests / self-signed dev hosts).
    pub fn danger_accept_invalid_certs(mut self, v: bool) -> Self {
        self.danger_accept_invalid_certs = v;
        self
    }
    /// Inject a pre-built `reqwest::Client`. Bypasses the timeout /
    /// TLS-verify options.
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.http_client = Some(client);
        self
    }
    /// Install a pre-send request callback (used by the integration runner
    /// to capture SDK-layer attribution without installing middleware).
    pub fn on_request(mut self, cb: OnRequest) -> Self {
        self.on_request = Some(cb);
        self
    }
    /// Build the client.
    pub fn build(self) -> Result<Client, ScrapflyError> {
        let key = self.api_key.ok_or(ScrapflyError::BadApiKey)?;
        if key.is_empty() {
            return Err(ScrapflyError::BadApiKey);
        }

        let http = if let Some(c) = self.http_client {
            c
        } else {
            let mut builder = reqwest::Client::builder()
                .timeout(self.timeout.unwrap_or(DEFAULT_TIMEOUT))
                .user_agent(SDK_USER_AGENT);
            if self.danger_accept_invalid_certs {
                builder = builder.danger_accept_invalid_certs(true);
            }
            builder.build().map_err(ScrapflyError::Transport)?
        };

        Ok(Client {
            http,
            key,
            host: self.host.unwrap_or_else(|| DEFAULT_HOST.to_string()),
            cloud_browser_host: self
                .cloud_browser_host
                .unwrap_or_else(|| DEFAULT_CLOUD_BROWSER_HOST.to_string()),
            on_request: self.on_request,
        })
    }
}

impl Client {
    /// Start a new [`ClientBuilder`].
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Return the configured API key.
    pub fn api_key(&self) -> &str {
        &self.key
    }

    /// Return the configured API host.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Return the configured Cloud Browser host.
    pub fn cloud_browser_host(&self) -> &str {
        &self.cloud_browser_host
    }

    /// Build a URL by joining `path` onto the configured host.
    /// Crate-internal shim over `build_url`, used by `schedule.rs` to share
    /// the same auth + host wiring as the rest of the SDK without exposing
    /// the helper publicly.
    pub(crate) fn build_url_public(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<Url, ScrapflyError> {
        self.build_url(path, query)
    }

    /// Crate-internal shim over `send_simple` — same rationale as
    /// `build_url_public`.
    pub(crate) async fn send_simple_public(
        &self,
        method: Method,
        url: Url,
        headers: Option<HeaderMap>,
        body: Option<Vec<u8>>,
    ) -> Result<Response, ScrapflyError> {
        self.send_simple(method, url, headers, body).await
    }

    fn build_url(&self, path: &str, query: &[(String, String)]) -> Result<Url, ScrapflyError> {
        let mut u = Url::parse(&format!("{}{}", self.host, path))
            .map_err(|e| ScrapflyError::Config(format!("invalid url: {}", e)))?;
        {
            let mut pairs = u.query_pairs_mut();
            pairs.append_pair("key", &self.key);
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        Ok(u)
    }

    /// Verify the API key by hitting `/account`.
    pub async fn verify_api_key(&self) -> Result<VerifyApiKeyResult, ScrapflyError> {
        let url = self.build_url("/account", &[])?;
        let resp = self.send_simple(Method::GET, url, None, None).await?;
        Ok(VerifyApiKeyResult {
            valid: resp.status().is_success(),
        })
    }

    /// Fetch account info.
    pub async fn account(&self) -> Result<AccountData, ScrapflyError> {
        let url = self.build_url("/account", &[])?;
        let resp = self.send_simple(Method::GET, url, None, None).await?;
        let (status, _headers, body) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Classify an already-fetched HTTP response for anti-bot blocking.
    ///
    /// Runs the same detection pipeline used by every live Scrapfly scrape
    /// against a response you already have (from your own proxy, cache, etc).
    /// 1 API credit per successful call. See
    /// <https://scrapfly.io/docs/scrape-api/classify>.
    pub async fn classify(&self, req: &ClassifyRequest) -> Result<ClassifyResult, ScrapflyError> {
        if req.url.is_empty() {
            return Err(ScrapflyError::Config("classify: url is required".into()));
        }
        if !(100..=599).contains(&req.status_code) {
            return Err(ScrapflyError::Config(
                "classify: status_code must be in [100, 599]".into(),
            ));
        }

        let url = self.build_url("/classify", &[])?;
        let body = serde_json::to_vec(req)
            .map_err(|e| ScrapflyError::Config(format!("marshal classify request: {}", e)))?;

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

        let resp = self
            .send_simple(Method::POST, url, Some(headers), Some(body))
            .await?;
        let (status, _headers, bytes) = read_response(resp).await?;
        if status >= 400 {
            return Err(from_response(status, &bytes, 0, false));
        }
        let out: ClassifyResult = serde_json::from_slice(&bytes)
            .map_err(|e| ScrapflyError::Config(format!("decode classify response: {}", e)))?;
        Ok(out)
    }

    // ── Monitoring API (Enterprise+ plan only) ──────────────────────
    // The Monitoring API exposes per-product aggregates and per-target
    // timeseries. Web Scraping / Screenshot / Extraction / Crawler share
    // one shape (request-based) but live under different URL prefixes;
    // Cloud Browser is session-based and exposes a distinct shape.
    // See <https://scrapfly.io/docs/monitoring#api>.

    fn build_metrics_pairs(opts: &MonitoringMetricsOptions) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = Vec::new();
        let format = opts.format.unwrap_or(MonitoringDataFormat::Structured);
        pairs.push(("format".into(), format.as_str().into()));
        if let Some(p) = opts.period {
            pairs.push(("period".into(), p.as_str().into()));
        }
        if let Some(ref aggs) = opts.aggregation {
            if !aggs.is_empty() {
                let joined = aggs
                    .iter()
                    .map(|a| a.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                pairs.push(("aggregation".into(), joined));
            }
        }
        if opts.include_webhook {
            pairs.push(("include_webhook".into(), "true".into()));
        }
        pairs
    }

    fn build_target_pairs(
        opts: &MonitoringTargetMetricsOptions,
    ) -> Result<Vec<(String, String)>, ScrapflyError> {
        if opts.domain.is_empty() {
            return Err(ScrapflyError::Config(
                "monitoring target metrics: domain is required".into(),
            ));
        }
        if opts.start.is_some() != opts.end.is_some() {
            return Err(ScrapflyError::Config(
                "monitoring target metrics: start and end must be provided together".into(),
            ));
        }
        let mut pairs: Vec<(String, String)> = Vec::new();
        pairs.push(("domain".into(), opts.domain.clone()));
        pairs.push(("group_subdomain".into(), opts.group_subdomain.to_string()));
        match (&opts.start, &opts.end) {
            (Some(s), Some(e)) => {
                pairs.push(("start".into(), s.clone()));
                pairs.push(("end".into(), e.clone()));
            }
            _ => {
                let period = opts
                    .period
                    .unwrap_or(crate::monitoring::MonitoringPeriod::Last24h);
                pairs.push(("period".into(), period.as_str().into()));
            }
        }
        if opts.include_webhook {
            pairs.push(("include_webhook".into(), "true".into()));
        }
        Ok(pairs)
    }

    async fn fetch_monitoring_json(
        &self,
        path: &str,
        pairs: &[(String, String)],
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = self.build_url(path, pairs)?;
        let resp = self.send_simple(Method::GET, url, None, None).await?;
        let (status, _headers, body) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    // ── Web Scraping API ─────────────────────────────────────────────

    /// Fetch aggregate monitoring metrics for the Web Scraping API.
    pub async fn get_monitoring_metrics(
        &self,
        opts: &MonitoringMetricsOptions,
    ) -> Result<serde_json::Value, ScrapflyError> {
        self.fetch_monitoring_json(
            "/scrape/monitoring/metrics",
            &Self::build_metrics_pairs(opts),
        )
        .await
    }

    /// Fetch per-target monitoring metrics for the Web Scraping API.
    pub async fn get_monitoring_target_metrics(
        &self,
        opts: &MonitoringTargetMetricsOptions,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let pairs = Self::build_target_pairs(opts)?;
        self.fetch_monitoring_json("/scrape/monitoring/metrics/target", &pairs)
            .await
    }

    // ── Screenshot API ───────────────────────────────────────────────

    /// Fetch aggregate monitoring metrics for the Screenshot API.
    pub async fn get_screenshot_monitoring_metrics(
        &self,
        opts: &MonitoringMetricsOptions,
    ) -> Result<serde_json::Value, ScrapflyError> {
        self.fetch_monitoring_json(
            "/screenshot/monitoring/metrics",
            &Self::build_metrics_pairs(opts),
        )
        .await
    }

    /// Fetch per-target monitoring metrics for the Screenshot API.
    pub async fn get_screenshot_monitoring_target_metrics(
        &self,
        opts: &MonitoringTargetMetricsOptions,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let pairs = Self::build_target_pairs(opts)?;
        self.fetch_monitoring_json("/screenshot/monitoring/metrics/target", &pairs)
            .await
    }

    // ── Extraction API ───────────────────────────────────────────────

    /// Fetch aggregate monitoring metrics for the Extraction API.
    pub async fn get_extraction_monitoring_metrics(
        &self,
        opts: &MonitoringMetricsOptions,
    ) -> Result<serde_json::Value, ScrapflyError> {
        self.fetch_monitoring_json(
            "/extraction/monitoring/metrics",
            &Self::build_metrics_pairs(opts),
        )
        .await
    }

    /// Fetch per-target monitoring metrics for the Extraction API.
    pub async fn get_extraction_monitoring_target_metrics(
        &self,
        opts: &MonitoringTargetMetricsOptions,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let pairs = Self::build_target_pairs(opts)?;
        self.fetch_monitoring_json("/extraction/monitoring/metrics/target", &pairs)
            .await
    }

    // ── Crawler API ──────────────────────────────────────────────────

    /// Fetch aggregate monitoring metrics for the Crawler API.
    pub async fn get_crawler_monitoring_metrics(
        &self,
        opts: &MonitoringMetricsOptions,
    ) -> Result<serde_json::Value, ScrapflyError> {
        self.fetch_monitoring_json(
            "/crawl/monitoring/metrics",
            &Self::build_metrics_pairs(opts),
        )
        .await
    }

    /// Fetch per-target monitoring metrics for the Crawler API.
    pub async fn get_crawler_monitoring_target_metrics(
        &self,
        opts: &MonitoringTargetMetricsOptions,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let pairs = Self::build_target_pairs(opts)?;
        self.fetch_monitoring_json("/crawl/monitoring/metrics/target", &pairs)
            .await
    }

    // ── Cloud Browser API (session-based, distinct shape) ────────────

    /// Fetch aggregate monitoring metrics for the Cloud Browser API.
    pub async fn get_browser_monitoring_metrics(
        &self,
        opts: &CloudBrowserMonitoringOptions,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let pairs = Self::build_browser_pairs(opts)?;
        self.fetch_monitoring_json("/browser/monitoring/metrics", &pairs)
            .await
    }

    /// Fetch monitoring time-series for the Cloud Browser API.
    pub async fn get_browser_monitoring_timeseries(
        &self,
        opts: &CloudBrowserMonitoringOptions,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let pairs = Self::build_browser_pairs(opts)?;
        self.fetch_monitoring_json("/browser/monitoring/metrics/timeseries", &pairs)
            .await
    }

    fn build_browser_pairs(
        opts: &CloudBrowserMonitoringOptions,
    ) -> Result<Vec<(String, String)>, ScrapflyError> {
        if opts.start.is_some() != opts.end.is_some() {
            return Err(ScrapflyError::Config(
                "cloud browser monitoring: start and end must be provided together".into(),
            ));
        }
        let mut pairs: Vec<(String, String)> = Vec::new();
        match (&opts.start, &opts.end) {
            (Some(s), Some(e)) => {
                pairs.push(("start".into(), s.clone()));
                pairs.push(("end".into(), e.clone()));
            }
            _ => {
                if let Some(p) = opts.period {
                    pairs.push(("period".into(), p.as_str().into()));
                }
            }
        }
        if let Some(ref pool) = opts.proxy_pool {
            pairs.push(("proxy_pool".into(), pool.clone()));
        }
        Ok(pairs)
    }

    /// Scrape a URL.
    pub async fn scrape(&self, config: &ScrapeConfig) -> Result<ScrapeResult, ScrapflyError> {
        let pairs = config.to_query_pairs()?;
        let url = self.build_url("/scrape", &pairs)?;
        let method = match config.method {
            Some(m) => Method::from_bytes(m.as_str().as_bytes())
                .map_err(|e| ScrapflyError::Config(format!("invalid method: {}", e)))?,
            None => Method::GET,
        };
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let body = config.body.clone();
        let resp = self
            .send_with_retry(method, url, Some(headers), body.map(|b| b.into_bytes()))
            .await?;
        let (status, _h, body_bytes) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body_bytes, 0, false));
        }
        // HEAD has no body per HTTP spec, so the Scrapfly API returns a 200
        // with an empty body — there's no JSON envelope to parse. Synthesize
        // a minimal ScrapeResult so callers still get a typed response with
        // status_code=200 and an empty content string. Matches Python SDK
        // behavior, which tolerates an empty body_handler read on HEAD.
        if matches!(config.method, Some(HttpMethod::Head)) && body_bytes.is_empty() {
            return Ok(ScrapeResult {
                uuid: String::new(),
                config: serde_json::Value::Null,
                context: serde_json::Value::Null,
                result: ResultData {
                    status_code: 200,
                    success: true,
                    ..Default::default()
                },
            });
        }
        let mut result: ScrapeResult = serde_json::from_slice(&body_bytes)?;
        // Upstream failure handling: the Scrapfly API call itself may succeed
        // (HTTP 200) while the *target* site returned a failure. In that case
        // result.result.success is false and we must surface it as an error
        // variant so callers can `match` on it. Mirrors the Go SDK behavior
        // in `sdk/go/client.go::checkResult` (4xx → UpstreamClient,
        // 5xx → UpstreamServer).
        if !result.result.success {
            let (err_code, err_message, err_doc, err_retryable) = match &result.result.error {
                Some(e) => (
                    e.code.clone(),
                    e.message.clone(),
                    e.doc_url.clone(),
                    e.retryable,
                ),
                None => (
                    result.result.status.clone(),
                    format!(
                        "scrape failed with status_code={}",
                        result.result.status_code
                    ),
                    String::new(),
                    false,
                ),
            };
            let api_err = ApiError {
                code: err_code,
                message: err_message,
                http_status: result.result.status_code,
                documentation_url: err_doc,
                hint: String::new(),
                retry_after_ms: 0,
                retryable: err_retryable,
                reason: String::new(),
            };
            let sc = result.result.status_code;
            if (400..500).contains(&sc) {
                return Err(ScrapflyError::UpstreamClient(api_err));
            }
            if (500..600).contains(&sc) {
                return Err(ScrapflyError::UpstreamServer(api_err));
            }
            // Unknown status code (e.g. 0, timeouts) — fall through to generic
            // Api error rather than silently returning a failed result.
            return Err(ScrapflyError::Api(api_err));
        }
        // Transparent large-object handling: when a scrape response is too
        // large, the engine offloads the body to a signed URL and sets
        // `format=clob|blob`, stashing the URL in `content`. The SDK must
        // auto-fetch and surface the final bytes + a user-friendly format
        // marker (clob→text, blob→binary). Mirrors `sdk/go/client.go::handleLargeObjects`.
        if result.result.success && result.result.status == "DONE" {
            let fmt = result.result.format.as_str();
            if fmt == "clob" || fmt == "blob" {
                let (new_content, new_format) =
                    self.fetch_large_object(&result.result.content, fmt).await?;
                result.result.content = new_content;
                result.result.format = new_format;
            }
        }
        Ok(result)
    }

    /// Fetch an offloaded large-object body from its signed URL, re-attaching
    /// the API key as a query param. Returns `(content, format)`:
    /// `clob → ("…text…", "text")`, `blob → ("…bytes as lossy utf8…", "binary")`.
    async fn fetch_large_object(
        &self,
        content_url: &str,
        format: &str,
    ) -> Result<(String, String), ScrapflyError> {
        let mut url = Url::parse(content_url)
            .map_err(|e| ScrapflyError::Config(format!("invalid large-object url: {}", e)))?;
        // Append the API key without clobbering existing query params.
        {
            let existing: Vec<(String, String)> = url
                .query_pairs()
                .filter(|(k, _)| k != "key")
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            let mut qs = url.query_pairs_mut();
            qs.clear();
            for (k, v) in existing {
                qs.append_pair(&k, &v);
            }
            qs.append_pair("key", self.api_key());
        }
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let resp = self
            .send_with_retry(Method::GET, url, Some(headers), None)
            .await?;
        let (status, _h, body) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        let new_format = match format {
            "clob" => "text",
            "blob" => "binary",
            _ => {
                return Err(ScrapflyError::Config(format!(
                    "unsupported large-object format: {}",
                    format
                )))
            }
        };
        // For blob (binary PDF, image, etc.) we use from_utf8_lossy to
        // preserve the raw bytes in the `content` string field, matching
        // the Go/Python SDKs' behavior.
        let content = String::from_utf8_lossy(&body).into_owned();
        Ok((content, new_format.to_string()))
    }

    /// Concurrent-scrape stream. Emits results in completion order.
    pub fn concurrent_scrape<'a, I>(
        &'a self,
        configs: I,
        concurrency_limit: usize,
    ) -> impl Stream<Item = Result<ScrapeResult, ScrapflyError>> + 'a
    where
        I: IntoIterator<Item = ScrapeConfig> + 'a,
        <I as IntoIterator>::IntoIter: 'a,
    {
        let limit = if concurrency_limit == 0 {
            5
        } else {
            concurrency_limit
        };
        futures_util::stream::iter(
            configs
                .into_iter()
                .map(move |cfg| async move { self.scrape(&cfg).await }),
        )
        .buffer_unordered(limit)
    }

    /// POST /scrape/batch: scrape up to 100 URLs and stream results
    /// back as each scrape completes. Returns an async stream where
    /// each item is `(correlation_id, Result<ScrapeResult, ScrapflyError>)`.
    ///
    /// Results arrive OUT OF ORDER — whichever scrape finishes first
    /// is yielded first. Every `ScrapeConfig` MUST carry a unique
    /// `correlation_id`; missing / duplicate values are caught
    /// client-side before the request is sent.
    ///
    /// Batch-level failures (plan gate, insufficient concurrency,
    /// validation) surface as the outer `Err(ScrapflyError)` returned
    /// from the `await` — the stream is only created after the
    /// batch request succeeds.
    pub async fn scrape_batch(
        &self,
        configs: &[ScrapeConfig],
    ) -> Result<impl Stream<Item = (String, crate::batch::BatchOutcome)>, ScrapflyError> {
        self.scrape_batch_with_options(configs, crate::batch::BatchOptions::default())
            .await
    }

    /// Like `scrape_batch` but with explicit `BatchOptions`
    /// (msgpack wire format, etc.).
    pub async fn scrape_batch_with_options(
        &self,
        configs: &[ScrapeConfig],
        opts: crate::batch::BatchOptions,
    ) -> Result<impl Stream<Item = (String, crate::batch::BatchOutcome)>, ScrapflyError> {
        use crate::batch::{
            api_error_from_part, build_proxified_response, decode_part_body, parts_from_response,
            BatchOutcome,
        };

        if configs.is_empty() {
            return Err(ScrapflyError::Config(
                "scrape_batch: configs is empty".into(),
            ));
        }

        if configs.len() > 100 {
            return Err(ScrapflyError::Config(format!(
                "scrape_batch: max 100 configs per batch (got {})",
                configs.len()
            )));
        }

        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut body_configs: Vec<HashMap<String, String>> = Vec::with_capacity(configs.len());

        for (i, cfg) in configs.iter().enumerate() {
            let correlation_id = cfg.correlation_id.clone().ok_or_else(|| {
                ScrapflyError::Config(format!(
                    "scrape_batch: configs[{}] is missing correlation_id (required for matching streamed parts)",
                    i
                ))
            })?;

            if let Some(prev) = seen.get(&correlation_id) {
                return Err(ScrapflyError::Config(format!(
                    "scrape_batch: correlation_id {:?} reused by configs[{}] and configs[{}]",
                    correlation_id, prev, i
                )));
            }

            seen.insert(correlation_id.clone(), i);

            let pairs = cfg.to_query_pairs()?;
            let mut entry: HashMap<String, String> = HashMap::with_capacity(pairs.len());

            for (k, v) in pairs {
                if k == "key" {
                    continue;
                }

                entry.insert(k, v);
            }

            body_configs.push(entry);
        }

        let body = serde_json::json!({ "configs": body_configs });
        let body_bytes = serde_json::to_vec(&body)?;

        let mut url = Url::parse(&self.host)
            .map_err(|e| ScrapflyError::Config(format!("invalid host: {}", e)))?;
        url.set_path("/scrape/batch");
        url.query_pairs_mut().append_pair("key", &self.key);

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(opts.format.accept_header()),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static(SDK_USER_AGENT));

        let method = Method::POST;

        if let Some(cb) = &self.on_request {
            cb(&method, &url, &headers);
        }

        let resp = self
            .http
            .request(method, url)
            .headers(headers)
            .body(body_bytes)
            .send()
            .await
            .map_err(|e| ScrapflyError::Config(format!("scrape_batch: send: {}", e)))?;

        let status = resp.status().as_u16();

        if status != 200 {
            let body_bytes = resp.bytes().await.unwrap_or_default();

            return Err(from_response(status, &body_bytes, 0, false));
        }

        let parts_stream = parts_from_response(resp)?;

        Ok(parts_stream.map(|part_r| match part_r {
            Ok(part) => {
                let correlation_id = part
                    .headers
                    .get("x-scrapfly-correlation-id")
                    .cloned()
                    .unwrap_or_default();

                // Proxified-response parts: the part body is the raw
                // upstream bytes, not a JSON envelope. Surface as a
                // BatchProxifiedResponse rather than attempting to
                // decode the body as JSON.
                if part
                    .headers
                    .get("x-scrapfly-proxified")
                    .map(|v| v == "true")
                    .unwrap_or(false)
                {
                    let prox = build_proxified_response(part);
                    return (correlation_id, BatchOutcome::Proxified(prox));
                }

                // API-generated error parts carry an error body instead of
                // the scrape envelope — surface them as typed errors.
                if let Some(err) = api_error_from_part(&part) {
                    return (correlation_id, BatchOutcome::Err(err));
                }

                match decode_part_body::<ScrapeResult>(&part) {
                    Ok(r) => (correlation_id, BatchOutcome::Scrape(r)),
                    Err(e) => (correlation_id, BatchOutcome::Err(e)),
                }
            }
            Err(e) => (String::new(), BatchOutcome::Err(e)),
        }))
    }

    /// Scrape a URL with `proxified_response=true`, returning the raw
    /// upstream `reqwest::Response` (target's status, headers, body).
    ///
    /// Unlike [`scrape()`], no JSON parsing occurs — the response body is
    /// the target page's raw content. Scrapfly metadata is available on
    /// the `X-Scrapfly-*` response headers (`Api-Cost`, `Content-Format`,
    /// `Log`, etc.).
    ///
    /// Automatically forces `proxified_response=true` regardless of the
    /// config's field value.
    pub async fn scrape_proxified(
        &self,
        config: &ScrapeConfig,
    ) -> Result<reqwest::Response, ScrapflyError> {
        let mut cfg = config.clone();
        cfg.proxified_response = true;
        let pairs = cfg.to_query_pairs()?;
        let url = self.build_url("/scrape", &pairs)?;
        let method = match cfg.method {
            Some(m) => Method::from_bytes(m.as_str().as_bytes())
                .map_err(|e| ScrapflyError::Config(format!("invalid method: {}", e)))?,
            None => Method::GET,
        };
        let body = cfg.body.clone();
        let resp = self
            .send_with_retry(method, url, None, body.map(|b| b.into_bytes()))
            .await?;
        // Error restoration: if X-Scrapfly-Reject-Code is present, the
        // scrape failed. Return a typed error so callers get the same
        // interface as non-proxified mode.
        if let Some(reject_code) = resp.headers().get("x-scrapfly-reject-code") {
            let code = reject_code.to_str().unwrap_or("").to_string();
            let desc = resp
                .headers()
                .get("x-scrapfly-reject-description")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let retryable = resp
                .headers()
                .get("x-scrapfly-reject-retryable")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("false")
                == "true";
            let retry_after_ms: u64 = if retryable {
                resp.headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0)
                    * 1000 // Retry-After header is in seconds
            } else {
                0
            };
            let status = resp.status().as_u16();
            let doc = resp
                .headers()
                .get("x-scrapfly-reject-doc")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            return Err(ScrapflyError::Api(crate::error::ApiError {
                code,
                message: format!("Proxified scrape error: {}", desc),
                http_status: status,
                documentation_url: doc,
                hint: String::new(),
                retry_after_ms,
                retryable,
                reason: String::new(),
            }));
        }
        Ok(resp)
    }

    /// Screenshot a URL.
    pub async fn screenshot(
        &self,
        config: &ScreenshotConfig,
    ) -> Result<ScreenshotResult, ScrapflyError> {
        let pairs = config.to_query_pairs()?;
        let url = self.build_url("/screenshot", &pairs)?;
        let resp = self.send_with_retry(Method::GET, url, None, None).await?;
        let (status, headers, body) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        let content_type = headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream");
        let ext = content_type
            .split('/')
            .nth(1)
            .and_then(|s| s.split(';').next())
            .unwrap_or("bin")
            .to_string();
        let upstream_status_code: u16 = headers
            .get("x-scrapfly-upstream-http-code")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let upstream_url = headers
            .get("x-scrapfly-upstream-url")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        Ok(ScreenshotResult {
            image: body,
            metadata: ScreenshotMetadata {
                extension_name: ext,
                upstream_status_code,
                upstream_url,
            },
        })
    }

    /// Run AI extraction on a document.
    pub async fn extract(
        &self,
        config: &ExtractionConfig,
    ) -> Result<ExtractionResult, ScrapflyError> {
        let pairs = config.to_query_pairs()?;
        let url = self.build_url("/extraction", &pairs)?;
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(&config.content_type)
                .map_err(|e| ScrapflyError::Config(format!("invalid content-type: {}", e)))?,
        );
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        if let Some(fmt) = config.document_compression_format {
            headers.insert(
                "content-encoding",
                HeaderValue::from_str(fmt.as_str())
                    .map_err(|e| ScrapflyError::Config(format!("invalid encoding: {}", e)))?,
            );
        }
        let resp = self
            .send_with_retry(Method::POST, url, Some(headers), Some(config.body.clone()))
            .await?;
        let (status, _h, body_bytes) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body_bytes, 0, false));
        }
        Ok(serde_json::from_slice(&body_bytes)?)
    }

    // ==============================================================================
    // Crawler methods
    // ==============================================================================

    /// Schedule a new crawler job.
    ///
    /// `POST /crawl` accepts two body formats:
    ///   * `application/json` — the entire crawler configuration as JSON.
    ///     Used for seed-URL crawls and `remote_url_list` crawls.
    ///   * `multipart/form-data` — a `config` JSON part and a `urls` text
    ///     part (one URL per line). Used only when the caller provides an
    ///     in-memory `url_list`, so the URLs are uploaded as a streamed
    ///     file payload instead of inlined into the JSON body.
    pub async fn start_crawl(
        &self,
        config: &CrawlerConfig,
    ) -> Result<CrawlerStartResponse, ScrapflyError> {
        let url = self.build_url("/crawl", &[])?;
        let mut headers = HeaderMap::new();
        let body: Vec<u8> = if !config.url_list.is_empty() {
            let (mp_body, ct) = config.to_multipart_body()?;
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_str(&ct).map_err(|e| {
                    ScrapflyError::Config(format!("invalid multipart content-type: {e}"))
                })?,
            );
            mp_body
        } else {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            config.to_json_body()?
        };
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let resp = self
            .send_with_retry(Method::POST, url, Some(headers), Some(body))
            .await?;
        let (status, _h, body_bytes) = read_response(resp).await?;
        if status != 200 && status != 201 {
            return Err(from_response(status, &body_bytes, 0, true));
        }
        let parsed: CrawlerStartResponse = serde_json::from_slice(&body_bytes)?;
        if parsed.crawler_uuid.is_empty() {
            return Err(ScrapflyError::UnexpectedResponseFormat(
                "crawler start response missing crawler_uuid".into(),
            ));
        }
        Ok(parsed)
    }

    /// Fetch crawler status.
    pub async fn crawl_status(&self, uuid: &str) -> Result<CrawlerStatus, ScrapflyError> {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        let url = self.build_url(&format!("/crawl/{}/status", uuid), &[])?;
        let resp = self.send_with_retry(Method::GET, url, None, None).await?;
        let (status, _h, body) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body, 0, true));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// List crawled URLs (streaming text endpoint).
    pub async fn crawl_urls(
        &self,
        uuid: &str,
        status_filter: Option<&str>,
        page: u32,
        per_page: u32,
    ) -> Result<CrawlerUrls, ScrapflyError> {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        let page = if page == 0 { 1 } else { page };
        let per_page = if per_page == 0 { 100 } else { per_page };
        let status_hint = status_filter.unwrap_or("visited");
        let mut pairs: Vec<(String, String)> = vec![
            ("page".into(), page.to_string()),
            ("per_page".into(), per_page.to_string()),
        ];
        if let Some(s) = status_filter {
            pairs.push(("status".into(), s.to_string()));
        }
        let url = self.build_url(&format!("/crawl/{}/urls", uuid), &pairs)?;
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("text/plain, application/json"),
        );
        let resp = self
            .send_with_retry(Method::GET, url, Some(headers), None)
            .await?;
        let (status, resp_headers, body) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body, 0, true));
        }
        let ct = resp_headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if ct.contains("application/json") {
            return Err(ScrapflyError::UnexpectedResponseFormat(format!(
                "GET /crawl/{}/urls returned JSON on a 200 response (expected text/plain)",
                uuid
            )));
        }
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| ScrapflyError::UnexpectedResponseFormat(format!("invalid utf8: {}", e)))?;
        Ok(CrawlerUrls::from_text(
            body_str,
            status_hint,
            page,
            per_page,
        ))
    }

    /// Bulk `GET /crawl/{uuid}/contents` in JSON mode.
    pub async fn crawl_contents_json(
        &self,
        uuid: &str,
        format: crate::enums::CrawlerContentFormat,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<CrawlerContents, ScrapflyError> {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        let mut pairs: Vec<(String, String)> = vec![("formats".into(), format.as_str().into())];
        if let Some(l) = limit {
            pairs.push(("limit".into(), l.to_string()));
        }
        if let Some(o) = offset {
            pairs.push(("offset".into(), o.to_string()));
        }
        let url = self.build_url(&format!("/crawl/{}/contents", uuid), &pairs)?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let resp = self
            .send_with_retry(Method::GET, url, Some(headers), None)
            .await?;
        let (status, resp_headers, body) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body, 0, true));
        }
        let ct = resp_headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !ct.contains("application/json") {
            return Err(ScrapflyError::UnexpectedResponseFormat(format!(
                "expected JSON, got Content-Type={}",
                ct
            )));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Plain single-URL `GET /crawl/{uuid}/contents?plain=true`.
    pub async fn crawl_contents_plain(
        &self,
        uuid: &str,
        target_url: &str,
        format: crate::enums::CrawlerContentFormat,
    ) -> Result<String, ScrapflyError> {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        if target_url.is_empty() {
            return Err(ScrapflyError::Config(
                "plain mode requires a single url argument".into(),
            ));
        }
        let pairs: Vec<(String, String)> = vec![
            ("formats".into(), format.as_str().into()),
            ("url".into(), target_url.into()),
            ("plain".into(), "true".into()),
        ];
        let url = self.build_url(&format!("/crawl/{}/contents", uuid), &pairs)?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        let resp = self
            .send_with_retry(Method::GET, url, Some(headers), None)
            .await?;
        let (status, _h, body) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body, 0, true));
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    /// Bulk-batch `POST /crawl/{uuid}/contents/batch`.
    /// Returns `url → format → content` (multipart/related response).
    pub async fn crawl_contents_batch(
        &self,
        uuid: &str,
        urls: &[String],
        formats: &[crate::enums::CrawlerContentFormat],
    ) -> Result<
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
        ScrapflyError,
    > {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        if urls.is_empty() {
            return Err(ScrapflyError::Config("at least one URL is required".into()));
        }
        if urls.len() > 100 {
            return Err(ScrapflyError::Config(format!(
                "batch is limited to 100 URLs per request, got {}",
                urls.len()
            )));
        }
        if formats.is_empty() {
            return Err(ScrapflyError::Config(
                "at least one format is required".into(),
            ));
        }
        let format_strs: Vec<&'static str> = formats.iter().map(|f| f.as_str()).collect();
        let pairs: Vec<(String, String)> = vec![("formats".into(), format_strs.join(","))];
        let url = self.build_url(&format!("/crawl/{}/contents/batch", uuid), &pairs)?;
        let body = urls.join("\n").into_bytes();
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("multipart/related, application/json"),
        );
        let resp = self
            .send_with_retry(Method::POST, url, Some(headers), Some(body))
            .await?;
        let (status, resp_headers, body_bytes) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body_bytes, 0, true));
        }
        let ct = resp_headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if ct.contains("application/json") {
            return Err(ScrapflyError::UnexpectedResponseFormat(
                "CrawlContentsBatch expected multipart/related, got JSON".into(),
            ));
        }
        parse_multipart_related(
            std::str::from_utf8(&body_bytes).unwrap_or(""),
            ct,
            &format_strs,
        )
    }

    /// Search the indexes of one or more crawls and return one merged ranking.
    ///
    /// `POST /crawl/search`: the collection endpoint is the real API and
    /// [`Client::crawl_search`] is sugar over a one-element list, so the two
    /// cannot drift.
    ///
    /// Only crawls started with [`CrawlerConfig::search`] whose index reached
    /// `READY` or `PARTIAL` contribute. The rest come back in
    /// [`CrawlerSearchResponse::skipped`] with a reason and never fail the
    /// call, so inspect that list before concluding a crawl had no match.
    pub async fn crawls_search(
        &self,
        crawl_ids: &[String],
        query: &str,
        opts: Option<&CrawlSearchOptions>,
    ) -> Result<CrawlerSearchResponse, ScrapflyError> {
        validate_crawl_ids(crawl_ids)?;
        if query.is_empty() {
            return Err(ScrapflyError::Config("query cannot be empty".into()));
        }

        let mut body = serde_json::Map::new();
        body.insert("query".into(), serde_json::Value::String(query.to_string()));
        body.insert("crawl_ids".into(), serde_json::to_value(crawl_ids)?);
        if let Some(o) = opts {
            o.apply(&mut body)?;
        }

        let url = self.build_url("/crawl/search", &[])?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let payload = serde_json::to_vec(&serde_json::Value::Object(body))?;
        let resp = self
            .send_with_retry(Method::POST, url, Some(headers), Some(payload))
            .await?;
        let (status, _h, bytes) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &bytes, 0, true));
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Search a single crawl's index.
    ///
    /// `GET /crawl/{uuid}/search` is the documented convenience path; this SDK
    /// reaches the same implementation through the collection endpoint so
    /// single and multi-crawl search cannot answer differently.
    pub async fn crawl_search(
        &self,
        uuid: &str,
        query: &str,
        opts: Option<&CrawlSearchOptions>,
    ) -> Result<CrawlerSearchResponse, ScrapflyError> {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        let ids = [uuid.to_string()];
        self.crawls_search(&ids, query, opts).await
    }

    /// Answer a question from the content of one or more crawls, streaming
    /// the answer as it is generated.
    ///
    /// `POST /crawl/prompt`: the collection endpoint is the real API and
    /// [`Client::crawl_prompt`] is sugar over a one-element list.
    ///
    /// The returned stream yields `Source` frames, then `Token` frames, then
    /// one `Done`. A server-sent `error` frame surfaces as an `Err` item,
    /// which can arrive after tokens were already yielded because generation
    /// fails mid-stream.
    ///
    /// No retry: the request runs a fan-out and a generation, both billable,
    /// so replaying it doubles the bill.
    pub async fn crawls_prompt(
        &self,
        crawl_ids: &[String],
        prompt: &str,
        opts: Option<&CrawlPromptOptions>,
    ) -> Result<impl Stream<Item = Result<CrawlerPromptEvent, ScrapflyError>>, ScrapflyError> {
        validate_crawl_ids(crawl_ids)?;
        if prompt.is_empty() {
            return Err(ScrapflyError::Config("prompt cannot be empty".into()));
        }

        let mut generation = serde_json::Map::new();
        generation.insert("stream".into(), serde_json::Value::Bool(true));
        if let Some(model) = opts.and_then(|o| o.model.as_deref()) {
            generation.insert("model".into(), serde_json::Value::String(model.to_string()));
        }
        let mut body = serde_json::Map::new();
        body.insert(
            "prompt".into(),
            serde_json::Value::String(prompt.to_string()),
        );
        body.insert("crawl_ids".into(), serde_json::to_value(crawl_ids)?);
        body.insert("generation".into(), serde_json::Value::Object(generation));
        if let Some(search) = opts.and_then(|o| o.search.as_ref()) {
            let mut map = serde_json::Map::new();
            search.apply(&mut map)?;
            if !map.is_empty() {
                body.insert("search".into(), serde_json::Value::Object(map));
            }
        }

        let url = self.build_url("/crawl/prompt", &[])?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        let payload = serde_json::to_vec(&serde_json::Value::Object(body))?;
        let resp = self.send_prompt_stream(url, headers, payload).await?;

        let status = resp.status().as_u16();
        if status != 200 {
            let bytes = resp.bytes().await?;
            return Err(from_response(status, &bytes, 0, true));
        }

        Ok(crawler_prompt_stream(resp))
    }

    /// Answer a question from a single crawl's content.
    pub async fn crawl_prompt(
        &self,
        uuid: &str,
        prompt: &str,
        opts: Option<&CrawlPromptOptions>,
    ) -> Result<impl Stream<Item = Result<CrawlerPromptEvent, ScrapflyError>>, ScrapflyError> {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        let ids = [uuid.to_string()];
        self.crawls_prompt(&ids, prompt, opts).await
    }

    /// Run one refresh of an existing crawl immediately, without waiting for
    /// the next scheduled period.
    ///
    /// `POST /crawl/{uuid}/refresh` re-scrapes the crawl's own URLs in place:
    /// same crawler UUID, same artifacts, same search index. Only pages whose
    /// content actually changed are re-indexed and pages that disappeared are
    /// dropped, so everything already pointing at this crawl keeps working.
    ///
    /// A refresh bills the pages it re-scrapes, exactly like the original
    /// crawl. What unchanged pages save is the embedding and the index write.
    ///
    /// No retry: replaying the request starts a second re-scrape of the whole
    /// site, and that is billable.
    pub async fn crawl_refresh_now(
        &self,
        uuid: &str,
    ) -> Result<CrawlerRefreshState, ScrapflyError> {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        let url = self.build_url(&format!("/crawl/{}/refresh", uuid), &[])?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let resp = self
            .send_once(Method::POST, url, Some(headers), None)
            .await?;
        let (status, _h, bytes) = read_response(resp).await?;
        if status != 200 && status != 202 {
            return Err(from_response(status, &bytes, 0, true));
        }
        Ok(CrawlerRefreshState::from_envelope(&bytes)?)
    }

    /// Change the refresh schedule of an existing crawl.
    ///
    /// `PATCH /crawl/{uuid}/refresh` — only the fields set on `settings` are
    /// changed, so turning a crawl off keeps its interval for when it is
    /// turned back on.
    ///
    /// Turning refresh on for a crawl started without it is allowed: the crawl
    /// already holds the URL index a refresh walks.
    pub async fn crawl_refresh_settings(
        &self,
        uuid: &str,
        settings: &CrawlRefreshSettings,
    ) -> Result<CrawlerRefreshState, ScrapflyError> {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        if settings.enabled.is_none() && settings.interval_seconds.is_none() {
            return Err(ScrapflyError::Config(
                "set at least one of enabled, interval_seconds".into(),
            ));
        }
        if let Some(interval) = settings.interval_seconds {
            if !(REFRESH_MIN_INTERVAL..=REFRESH_MAX_INTERVAL).contains(&interval) {
                return Err(ScrapflyError::Config(format!(
                    "interval_seconds must be between {} and {} seconds, got {}",
                    REFRESH_MIN_INTERVAL, REFRESH_MAX_INTERVAL, interval
                )));
            }
        }

        // Wire keys are the ones POST /crawl already takes, so a crawl body and
        // a later PATCH name the same things. The enabled/interval_seconds
        // spelling belongs to the state block this call answers with, not to
        // its request; the API decodes the body with unknown fields rejected.
        let mut body = serde_json::Map::new();
        if let Some(enabled) = settings.enabled {
            body.insert("refresh".into(), serde_json::Value::Bool(enabled));
        }
        if let Some(interval) = settings.interval_seconds {
            body.insert("refresh_interval".into(), serde_json::json!(interval));
        }

        let url = self.build_url(&format!("/crawl/{}/refresh", uuid), &[])?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let payload = serde_json::to_vec(&serde_json::Value::Object(body))?;
        let resp = self
            .send_with_retry(Method::PATCH, url, Some(headers), Some(payload))
            .await?;
        let (status, _h, bytes) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &bytes, 0, true));
        }
        Ok(CrawlerRefreshState::from_envelope(&bytes)?)
    }

    /// Read a crawl's refresh timeline, newest last.
    ///
    /// `GET /crawl/{uuid}/refresh/history` — the server keeps the 50 most
    /// recent runs; older rows are trimmed rather than paged, because the
    /// timeline exists to show recent activity. `limit` of `None` returns
    /// everything the server kept.
    pub async fn crawl_refresh_history(
        &self,
        uuid: &str,
        limit: Option<u32>,
    ) -> Result<Vec<CrawlerRefreshEntry>, ScrapflyError> {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        let mut pairs: Vec<(String, String)> = Vec::new();
        if let Some(limit) = limit {
            pairs.push(("limit".into(), limit.to_string()));
        }
        let url = self.build_url(&format!("/crawl/{}/refresh/history", uuid), &pairs)?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let resp = self
            .send_with_retry(Method::GET, url, Some(headers), None)
            .await?;
        let (status, _h, bytes) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &bytes, 0, true));
        }
        Ok(CrawlerRefreshState::from_envelope(&bytes)?.history)
    }

    /// Cancel a crawler job.
    pub async fn crawl_cancel(&self, uuid: &str) -> Result<(), ScrapflyError> {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        let url = self.build_url(&format!("/crawl/{}/cancel", uuid), &[])?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let resp = self
            .send_with_retry(Method::POST, url, Some(headers), None)
            .await?;
        let (status, _h, body) = read_response(resp).await?;
        if status != 200 && status != 202 {
            return Err(from_response(status, &body, 0, true));
        }
        Ok(())
    }

    /// Download a crawler artifact (WARC or HAR).
    pub async fn crawl_artifact(
        &self,
        uuid: &str,
        artifact_type: CrawlerArtifactType,
    ) -> Result<CrawlerArtifact, ScrapflyError> {
        if uuid.is_empty() {
            return Err(ScrapflyError::Config("uuid cannot be empty".into()));
        }
        let pairs: Vec<(String, String)> = vec![("type".into(), artifact_type.as_str().into())];
        let url = self.build_url(&format!("/crawl/{}/artifact", uuid), &pairs)?;
        let mut headers = HeaderMap::new();
        // HAR is plain JSON — asking for `application/gzip` makes the server
        // gzip-wrap it, and reqwest can't auto-decode it without a matching
        // `Content-Encoding` header. Match `sdk/go/crawler.go::CrawlArtifact`
        // which sends different Accept per artifact type.
        let accept = match artifact_type {
            CrawlerArtifactType::Har => "application/json, application/octet-stream",
            CrawlerArtifactType::Warc => {
                "application/gzip, application/octet-stream, application/json"
            }
        };
        headers.insert(ACCEPT, HeaderValue::from_static(accept));
        let resp = self
            .send_with_retry(Method::GET, url, Some(headers), None)
            .await?;
        let (status, _h, body) = read_response(resp).await?;
        if status != 200 {
            return Err(from_response(status, &body, 0, true));
        }
        Ok(CrawlerArtifact {
            artifact_type,
            data: body,
        })
    }

    // ==============================================================================
    // Cloud browser methods (implementations in cloud_browser.rs)
    // ==============================================================================

    /// Fire a request through the retry loop.
    pub(crate) async fn send_with_retry(
        &self,
        method: Method,
        url: Url,
        headers: Option<HeaderMap>,
        body: Option<Vec<u8>>,
    ) -> Result<Response, ScrapflyError> {
        let mut last_err: Option<ScrapflyError> = None;
        for attempt in 0..DEFAULT_RETRIES {
            let mut req = self.http.request(method.clone(), url.clone());
            let mut hmap = headers.clone().unwrap_or_default();
            if !hmap.contains_key(USER_AGENT) {
                hmap.insert(USER_AGENT, HeaderValue::from_static(SDK_USER_AGENT));
            }
            if let Some(cb) = &self.on_request {
                cb(&method, &url, &hmap);
            }
            req = req.headers(hmap);
            if let Some(b) = &body {
                req = req.body(b.clone());
            }
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if (500..600).contains(&status) && attempt + 1 < DEFAULT_RETRIES {
                        last_err = Some(ScrapflyError::ApiServer(crate::error::ApiError {
                            message: "server error".into(),
                            http_status: status,
                            ..Default::default()
                        }));
                        tokio::time::sleep(DEFAULT_RETRY_DELAY).await;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    last_err = Some(ScrapflyError::Transport(e));
                    if attempt + 1 < DEFAULT_RETRIES {
                        tokio::time::sleep(DEFAULT_RETRY_DELAY).await;
                        continue;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| ScrapflyError::Config("retry loop exhausted".into())))
    }

    /// Fire a request exactly once, no retry. For calls whose replay is not
    /// free: a refresh re-scrapes the whole site and is billable.
    pub(crate) async fn send_once(
        &self,
        method: Method,
        url: Url,
        headers: Option<HeaderMap>,
        body: Option<Vec<u8>>,
    ) -> Result<Response, ScrapflyError> {
        let mut hmap = headers.unwrap_or_default();
        if !hmap.contains_key(USER_AGENT) {
            hmap.insert(USER_AGENT, HeaderValue::from_static(SDK_USER_AGENT));
        }
        if let Some(cb) = &self.on_request {
            cb(&method, &url, &hmap);
        }
        let mut req = self.http.request(method, url).headers(hmap);
        if let Some(b) = body {
            req = req.body(b);
        }
        req.send().await.map_err(ScrapflyError::Transport)
    }

    /// Single-shot send, no retry (for `verify_api_key`/`account` style calls).
    /// Send the prompt request with a deadline that outlives the server's own
    /// budget. `reqwest`'s client timeout covers the response body, so the
    /// 150s default would cut a legitimate SSE stream mid-answer: the API
    /// allows retrieval plus up to 150s of generation under a 165s ceiling.
    /// The per-request override leaves every other call's deadline alone.
    async fn send_prompt_stream(
        &self,
        url: Url,
        headers: HeaderMap,
        body: Vec<u8>,
    ) -> Result<Response, ScrapflyError> {
        let mut hmap = headers;
        if !hmap.contains_key(USER_AGENT) {
            hmap.insert(USER_AGENT, HeaderValue::from_static(SDK_USER_AGENT));
        }
        if let Some(cb) = &self.on_request {
            cb(&Method::POST, &url, &hmap);
        }
        self.http
            .request(Method::POST, url)
            .headers(hmap)
            .body(body)
            .timeout(CRAWL_PROMPT_STREAM_TIMEOUT)
            .send()
            .await
            .map_err(ScrapflyError::Transport)
    }

    async fn send_simple(
        &self,
        method: Method,
        url: Url,
        headers: Option<HeaderMap>,
        body: Option<Vec<u8>>,
    ) -> Result<Response, ScrapflyError> {
        let mut req = self.http.request(method.clone(), url.clone());
        let mut hmap = headers.unwrap_or_default();
        if !hmap.contains_key(USER_AGENT) {
            hmap.insert(USER_AGENT, HeaderValue::from_static(SDK_USER_AGENT));
        }
        if let Some(cb) = &self.on_request {
            cb(&method, &url, &hmap);
        }
        req = req.headers(hmap);
        if let Some(b) = body {
            req = req.body(b);
        }
        req.send().await.map_err(ScrapflyError::Transport)
    }
}

/// Drain a response into (status, headers, body bytes) and propagate
/// `Retry-After` into the retry-ms field when present.
async fn read_response(resp: Response) -> Result<(u16, HeaderMap, bytes::Bytes), ScrapflyError> {
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
    let _ = parse_retry_after(headers.get("retry-after").and_then(|v| v.to_str().ok()));
    Ok((status, headers, body))
}

/// Minimal RFC 2387 multipart/related parser — ported from
/// `sdk/go/crawler.go::parseMultipartRelated`.
fn parse_multipart_related(
    body: &str,
    content_type: &str,
    formats: &[&str],
) -> Result<
    std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    ScrapflyError,
> {
    let mut boundary = String::new();
    for part in content_type.split(';') {
        let p = part.trim();
        if let Some(stripped) = p.strip_prefix("boundary=") {
            boundary = stripped.trim_matches('"').to_string();
            break;
        }
    }
    if boundary.is_empty() {
        return Err(ScrapflyError::UnexpectedResponseFormat(format!(
            "multipart response has no boundary in Content-Type: {}",
            content_type
        )));
    }
    let delimiter = format!("--{}", boundary);
    let mut result: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> =
        std::collections::BTreeMap::new();
    let segments: Vec<&str> = body.split(&delimiter as &str).collect();
    for segment in segments.iter().skip(1) {
        let mut seg = *segment;
        seg = seg.trim_start_matches("\r\n").trim_start_matches('\n');
        if seg.starts_with("--") {
            break;
        }
        seg = seg.trim_end_matches("\r\n").trim_end_matches('\n');
        let (headers_raw, part_body) = if let Some(idx) = seg.find("\r\n\r\n") {
            (&seg[..idx], &seg[idx + 4..])
        } else if let Some(idx) = seg.find("\n\n") {
            (&seg[..idx], &seg[idx + 2..])
        } else {
            continue;
        };
        let mut part_url = String::new();
        let mut part_format = String::new();
        for line in headers_raw.split('\n') {
            let line = line.trim_end_matches('\r');
            if let Some(colon) = line.find(':') {
                let name = line[..colon].trim().to_ascii_lowercase();
                let value = line[colon + 1..].trim().to_string();
                match name.as_str() {
                    "content-location" => part_url = value,
                    "content-type" => part_format = infer_format_from_content_type(&value),
                    _ => {}
                }
            }
        }
        if part_url.is_empty() {
            continue;
        }
        if part_format.is_empty() {
            part_format = formats.first().copied().unwrap_or("html").to_string();
        }
        result
            .entry(part_url)
            .or_default()
            .insert(part_format, part_body.to_string());
    }
    Ok(result)
}

/// Which retrieval legs a crawl search runs.
///
/// `Hybrid` runs both and merges them with reciprocal rank fusion; it is the
/// server default when the mode is left unset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrawlerSearchMode {
    /// Semantic (embedding) leg only.
    Vector,
    /// Keyword (full-text) leg only.
    Fts,
    /// Both legs, merged with reciprocal rank fusion.
    Hybrid,
}

impl CrawlerSearchMode {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Vector => "vector",
            Self::Fts => "fts",
            Self::Hybrid => "hybrid",
        }
    }
}

/// Fields to change on a crawl's refresh schedule, for
/// [`Client::crawl_refresh_settings`].
///
/// Every field is optional so that "leave alone" stays distinguishable from
/// "set to false" / "set to zero": turning a crawl off must keep its interval
/// for when it is turned back on.
#[derive(Debug, Clone, Default)]
pub struct CrawlRefreshSettings {
    /// Turn auto-refresh on or off.
    pub enabled: Option<bool>,
    /// Period between runs, in seconds.
    pub interval_seconds: Option<u32>,
}

/// Options for [`Client::crawls_search`]. The default sends nothing and lets
/// the server apply its own defaults for every field.
#[derive(Debug, Clone, Default)]
pub struct CrawlSearchOptions {
    /// Result cap, 1-50 (server cap). `None` = server default.
    pub limit: Option<u32>,
    /// Retrieval legs. `None` = server default (hybrid).
    pub mode: Option<CrawlerSearchMode>,
    /// Flat filter map: `url_prefix`, `host`, `source_format`,
    /// `content_type`, `http_status`, `crawler_uuid`. Filters are pushed down
    /// per crawl before its top-K, so they never cost recall. Unknown keys
    /// are rejected server-side rather than ignored.
    pub filters: Option<serde_json::Value>,
    /// Next-page token from a previous response. `None` starts page 1.
    pub cursor: Option<String>,
}

impl CrawlSearchOptions {
    fn apply(
        &self,
        body: &mut serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), ScrapflyError> {
        if let Some(limit) = self.limit {
            body.insert("limit".into(), serde_json::Value::from(limit));
        }
        if let Some(mode) = self.mode {
            body.insert(
                "mode".into(),
                serde_json::Value::String(mode.as_str().to_string()),
            );
        }
        if let Some(filters) = &self.filters {
            if !filters.is_object() {
                return Err(ScrapflyError::Config(
                    "search filters must be a JSON object".into(),
                ));
            }
            body.insert("filters".into(), filters.clone());
        }
        if let Some(cursor) = &self.cursor {
            body.insert("cursor".into(), serde_json::Value::String(cursor.clone()));
        }
        Ok(())
    }
}

/// Options for [`Client::crawls_prompt`].
#[derive(Debug, Clone, Default)]
pub struct CrawlPromptOptions {
    /// Retrieval overrides. `None` uses the server's defaults.
    pub search: Option<CrawlSearchOptions>,
    /// Generation model id. `None` uses the server default.
    pub model: Option<String>,
}

/// Enforce the shape both collection endpoints share: a non-empty list with
/// no duplicates. Duplicates are rejected by the API, and catching them here
/// saves a round trip that can only fail.
fn validate_crawl_ids(crawl_ids: &[String]) -> Result<(), ScrapflyError> {
    if crawl_ids.is_empty() {
        return Err(ScrapflyError::Config(
            "crawl_ids must contain at least one crawler UUID".into(),
        ));
    }
    let mut seen = std::collections::HashSet::with_capacity(crawl_ids.len());
    for id in crawl_ids {
        if id.is_empty() {
            return Err(ScrapflyError::Config(
                "crawl_ids contains an empty crawler UUID".into(),
            ));
        }
        if !seen.insert(id.as_str()) {
            return Err(ScrapflyError::Config(format!(
                "crawl_ids contains duplicate crawler UUID {id}"
            )));
        }
    }
    Ok(())
}

/// Buffered SSE decoder state for the `/crawl/prompt` body.
struct PromptStreamState {
    body: std::pin::Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    /// Undecoded tail: a UTF-8 character or an SSE line can straddle chunks.
    buffer: Vec<u8>,
    event: String,
    data: Vec<String>,
    pending: std::collections::VecDeque<Result<CrawlerPromptEvent, ScrapflyError>>,
    finished: bool,
}

/// Decode the `POST /crawl/prompt` SSE body into typed frames.
///
/// Only `event:` and `data:` lines matter; `:keepalive` comment frames exist
/// to keep intermediaries from closing an idle connection and carry nothing.
/// Token payloads are JSON strings; every other frame is a JSON object.
fn crawler_prompt_stream(
    resp: Response,
) -> impl Stream<Item = Result<CrawlerPromptEvent, ScrapflyError>> {
    let state = PromptStreamState {
        body: Box::pin(resp.bytes_stream()),
        buffer: Vec::new(),
        event: String::new(),
        data: Vec::new(),
        pending: std::collections::VecDeque::new(),
        finished: false,
    };

    futures_util::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(item) = st.pending.pop_front() {
                return Some((item, st));
            }
            if st.finished {
                return None;
            }
            match st.body.next().await {
                Some(Ok(chunk)) => {
                    st.buffer.extend_from_slice(&chunk);
                    drain_prompt_frames(&mut st, false);
                }
                Some(Err(e)) => {
                    st.finished = true;
                    st.pending.push_back(Err(ScrapflyError::Transport(e)));
                }
                None => {
                    st.finished = true;
                    drain_prompt_frames(&mut st, true);
                }
            }
        }
    })
}

/// Pull every complete line out of the buffer and push decoded frames onto
/// `pending`. With `eof` the trailing partial line is flushed too, which
/// covers a server that omits the final blank line.
fn drain_prompt_frames(st: &mut PromptStreamState, eof: bool) {
    while let Some(idx) = st.buffer.iter().position(|b| *b == b'\n') {
        let raw: Vec<u8> = st.buffer.drain(..=idx).collect();
        let line = String::from_utf8_lossy(&raw[..raw.len() - 1])
            .trim_end_matches('\r')
            .to_string();
        handle_prompt_line(st, &line);
    }
    if eof {
        if !st.buffer.is_empty() {
            let raw = std::mem::take(&mut st.buffer);
            let line = String::from_utf8_lossy(&raw)
                .trim_end_matches('\r')
                .to_string();
            handle_prompt_line(st, &line);
        }
        // A stream that ended without its terminating blank line still has a
        // complete frame buffered; emitting it beats losing the answer.
        flush_prompt_frame(st);
    }
}

fn handle_prompt_line(st: &mut PromptStreamState, line: &str) {
    if line.starts_with(':') {
        return;
    }
    // A blank line terminates a frame.
    if line.is_empty() {
        flush_prompt_frame(st);
        return;
    }
    if let Some(rest) = line.strip_prefix("event:") {
        st.event = rest.trim().to_string();
    } else if let Some(rest) = line.strip_prefix("data:") {
        st.data
            .push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
    }
}

fn flush_prompt_frame(st: &mut PromptStreamState) {
    if st.event.is_empty() || st.data.is_empty() {
        st.event.clear();
        st.data.clear();
        return;
    }
    let raw = st.data.join("\n");
    let event = std::mem::take(&mut st.event);
    st.data.clear();

    let decoded = match event.as_str() {
        "token" => {
            // A server sending bare text instead of a JSON string is still
            // sending a token; do not drop the answer over the quoting.
            let token = serde_json::from_str::<String>(&raw).unwrap_or_else(|_| raw.clone());
            Ok(CrawlerPromptEvent::Token(token))
        }
        "source" => serde_json::from_str::<CrawlerPromptSource>(&raw)
            .map(CrawlerPromptEvent::Source)
            .map_err(ScrapflyError::Json),
        "done" => serde_json::from_str::<CrawlerPromptDone>(&raw)
            .map(CrawlerPromptEvent::Done)
            .map_err(ScrapflyError::Json),
        "error" => {
            let payload: serde_json::Value =
                serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
            Err(ScrapflyError::Api(ApiError {
                code: payload
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ERR::CRAWLER::UNKNOWN")
                    .to_string(),
                message: payload
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or(raw.as_str())
                    .to_string(),
                http_status: 200,
                ..Default::default()
            }))
        }
        _ => return,
    };
    st.pending.push_back(decoded);
}

fn infer_format_from_content_type(ct: &str) -> String {
    let lc = ct
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match lc.as_str() {
        "text/html" => "html".into(),
        "text/markdown" => "markdown".into(),
        "text/plain" => "text".into(),
        "application/json" => "json".into(),
        _ => String::new(),
    }
}
