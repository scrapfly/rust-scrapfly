//! # scrapfly-sdk
//!
//! Async Rust client for the Scrapfly API. See the crate-level
//! [`Client`] and the `examples/` directory for usage.
//!
//! ```no_run
//! use scrapfly_sdk::{Client, ScrapeConfig};
//!
//! # async fn run() -> Result<(), scrapfly_sdk::ScrapflyError> {
//! let client = Client::builder().api_key("scp-...").build()?;
//! let result = client
//!     .scrape(&ScrapeConfig::builder("https://httpbin.dev/html").build()?)
//!     .await?;
//! println!("{}", result.result.status_code);
//! # Ok(()) }
//! ```
//!
//! ## Unblocker
//!
//! The anti-bot bypass is turned on with `unblocker`, on both
//! [`ScrapeConfig`] and [`CrawlerConfig`]:
//!
//! ```no_run
//! # use scrapfly_sdk::ScrapeConfig;
//! # fn run() -> Result<(), scrapfly_sdk::ScrapflyError> {
//! let cfg = ScrapeConfig::builder("https://example.com")
//!     .unblocker(true)
//!     .build()?;
//! # Ok(()) }
//! ```
//!
//! `asp` is the previous name. Its builder methods are deprecated aliases
//! that keep working; when both names are supplied `asp` wins, in either call
//! order, and the two are never OR-ed. On the built structs the value lives in
//! a single field still called `asp` (renaming a public field would break
//! existing callers); `unblocker_enabled()` reads it and `set_unblocker()`
//! writes it. The request still carries the parameter as `asp` on the wire —
//! a server-compatibility detail of this release.
//!
//! The error variant [`ScrapflyError::AspBypassFailed`] keeps its name: it is
//! dispatched from the literal `ERR::ASP::*` codes the API returns, and
//! customer `match` arms name it. Rust cannot alias an enum variant, so the
//! current name reaches the error surface as
//! [`ScrapflyError::is_unblocker_failure`] — the counterpart of Go's
//! `ErrUnblockerBypassFailed` and the TypeScript / Python SDKs'
//! `ScrapflyUnblockerError`.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod batch;
pub mod client;
pub mod cloud_browser;
pub mod config;
pub mod crawler;
pub mod enums;
pub mod error;
pub mod monitoring;
pub mod result;
pub mod schedule;

pub use client::{
    Client, ClientBuilder, CrawlPromptOptions, CrawlRefreshSettings, CrawlSearchOptions,
    CrawlerSearchMode, OnRequest,
};
pub use cloud_browser::{project_salt, BrowserConfig, UnblockConfig, UnblockResult};
pub use config::crawler::CrawlerConfig;
pub use config::extraction::ExtractionConfig;
pub use config::scrape::ScrapeConfig;
pub use config::screenshot::ScreenshotConfig;
pub use crawler::{Crawl, WaitOptions};
pub use enums::*;
pub use error::{ApiError, ScrapflyError};
pub use monitoring::{
    CloudBrowserMonitoringOptions, MonitoringAggregation, MonitoringDataFormat,
    MonitoringMetricsOptions, MonitoringPeriod, MonitoringTargetMetricsOptions,
};
pub use result::account::{AccountData, VerifyApiKeyResult};
pub use result::crawler::{
    CrawlContent, CrawlerArtifact, CrawlerArtifactType, CrawlerContents, CrawlerLifecyclePayload,
    CrawlerPromptDone, CrawlerPromptEvent, CrawlerPromptSource, CrawlerRefreshEntry,
    CrawlerRefreshState, CrawlerSearchCrawl, CrawlerSearchPayload, CrawlerSearchResponse,
    CrawlerSearchResult, CrawlerSearchScores, CrawlerSearchSkipped, CrawlerSearchState,
    CrawlerSearchStats, CrawlerStartResponse, CrawlerStatus, CrawlerUpdatedDocuments,
    CrawlerUpdatedPayload, CrawlerUrlDiscoveredPayload, CrawlerUrlEntry, CrawlerUrlFailedPayload,
    CrawlerUrlSkippedPayload, CrawlerUrlVisitedPayload, CrawlerUrls, CrawlerWebhook,
    CrawlerWebhookCommon, CrawlerWebhookLogLink, CrawlerWebhookScrape, CrawlerWebhookStatusLink,
};
pub use result::extraction::ExtractionResult;
pub use result::scrape::ScrapeResult;
pub use result::screenshot::{ScreenshotMetadata, ScreenshotResult};
pub use schedule::{
    CreateScheduleRequest, ListSchedulesOptions, Schedule, ScheduleEnd, ScheduleRecurrence,
    UpdateScheduleRequest,
};
