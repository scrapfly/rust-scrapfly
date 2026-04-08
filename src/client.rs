//! HTTP client for the Scrapfly API.
//!
//! Built on `reqwest` with `rustls`. Single shared [`reqwest::Client`]
//! re-used across every call.

use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{Stream, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, USER_AGENT};
use reqwest::{Method, Response, Url};

use crate::config::crawler::CrawlerConfig;
use crate::config::extraction::ExtractionConfig;
use crate::config::scrape::ScrapeConfig;
use crate::config::screenshot::ScreenshotConfig;
use crate::error::{from_response, parse_retry_after, ApiError, ScrapflyError};
use crate::result::account::{AccountData, VerifyApiKeyResult};
use crate::result::crawler::{
    CrawlerArtifact, CrawlerArtifactType, CrawlerContents, CrawlerStartResponse, CrawlerStatus,
    CrawlerUrls,
};
use crate::result::extraction::ExtractionResult;
use crate::enums::HttpMethod;
use crate::result::scrape::{ResultData, ScrapeResult};
use crate::result::screenshot::{ScreenshotMetadata, ScreenshotResult};

const DEFAULT_HOST: &str = "https://api.scrapfly.io";
const DEFAULT_CLOUD_BROWSER_HOST: &str = "https://browser.scrapfly.io";
const SDK_USER_AGENT: &str = "Scrapfly-Rust-SDK";
const DEFAULT_RETRIES: usize = 3;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(150);

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
            let (err_code, err_message, err_doc) = match &result.result.error {
                Some(e) => (e.code.clone(), e.message.clone(), e.doc_url.clone()),
                None => (
                    result.result.status.clone(),
                    format!(
                        "scrape failed with status_code={}",
                        result.result.status_code
                    ),
                    String::new(),
                ),
            };
            let api_err = ApiError {
                code: err_code,
                message: err_message,
                http_status: result.result.status_code,
                documentation_url: err_doc,
                hint: String::new(),
                retry_after_ms: 0,
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
    pub async fn start_crawl(
        &self,
        config: &CrawlerConfig,
    ) -> Result<CrawlerStartResponse, ScrapflyError> {
        let body = config.to_json_body()?;
        let url = self.build_url("/crawl", &[])?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
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

    /// Single-shot send, no retry (for `verify_api_key`/`account` style calls).
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
