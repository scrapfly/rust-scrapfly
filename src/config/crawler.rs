//! Crawler endpoint configuration — ported from `sdk/go/config_crawler.go`.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::enums::{CrawlerContentFormat, CrawlerWebhookEvent};
use crate::error::ScrapflyError;

/// Configuration for a `POST /crawl` request.
///
/// Every field except `url` is optional; unset fields are NOT serialized so
/// the server applies its own documented defaults.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CrawlerConfig {
    /// Seed URL (required, must be HTTP/HTTPS).
    pub url: String,

    /// Max pages to crawl.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_limit: Option<u32>,
    /// Max link-follow depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
    /// Max duration (seconds, 15..=10800).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_duration: Option<u32>,
    /// Max API credit to spend (0 = no limit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_api_credit: Option<u32>,

    /// Exclude these URL paths (≤100 entries).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_paths: Vec<String>,
    /// Restrict crawl to these paths (≤100 entries).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub include_only_paths: Vec<String>,

    /// Ignore the seed URL's base-path restriction.
    #[serde(skip_serializing_if = "is_false")]
    pub ignore_base_path_restriction: bool,
    /// Follow links to external domains.
    #[serde(skip_serializing_if = "is_false")]
    pub follow_external_links: bool,
    /// Whitelist of external domains (≤250 entries).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_external_domains: Vec<String>,

    /// Tri-state: None = unset (server default true), Some(v) = explicit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_internal_subdomains: Option<bool>,
    /// Whitelist of internal subdomains (≤250 entries).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_internal_subdomains: Vec<String>,

    /// Request headers sent for every crawled page.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// Delay between requests (ms, 0..=15000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay: Option<u32>,
    /// Override User-Agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// Max concurrent workers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrency: Option<u32>,
    /// Rendering delay (ms, 0..=25000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendering_delay: Option<u32>,

    /// Honor sitemaps.
    #[serde(skip_serializing_if = "is_false")]
    pub use_sitemaps: bool,
    /// Follow `nofollow` links anyway.
    #[serde(skip_serializing_if = "is_false")]
    pub ignore_no_follow: bool,

    /// Tri-state: None = server default (true).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub respect_robots_txt: Option<bool>,

    /// Enable cache.
    #[serde(skip_serializing_if = "is_false")]
    pub cache: bool,
    /// Cache TTL seconds (0..=604800).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_ttl: Option<u32>,
    /// Force cache refresh.
    #[serde(skip_serializing_if = "is_false")]
    pub cache_clear: bool,

    /// Desired content formats.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub content_formats: Vec<CrawlerContentFormat>,
    /// Inline extraction rules.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_rules: Option<serde_json::Value>,

    /// Enable ASP bypass.
    #[serde(skip_serializing_if = "is_false")]
    pub asp: bool,
    /// Proxy pool name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_pool: Option<String>,
    /// Proxy country.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,

    /// Webhook name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_name: Option<String>,
    /// Webhook events.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub webhook_events: Vec<CrawlerWebhookEvent>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

impl CrawlerConfig {
    /// Start a builder for `url`.
    pub fn builder(url: impl Into<String>) -> CrawlerConfigBuilder {
        CrawlerConfigBuilder {
            cfg: CrawlerConfig {
                url: url.into(),
                ..Default::default()
            },
        }
    }

    /// Validate numeric bounds + list sizes. Ported from
    /// `sdk/go/config_crawler.go::validateBounds`.
    pub fn validate(&self) -> Result<(), ScrapflyError> {
        if self.url.is_empty() {
            return Err(ScrapflyError::Config("url is required".into()));
        }
        if let Some(d) = self.max_duration {
            if !(15..=10800).contains(&d) {
                return Err(ScrapflyError::Config(format!(
                    "max_duration must be between 15 and 10800 seconds, got {}",
                    d
                )));
            }
        }
        if let Some(rd) = self.rendering_delay {
            if rd > 25000 {
                return Err(ScrapflyError::Config(format!(
                    "rendering_delay must be between 0 and 25000 ms, got {}",
                    rd
                )));
            }
        }
        if let Some(delay) = self.delay {
            if delay > 15000 {
                return Err(ScrapflyError::Config(format!(
                    "delay must be between 0 and 15000 ms, got {}",
                    delay
                )));
            }
        }
        if let Some(ttl) = self.cache_ttl {
            if ttl > 604800 {
                return Err(ScrapflyError::Config(format!(
                    "cache_ttl must be between 0 and 604800 seconds, got {}",
                    ttl
                )));
            }
        }
        if self.exclude_paths.len() > 100 {
            return Err(ScrapflyError::Config(format!(
                "exclude_paths is limited to 100 entries, got {}",
                self.exclude_paths.len()
            )));
        }
        if self.include_only_paths.len() > 100 {
            return Err(ScrapflyError::Config(format!(
                "include_only_paths is limited to 100 entries, got {}",
                self.include_only_paths.len()
            )));
        }
        if !self.exclude_paths.is_empty() && !self.include_only_paths.is_empty() {
            return Err(ScrapflyError::Config(
                "exclude_paths and include_only_paths are mutually exclusive".into(),
            ));
        }
        if self.allowed_external_domains.len() > 250 {
            return Err(ScrapflyError::Config(format!(
                "allowed_external_domains is limited to 250 entries, got {}",
                self.allowed_external_domains.len()
            )));
        }
        if self.allowed_internal_subdomains.len() > 250 {
            return Err(ScrapflyError::Config(format!(
                "allowed_internal_subdomains is limited to 250 entries, got {}",
                self.allowed_internal_subdomains.len()
            )));
        }
        Ok(())
    }

