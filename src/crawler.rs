//! High-level [`Crawl`] wrapper — port of `sdk/go/crawl.go`.

use std::time::{Duration, Instant};

use crate::client::Client;
use crate::config::crawler::CrawlerConfig;
use crate::enums::CrawlerContentFormat;
use crate::error::ScrapflyError;
use crate::result::crawler::{
    CrawlContent, CrawlerArtifact, CrawlerArtifactType, CrawlerContents, CrawlerStatus, CrawlerUrls,
};

/// Polling options for [`Crawl::wait`].
#[derive(Debug, Clone)]
pub struct WaitOptions {
    /// How often to poll (default 5s).
    pub poll_interval: Duration,
    /// Optional deadline; `None` means wait forever.
    pub max_wait: Option<Duration>,
    /// Verbose logging (currently a no-op — reserved for future use).
    pub verbose: bool,
    /// Return `Ok(())` instead of `CrawlerCancelled` when the job
    /// terminates in the CANCELLED state. Useful for the
    /// cancel-then-wait pattern.
    pub allow_cancelled: bool,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            max_wait: None,
            verbose: false,
            allow_cancelled: false,
        }
    }
}

/// High-level crawler lifecycle wrapper. Holds a borrow of the [`Client`]
/// and caches the last status + downloaded artifacts.
pub struct Crawl<'a> {
    client: &'a Client,
    config: CrawlerConfig,
    uuid: Option<String>,
    cached_status: Option<CrawlerStatus>,
    cached_warc: Option<CrawlerArtifact>,
    cached_har: Option<CrawlerArtifact>,
}

impl<'a> Crawl<'a> {
    /// Wrap a [`CrawlerConfig`] without starting the job.
    pub fn new(client: &'a Client, config: CrawlerConfig) -> Self {
        Self {
            client,
            config,
            uuid: None,
            cached_status: None,
            cached_warc: None,
            cached_har: None,
        }
    }

    /// Job UUID (empty string before [`Crawl::start`]).
    pub fn uuid(&self) -> &str {
        self.uuid.as_deref().unwrap_or("")
    }

    /// Whether `start()` has been called successfully.
    pub fn started(&self) -> bool {
        self.uuid.is_some()
    }

    /// Schedule the crawler job. Returns `CrawlerAlreadyStarted` on re-entry.
    pub async fn start(&mut self) -> Result<(), ScrapflyError> {
        if self.uuid.is_some() {
            return Err(ScrapflyError::CrawlerAlreadyStarted);
        }
        let resp = self.client.start_crawl(&self.config).await?;
        self.uuid = Some(resp.crawler_uuid);
        Ok(())
    }

    fn uuid_required(&self) -> Result<&str, ScrapflyError> {
        match &self.uuid {
            Some(u) => Ok(u.as_str()),
            None => Err(ScrapflyError::CrawlerNotStarted),
        }
    }

    /// Fetch the status, optionally using the cached copy.
    pub async fn status(&mut self, refresh: bool) -> Result<&CrawlerStatus, ScrapflyError> {
        let uuid = self.uuid_required()?.to_string();
        if refresh || self.cached_status.is_none() {
            let s = self.client.crawl_status(&uuid).await?;
            self.cached_status = Some(s);
        }
        match &self.cached_status {
            Some(s) => Ok(s),
            None => Err(ScrapflyError::CrawlerNotStarted),
        }
    }

    /// Poll status until the job reaches a terminal state.
    pub async fn wait(&mut self, opts: WaitOptions) -> Result<(), ScrapflyError> {
        self.uuid_required()?;
        let deadline = opts.max_wait.map(|d| Instant::now() + d);
        loop {
            let status = self.status(true).await?.clone();
            if status.is_finished || status.is_cancelled() {
                if status.is_failed() {
                    let reason = status.state.stop_reason.clone().unwrap_or_default();
                    return Err(ScrapflyError::CrawlerFailed(crate::error::ApiError {
                        message: format!("crawl failed (stop_reason={})", reason),
                        ..Default::default()
                    }));
                }
                if status.is_cancelled() {
                    if opts.allow_cancelled {
                        return Ok(());
                    }
                    return Err(ScrapflyError::CrawlerCancelled);
                }
                return Ok(());
            }
            if let Some(d) = deadline {
                if Instant::now() + opts.poll_interval > d {
                    return Err(ScrapflyError::CrawlerTimeout);
                }
            }
            tokio::time::sleep(opts.poll_interval).await;
        }
    }

