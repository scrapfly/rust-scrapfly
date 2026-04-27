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

pub use client::{Client, ClientBuilder, OnRequest};
pub use cloud_browser::{BrowserConfig, UnblockConfig, UnblockResult};
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
    CrawlContent, CrawlerArtifact, CrawlerArtifactType, CrawlerContents, CrawlerStartResponse,
    CrawlerStatus, CrawlerUrlEntry, CrawlerUrls,
};
pub use result::extraction::ExtractionResult;
pub use result::scrape::ScrapeResult;
pub use result::screenshot::{ScreenshotMetadata, ScreenshotResult};
pub use schedule::{
    CreateScheduleRequest, ListSchedulesOptions, Schedule, ScheduleEnd, ScheduleRecurrence,
    UpdateScheduleRequest,
};
