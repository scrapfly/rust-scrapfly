//! Screenshot endpoint configuration — ported from `sdk/go/config_screenshot.go`.

use crate::enums::{ScreenshotFormat, ScreenshotOption, VisionDeficiencyType};
use crate::error::ScrapflyError;

use super::url_safe_b64_encode;

/// Configuration for a `POST /screenshot` request.
#[derive(Debug, Clone, Default)]
pub struct ScreenshotConfig {
    /// Target URL (required).
    pub url: String,
    /// Image format.
    pub format: Option<ScreenshotFormat>,
    /// `fullpage` or a CSS selector.
    pub capture: Option<String>,
    /// Viewport resolution (e.g. `1920x1080`).
    pub resolution: Option<String>,
    /// Proxy country.
    pub country: Option<String>,
    /// Timeout in milliseconds.
    pub timeout: Option<u32>,
    /// Extra rendering wait (ms).
    pub rendering_wait: Option<u32>,
    /// Wait for CSS selector.
    pub wait_for_selector: Option<String>,
    /// Capture options.
    pub options: Vec<ScreenshotOption>,
    /// Enable auto-scroll.
    pub auto_scroll: bool,
    /// Custom JavaScript (base64url-encoded on the wire).
    pub js: Option<String>,
    /// Enable cache.
    pub cache: bool,
    /// Cache TTL (seconds).
    pub cache_ttl: Option<u32>,
    /// Force cache refresh.
    pub cache_clear: bool,
    /// Webhook name.
    pub webhook: Option<String>,
    /// Simulated vision deficiency.
    pub vision_deficiency: Option<VisionDeficiencyType>,
}

impl ScreenshotConfig {
    /// Start a builder for `url`.
    pub fn builder(url: impl Into<String>) -> ScreenshotConfigBuilder {
        ScreenshotConfigBuilder {
            cfg: ScreenshotConfig {
                url: url.into(),
                ..Default::default()
            },
        }
    }

    /// Serialize into query-param pairs. Mirrors `toAPIParams` in the Go SDK.
    pub fn to_query_pairs(&self) -> Result<Vec<(String, String)>, ScrapflyError> {
        if self.url.is_empty() {
            return Err(ScrapflyError::Config("url is required".into()));
        }
        let mut out: Vec<(String, String)> = Vec::new();
        out.push(("url".into(), self.url.clone()));
        if let Some(f) = self.format {
            out.push(("format".into(), f.as_str().into()));
        }
        if let Some(c) = &self.capture {
            out.push(("capture".into(), c.clone()));
        }
        if let Some(r) = &self.resolution {
            out.push(("resolution".into(), r.clone()));
        }
        if let Some(c) = &self.country {
            out.push(("country".into(), c.clone()));
        }
        if let Some(t) = self.timeout {
            out.push(("timeout".into(), t.to_string()));
        }
        if let Some(w) = self.rendering_wait {
            out.push(("rendering_wait".into(), w.to_string()));
        }
        if let Some(s) = &self.wait_for_selector {
            out.push(("wait_for_selector".into(), s.clone()));
        }
        if self.auto_scroll {
            out.push(("auto_scroll".into(), "true".into()));
        }
        if let Some(js) = &self.js {
            out.push(("js".into(), url_safe_b64_encode(js)));
        }
        if !self.options.is_empty() {
            let joined = self
                .options
                .iter()
                .map(|o| o.as_str())
                .collect::<Vec<_>>()
                .join(",");
            out.push(("options".into(), joined));
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
        if let Some(wh) = &self.webhook {
            out.push(("webhook_name".into(), wh.clone()));
        }
        if let Some(vd) = self.vision_deficiency {
            out.push(("vision_deficiency".into(), vd.as_str().into()));
        }
        Ok(out)
    }
}

/// Builder for [`ScreenshotConfig`].
#[derive(Debug, Clone)]
pub struct ScreenshotConfigBuilder {
    cfg: ScreenshotConfig,
}

impl ScreenshotConfigBuilder {
    /// Set output format.
    pub fn format(mut self, f: ScreenshotFormat) -> Self {
        self.cfg.format = Some(f);
        self
    }
    /// Set capture target.
    pub fn capture(mut self, c: impl Into<String>) -> Self {
        self.cfg.capture = Some(c.into());
        self
    }
    /// Set viewport resolution.
    pub fn resolution(mut self, r: impl Into<String>) -> Self {
        self.cfg.resolution = Some(r.into());
        self
    }
    /// Set proxy country.
    pub fn country(mut self, c: impl Into<String>) -> Self {
        self.cfg.country = Some(c.into());
        self
    }
    /// Set timeout (ms).
    pub fn timeout(mut self, t: u32) -> Self {
        self.cfg.timeout = Some(t);
        self
    }
    /// Set rendering wait (ms).
    pub fn rendering_wait(mut self, t: u32) -> Self {
        self.cfg.rendering_wait = Some(t);
        self
    }
    /// Set wait-for-selector.
    pub fn wait_for_selector(mut self, s: impl Into<String>) -> Self {
        self.cfg.wait_for_selector = Some(s.into());
        self
    }
    /// Add a capture option.
    pub fn option(mut self, o: ScreenshotOption) -> Self {
        self.cfg.options.push(o);
        self
    }
    /// Enable auto-scroll.
    pub fn auto_scroll(mut self, v: bool) -> Self {
        self.cfg.auto_scroll = v;
        self
    }
    /// Set custom JS.
    pub fn js(mut self, js: impl Into<String>) -> Self {
        self.cfg.js = Some(js.into());
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
    /// Set webhook name.
    pub fn webhook(mut self, v: impl Into<String>) -> Self {
        self.cfg.webhook = Some(v.into());
        self
    }
    /// Set vision deficiency simulation.
    pub fn vision_deficiency(mut self, v: VisionDeficiencyType) -> Self {
        self.cfg.vision_deficiency = Some(v);
        self
    }
    /// Finalize the builder.
    pub fn build(self) -> Result<ScreenshotConfig, ScrapflyError> {
        if self.cfg.url.is_empty() {
            return Err(ScrapflyError::Config("url is required".into()));
        }
        Ok(self.cfg)
    }
}