    /// Cancel the running crawl. No-op if already finished server-side.
    pub async fn cancel(&self) -> Result<(), ScrapflyError> {
        let uuid = self.uuid_required()?;
        self.client.crawl_cancel(uuid).await
    }

    /// Paginated URL listing.
    pub async fn urls(
        &self,
        status_filter: Option<&str>,
        page: u32,
        per_page: u32,
    ) -> Result<CrawlerUrls, ScrapflyError> {
        let uuid = self.uuid_required()?;
        self.client
            .crawl_urls(uuid, status_filter, page, per_page)
            .await
    }

    /// Read a single URL's content and wrap it in a [`CrawlContent`]. Returns
    /// `Ok(None)` when the URL isn't part of the crawl.
    pub async fn read(
        &self,
        target_url: &str,
        format: CrawlerContentFormat,
    ) -> Result<Option<CrawlContent>, ScrapflyError> {
        let uuid = self.uuid_required()?.to_string();
        match self
            .client
            .crawl_contents_plain(&uuid, target_url, format)
            .await
        {
            Ok(content) => Ok(Some(CrawlContent {
                url: target_url.to_string(),
                content,
                crawl_uuid: uuid,
            })),
            Err(ScrapflyError::ApiClient(e)) if e.http_status == 404 => Ok(None),
            Err(ScrapflyError::CrawlerFailed(e)) if e.http_status == 404 => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Read the raw content string (empty string when URL not in crawl).
    pub async fn read_string(
        &self,
        target_url: &str,
        format: CrawlerContentFormat,
    ) -> Result<String, ScrapflyError> {
        Ok(self
            .read(target_url, format)
            .await?
            .map(|c| c.content)
            .unwrap_or_default())
    }

    /// Batch read up to 100 URLs.
    pub async fn read_batch(
        &self,
        urls: &[String],
        formats: &[CrawlerContentFormat],
    ) -> Result<
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
        ScrapflyError,
    > {
        let uuid = self.uuid_required()?;
        self.client.crawl_contents_batch(uuid, urls, formats).await
    }

    /// Bulk JSON contents.
    pub async fn contents(
        &self,
        format: CrawlerContentFormat,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<CrawlerContents, ScrapflyError> {
        let uuid = self.uuid_required()?;
        self.client
            .crawl_contents_json(uuid, format, limit, offset)
            .await
    }

    /// Download + cache the WARC artifact.
    pub async fn warc(&mut self) -> Result<&CrawlerArtifact, ScrapflyError> {
        let uuid = self.uuid_required()?.to_string();
        if self.cached_warc.is_none() {
            let a = self
                .client
                .crawl_artifact(&uuid, CrawlerArtifactType::Warc)
                .await?;
            self.cached_warc = Some(a);
        }
        match &self.cached_warc {
            Some(a) => Ok(a),
            None => Err(ScrapflyError::Config(
                "warc cache unexpectedly empty".into(),
            )),
        }
    }

    /// Download + cache the HAR artifact.
    pub async fn har(&mut self) -> Result<&CrawlerArtifact, ScrapflyError> {
        let uuid = self.uuid_required()?.to_string();
        if self.cached_har.is_none() {
            let a = self
                .client
                .crawl_artifact(&uuid, CrawlerArtifactType::Har)
                .await?;
            self.cached_har = Some(a);
        }
        match &self.cached_har {
            Some(a) => Ok(a),
            None => Err(ScrapflyError::Config("har cache unexpectedly empty".into())),
        }
    }
}