    /// Serialize into the JSON body the crawler endpoint expects.
    pub fn to_json_body(&self) -> Result<Vec<u8>, ScrapflyError> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }
}

/// Builder for [`CrawlerConfig`].
#[derive(Debug, Clone)]
pub struct CrawlerConfigBuilder {
    cfg: CrawlerConfig,
}

impl CrawlerConfigBuilder {
    /// Set page limit.
    pub fn page_limit(mut self, v: u32) -> Self {
        self.cfg.page_limit = Some(v);
        self
    }
    /// Set max depth.
    pub fn max_depth(mut self, v: u32) -> Self {
        self.cfg.max_depth = Some(v);
        self
    }
    /// Set max duration (seconds).
    pub fn max_duration(mut self, v: u32) -> Self {
        self.cfg.max_duration = Some(v);
        self
    }
    /// Set max API credit.
    pub fn max_api_credit(mut self, v: u32) -> Self {
        self.cfg.max_api_credit = Some(v);
        self
    }
    /// Set exclude paths.
    pub fn exclude_paths(mut self, v: Vec<String>) -> Self {
        self.cfg.exclude_paths = v;
        self
    }
    /// Set include-only paths.
    pub fn include_only_paths(mut self, v: Vec<String>) -> Self {
        self.cfg.include_only_paths = v;
        self
    }
    /// Ignore base-path restriction.
    pub fn ignore_base_path_restriction(mut self, v: bool) -> Self {
        self.cfg.ignore_base_path_restriction = v;
        self
    }
    /// Follow external links.
    pub fn follow_external_links(mut self, v: bool) -> Self {
        self.cfg.follow_external_links = v;
        self
    }
    /// Set allowed external domains.
    pub fn allowed_external_domains(mut self, v: Vec<String>) -> Self {
        self.cfg.allowed_external_domains = v;
        self
    }
    /// Tri-state follow-internal-subdomains.
    pub fn follow_internal_subdomains(mut self, v: bool) -> Self {
        self.cfg.follow_internal_subdomains = Some(v);
        self
    }
    /// Set allowed internal subdomains.
    pub fn allowed_internal_subdomains(mut self, v: Vec<String>) -> Self {
        self.cfg.allowed_internal_subdomains = v;
        self
    }
    /// Add header.
    pub fn header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.cfg.headers.insert(k.into(), v.into());
        self
    }
    /// Set delay (ms).
    pub fn delay(mut self, v: u32) -> Self {
        self.cfg.delay = Some(v);
        self
    }
    /// Set User-Agent.
    pub fn user_agent(mut self, v: impl Into<String>) -> Self {
        self.cfg.user_agent = Some(v.into());
        self
    }
    /// Set max concurrency.
    pub fn max_concurrency(mut self, v: u32) -> Self {
        self.cfg.max_concurrency = Some(v);
        self
    }
    /// Set rendering delay (ms).
    pub fn rendering_delay(mut self, v: u32) -> Self {
        self.cfg.rendering_delay = Some(v);
        self
    }
    /// Honor sitemaps.
    pub fn use_sitemaps(mut self, v: bool) -> Self {
        self.cfg.use_sitemaps = v;
        self
    }
    /// Ignore nofollow.
    pub fn ignore_no_follow(mut self, v: bool) -> Self {
        self.cfg.ignore_no_follow = v;
        self
    }
    /// Tri-state respect-robots-txt.
    pub fn respect_robots_txt(mut self, v: bool) -> Self {
        self.cfg.respect_robots_txt = Some(v);
        self
    }
    /// Enable cache.
    pub fn cache(mut self, v: bool) -> Self {
        self.cfg.cache = v;
        self
    }
    /// Cache TTL.
    pub fn cache_ttl(mut self, v: u32) -> Self {
        self.cfg.cache_ttl = Some(v);
        self
    }
    /// Force cache refresh.
    pub fn cache_clear(mut self, v: bool) -> Self {
        self.cfg.cache_clear = v;
        self
    }
    /// Add content format.
    pub fn content_format(mut self, v: CrawlerContentFormat) -> Self {
        self.cfg.content_formats.push(v);
        self
    }
    /// Set extraction rules.
    pub fn extraction_rules(mut self, v: serde_json::Value) -> Self {
        self.cfg.extraction_rules = Some(v);
        self
    }
    /// Enable ASP.
    pub fn asp(mut self, v: bool) -> Self {
        self.cfg.asp = v;
        self
    }
    /// Set proxy pool name.
    pub fn proxy_pool(mut self, v: impl Into<String>) -> Self {
        self.cfg.proxy_pool = Some(v.into());
        self
    }
    /// Set country.
    pub fn country(mut self, v: impl Into<String>) -> Self {
        self.cfg.country = Some(v.into());
        self
    }
    /// Set webhook name.
    pub fn webhook_name(mut self, v: impl Into<String>) -> Self {
        self.cfg.webhook_name = Some(v.into());
        self
    }
    /// Add webhook event.
    pub fn webhook_event(mut self, v: CrawlerWebhookEvent) -> Self {
        self.cfg.webhook_events.push(v);
        self
    }
    /// Finalize the builder.
    pub fn build(self) -> Result<CrawlerConfig, ScrapflyError> {
        self.cfg.validate()?;
        Ok(self.cfg)
    }
}
