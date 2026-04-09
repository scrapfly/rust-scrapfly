//! Scrape endpoint configuration — ported from `sdk/go/config_scrape.go`.

use std::collections::BTreeMap;

use crate::enums::{ExtractionModel, Format, FormatOption, HttpMethod, ProxyPool, ScreenshotFlag};
use crate::error::ScrapflyError;

use super::url_safe_b64_encode;

/// Configuration for a single `POST /scrape` request.
///
/// Construct via [`ScrapeConfig::builder`].
#[derive(Debug, Clone, Default)]
pub struct ScrapeConfig {
    /// Target URL (required).
    pub url: String,
    /// HTTP method; defaults to `GET`.
    pub method: Option<HttpMethod>,
    /// Raw request body.
    pub body: Option<String>,
    /// Request headers (sent as `headers[key]=value`).
    pub headers: BTreeMap<String, String>,
    /// Cookies (merged into `headers[cookie]`).
    pub cookies: BTreeMap<String, String>,
    /// Proxy country.
    pub country: Option<String>,
    /// Proxy pool.
    pub proxy_pool: Option<ProxyPool>,
    /// Enable JavaScript rendering.
    pub render_js: bool,
    /// Enable Anti-Scraping Protection bypass.
    pub asp: bool,
    /// Enable cache.
    pub cache: bool,
    /// Cache TTL (seconds).
    pub cache_ttl: Option<u32>,
    /// Force cache refresh.
    pub cache_clear: bool,
    /// Timeout in milliseconds.
    pub timeout: Option<u32>,
    /// Maximum API credit cost the caller is willing to spend on this
    /// request. If the server's pre-flight estimate exceeds this budget the
    /// request is rejected before execution.
    pub cost_budget: Option<u32>,
    /// Enable automatic retries.
    pub retry: Option<bool>,
    /// Session name.
    pub session: Option<String>,
    /// Sticky-proxy inside the session.
    pub session_sticky_proxy: bool,
    /// Custom tags.
    pub tags: Vec<String>,
    /// Webhook name.
    pub webhook: Option<String>,
    /// Debug mode.
    pub debug: bool,
    /// Capture SSL details.
    pub ssl: bool,
    /// Capture DNS details.
    pub dns: bool,
    /// Correlation ID.
    pub correlation_id: Option<String>,
    /// Output format.
    pub format: Option<Format>,
    /// Format options.
    pub format_options: Vec<FormatOption>,
    /// Saved extraction template name.
    pub extraction_template: Option<String>,
    /// Inline (ephemeral) extraction template as JSON value.
    pub extraction_ephemeral_template: Option<serde_json::Value>,
    /// AI extraction prompt.
    pub extraction_prompt: Option<String>,
    /// Extraction model.
    pub extraction_model: Option<ExtractionModel>,
    /// Wait for CSS selector (requires `render_js`).
    pub wait_for_selector: Option<String>,
    /// Extra wait after page load, milliseconds.
    pub rendering_wait: Option<u32>,
    /// Auto-scroll to load lazy content.
    pub auto_scroll: bool,
    /// Named screenshots (name → selector, or "fullpage").
    pub screenshots: BTreeMap<String, String>,
    /// Screenshot flags.
    pub screenshot_flags: Vec<ScreenshotFlag>,
    /// Inline JavaScript code (base64url-encoded on the wire).
    pub js: Option<String>,
    /// JS scenario (serialized as JSON then base64url-encoded).
    pub js_scenario: Option<serde_json::Value>,
    /// OS fingerprint hint.
    pub os: Option<String>,
    /// Accept-Language values.
    pub lang: Vec<String>,
    /// Browser brand (`chrome` | `edge` | `brave` | `opera`).
    pub browser_brand: Option<String>,
    /// Spoof browser geolocation. Format: `"latitude,longitude"`.
    pub geolocation: Option<String>,
    /// Page load stage to wait for. `complete` (default) or `domcontentloaded`.
    pub rendering_stage: Option<String>,
    /// Return the raw upstream response instead of the JSON envelope.
    /// When true, callers must use `Client::scrape_proxified()` which
    /// returns `reqwest::Response` directly.
    pub proxified_response: bool,
}

impl ScrapeConfig {
    /// Start a builder for `url`.
    pub fn builder(url: impl Into<String>) -> ScrapeConfigBuilder {
        ScrapeConfigBuilder {
            cfg: ScrapeConfig {
                url: url.into(),
                ..Default::default()
            },
        }
    }

    /// Serialize the config into the query-parameter pairs that the
    /// `/scrape` endpoint expects. Mirrors
    /// `sdk/go/config_scrape.go::toAPIParamsWithValidation`.
    pub fn to_query_pairs(&self) -> Result<Vec<(String, String)>, ScrapflyError> {
        if self.url.is_empty() {
            return Err(ScrapflyError::Config("url is required".into()));
        }

        let mut out: Vec<(String, String)> = Vec::new();
        out.push(("url".into(), self.url.clone()));

        if let Some(country) = &self.country {
            out.push(("country".into(), country.to_lowercase()));
        }
        if let Some(pool) = &self.proxy_pool {
            out.push(("proxy_pool".into(), pool.as_str().into()));
        }

        if self.render_js {
            out.push(("render_js".into(), "true".into()));
            if let Some(sel) = &self.wait_for_selector {
                out.push(("wait_for_selector".into(), sel.clone()));
            }
            if let Some(wait) = self.rendering_wait {
                out.push(("rendering_wait".into(), wait.to_string()));
            }
            if let Some(ref geo) = self.geolocation {
                out.push(("geolocation".into(), geo.clone()));
            }
            if let Some(ref stage) = self.rendering_stage {
                if stage != "complete" {
                    out.push(("rendering_stage".into(), stage.clone()));
                }
            }
            if self.auto_scroll {
                out.push(("auto_scroll".into(), "true".into()));
            }
            if let Some(js) = &self.js {
                out.push(("js".into(), url_safe_b64_encode(js)));
            }
            if let Some(sc) = &self.js_scenario {
                let as_str = serde_json::to_string(sc)?;
                out.push(("js_scenario".into(), url_safe_b64_encode(&as_str)));
            }
            for (name, value) in &self.screenshots {
                if value.is_empty() {
                    return Err(ScrapflyError::Config(format!(
                        "screenshots[{}] requires either a selector or 'fullpage'",
                        name
                    )));
                }
                out.push((format!("screenshots[{}]", name), value.clone()));
            }
            if !self.screenshot_flags.is_empty() {
                let joined = self
                    .screenshot_flags
                    .iter()
                    .map(|f| f.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                out.push(("screenshot_flags".into(), joined));
            }
        }

        if self.asp {
            out.push(("asp".into(), "true".into()));
        }
        if self.retry == Some(false) {
            out.push(("retry".into(), "false".into()));
        }
        if self.cache {
            out.push(("cache".into(), "true".into()));
            if let Some(ttl) = self.cache_ttl {
                out.push(("cache_ttl".into(), ttl.to_string()));
            }
            if self.cache_clear {
                out.push(("cache_clear".into(), "true".into()));
            }
        }
        if let Some(timeout) = self.timeout {
            out.push(("timeout".into(), timeout.to_string()));
        }
        if let Some(budget) = self.cost_budget {
            out.push(("cost_budget".into(), budget.to_string()));
        }
        if self.debug {
            out.push(("debug".into(), "true".into()));
        }
        if self.ssl {
            out.push(("ssl".into(), "true".into()));
        }
        if self.dns {
            out.push(("dns".into(), "true".into()));
        }
        if let Some(cid) = &self.correlation_id {
            out.push(("correlation_id".into(), cid.clone()));
        }
        if !self.tags.is_empty() {
            out.push(("tags".into(), self.tags.join(",")));
        }
        if let Some(wh) = &self.webhook {
            out.push(("webhook_name".into(), wh.clone()));
        }
        if let Some(session) = &self.session {
            out.push(("session".into(), session.clone()));
            if self.session_sticky_proxy {
                out.push(("session_sticky_proxy".into(), "true".into()));
            }
        }
        if let Some(os) = &self.os {
            out.push(("os".into(), os.clone()));
        }
        if !self.lang.is_empty() {
            out.push(("lang".into(), self.lang.join(",")));
        }
        if let Some(bb) = &self.browser_brand {
            out.push(("browser_brand".into(), bb.clone()));
        }
        if self.proxified_response {
            out.push(("proxified_response".into(), "true".into()));
        }

        if let Some(format) = self.format {
            let mut val = format.as_str().to_string();
            if !self.format_options.is_empty() {
                val.push(':');
                val.push_str(
                    &self
                        .format_options
                        .iter()
                        .map(|f| f.as_str())
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            out.push(("format".into(), val));
        }

        // Extraction — exclusivity enforced by builder.
        if let Some(tpl) = &self.extraction_template {
            out.push(("extraction_template".into(), tpl.clone()));
        } else if let Some(tpl) = &self.extraction_ephemeral_template {
            let as_str = serde_json::to_string(tpl)?;
            out.push((
                "extraction_template".into(),
                format!("ephemeral:{}", url_safe_b64_encode(&as_str)),
            ));
        } else if let Some(prompt) = &self.extraction_prompt {
            out.push(("extraction_prompt".into(), prompt.clone()));
        } else if let Some(model) = self.extraction_model {
            out.push(("extraction_model".into(), model.as_str().into()));
        }

        // Headers → `headers[key]=value`.
        for (k, v) in &self.headers {
            if k.is_empty() || v.is_empty() {
                return Err(ScrapflyError::Config(
                    "headers key and value cannot be empty".into(),
                ));
            }
            out.push((format!("headers[{}]", k.to_lowercase()), v.clone()));
        }

        // Cookies merged into `headers[cookie]`.
        if !self.cookies.is_empty() {
            let cookie_str = self
                .cookies
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("; ");
            // Check for any existing `headers[cookie]` from above.
            let existing = out
                .iter()
                .position(|(k, _)| k.eq_ignore_ascii_case("headers[cookie]"));
            match existing {
                Some(idx) => {
                    let existing_val = out[idx].1.clone();
                    out[idx].1 = format!("{}; {}", existing_val, cookie_str);
                }
                None => out.push(("headers[cookie]".into(), cookie_str)),
            }
        }

        Ok(out)
    }
}

/// Builder for [`ScrapeConfig`].
#[derive(Debug, Clone)]
pub struct ScrapeConfigBuilder {
    cfg: ScrapeConfig,
}

impl ScrapeConfigBuilder {
    /// Set HTTP method.
    pub fn method(mut self, m: HttpMethod) -> Self {
        self.cfg.method = Some(m);
        self
    }
    /// Set raw request body.
    pub fn body(mut self, b: impl Into<String>) -> Self {
        self.cfg.body = Some(b.into());
        self
    }
    /// Set a custom header.
    pub fn header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.cfg.headers.insert(k.into(), v.into());
        self
    }
    /// Replace the full header map.
    pub fn headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.cfg.headers = headers;
        self
    }
    /// Set a cookie.
    pub fn cookie(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.cfg.cookies.insert(k.into(), v.into());
        self
    }
    /// Set proxy country.
    pub fn country(mut self, c: impl Into<String>) -> Self {
        self.cfg.country = Some(c.into());
        self
    }
    /// Set proxy pool.
    pub fn proxy_pool(mut self, p: ProxyPool) -> Self {
        self.cfg.proxy_pool = Some(p);
        self
    }
    /// Enable JS rendering.
    pub fn render_js(mut self, v: bool) -> Self {
        self.cfg.render_js = v;
        self
    }
    /// Enable ASP bypass.
    pub fn asp(mut self, v: bool) -> Self {
        self.cfg.asp = v;
        self
    }
    /// Enable cache.
    pub fn cache(mut self, v: bool) -> Self {
        self.cfg.cache = v;
        self
    }
    /// Set cache TTL.
    pub fn cache_ttl(mut self, v: u32) -> Self {
        self.cfg.cache_ttl = Some(v);
        self
    }
    /// Force cache refresh.
    pub fn cache_clear(mut self, v: bool) -> Self {
        self.cfg.cache_clear = v;
        self
    }
    /// Set the maximum API credit cost the caller will accept for this
    /// request. The server rejects the request pre-flight if its estimate
    /// exceeds the budget, so callers get a fast failure instead of a
    /// surprise bill.
    pub fn cost_budget(mut self, v: u32) -> Self {
        self.cfg.cost_budget = Some(v);
        self
    }
    /// Set request timeout (ms).
    pub fn timeout(mut self, v: u32) -> Self {
        self.cfg.timeout = Some(v);
        self
    }
    /// Set automatic retry flag.
    pub fn retry(mut self, v: bool) -> Self {
        self.cfg.retry = Some(v);
        self
    }
    /// Set session name.
    pub fn session(mut self, v: impl Into<String>) -> Self {
        self.cfg.session = Some(v.into());
        self
    }
    /// Sticky proxy in a session.
    pub fn session_sticky_proxy(mut self, v: bool) -> Self {
        self.cfg.session_sticky_proxy = v;
        self
    }
    /// Add a tag.
    pub fn tag(mut self, v: impl Into<String>) -> Self {
        self.cfg.tags.push(v.into());
        self
    }
    /// Set all tags.
    pub fn tags(mut self, v: Vec<String>) -> Self {
        self.cfg.tags = v;
        self
    }
    /// Set webhook name.
    pub fn webhook(mut self, v: impl Into<String>) -> Self {
        self.cfg.webhook = Some(v.into());
        self
    }
    /// Enable debug mode.
    pub fn debug(mut self, v: bool) -> Self {
        self.cfg.debug = v;
        self
    }
    /// Capture SSL details.
    pub fn ssl(mut self, v: bool) -> Self {
        self.cfg.ssl = v;
        self
    }
    /// Capture DNS details.
    pub fn dns(mut self, v: bool) -> Self {
        self.cfg.dns = v;
        self
    }
    /// Set correlation ID.
    pub fn correlation_id(mut self, v: impl Into<String>) -> Self {
        self.cfg.correlation_id = Some(v.into());
        self
    }
    /// Set output format.
    pub fn format(mut self, v: Format) -> Self {
        self.cfg.format = Some(v);
        self
    }
    /// Add a format option.
    pub fn format_option(mut self, v: FormatOption) -> Self {
        self.cfg.format_options.push(v);
        self
    }
    /// Set saved extraction template name.
    pub fn extraction_template(mut self, v: impl Into<String>) -> Self {
        self.cfg.extraction_template = Some(v.into());
        self
    }
    /// Set inline extraction template.
    pub fn extraction_ephemeral_template(mut self, v: serde_json::Value) -> Self {
        self.cfg.extraction_ephemeral_template = Some(v);
        self
    }
    /// Set AI extraction prompt.
    pub fn extraction_prompt(mut self, v: impl Into<String>) -> Self {
        self.cfg.extraction_prompt = Some(v.into());
        self
    }
    /// Set extraction model.
    pub fn extraction_model(mut self, v: ExtractionModel) -> Self {
        self.cfg.extraction_model = Some(v);
        self
    }
    /// Set wait-for-selector.
    pub fn wait_for_selector(mut self, v: impl Into<String>) -> Self {
        self.cfg.wait_for_selector = Some(v.into());
        self
    }
    /// Set extra rendering wait (ms).
    pub fn rendering_wait(mut self, v: u32) -> Self {
        self.cfg.rendering_wait = Some(v);
        self
    }
    /// Enable auto-scroll.
    pub fn auto_scroll(mut self, v: bool) -> Self {
        self.cfg.auto_scroll = v;
        self
    }
    /// Add a named screenshot.
    pub fn screenshot(mut self, name: impl Into<String>, selector: impl Into<String>) -> Self {
        self.cfg.screenshots.insert(name.into(), selector.into());
        self
    }
    /// Add a screenshot flag.
    pub fn screenshot_flag(mut self, v: ScreenshotFlag) -> Self {
        self.cfg.screenshot_flags.push(v);
        self
    }
    /// Set inline JS code.
    pub fn js(mut self, v: impl Into<String>) -> Self {
        self.cfg.js = Some(v.into());
        self
    }
    /// Set JS scenario (as serde_json::Value).
    pub fn js_scenario(mut self, v: serde_json::Value) -> Self {
        self.cfg.js_scenario = Some(v);
        self
    }
    /// Set OS fingerprint hint.
    pub fn os(mut self, v: impl Into<String>) -> Self {
        self.cfg.os = Some(v.into());
        self
    }
    /// Add an Accept-Language value.
    pub fn lang(mut self, v: impl Into<String>) -> Self {
        self.cfg.lang.push(v.into());
        self
    }
    /// Set browser brand.
    pub fn browser_brand(mut self, v: impl Into<String>) -> Self {
        self.cfg.browser_brand = Some(v.into());
        self
    }
    /// Enable proxified response mode (raw upstream pass-through).
    /// Spoof browser geolocation. Format: `"latitude,longitude"`.
    pub fn geolocation(mut self, v: impl Into<String>) -> Self {
        self.cfg.geolocation = Some(v.into());
        self
    }
    /// Set page load stage: `"complete"` (default) or `"domcontentloaded"`.
    pub fn rendering_stage(mut self, v: impl Into<String>) -> Self {
        self.cfg.rendering_stage = Some(v.into());
        self
    }
    /// Enable proxified response mode (raw upstream pass-through).
    pub fn proxified_response(mut self) -> Self {
        self.cfg.proxified_response = true;
        self
    }

    /// Finalize the builder, enforcing the mutual-exclusion rules for the
    /// extraction fields.
    pub fn build(self) -> Result<ScrapeConfig, ScrapflyError> {
        let cfg = self.cfg;
        let count = [
            cfg.extraction_template.is_some(),
            cfg.extraction_ephemeral_template.is_some(),
            cfg.extraction_prompt.is_some(),
            cfg.extraction_model.is_some(),
        ]
        .iter()
        .filter(|x| **x)
        .count();
        if count > 1 {
            return Err(ScrapflyError::Config(
                "extraction_template, extraction_ephemeral_template, extraction_prompt and extraction_model are mutually exclusive"
                    .into(),
            ));
        }
        Ok(cfg)
    }
}
