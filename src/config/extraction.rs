//! Extraction endpoint configuration — ported from `sdk/go/config_extraction.go`.

use crate::enums::{CompressionFormat, ExtractionModel};
use crate::error::ScrapflyError;

use super::url_safe_b64_encode;

/// Configuration for a `POST /extraction` request.
#[derive(Debug, Clone, Default)]
pub struct ExtractionConfig {
    /// Document bytes (required).
    pub body: Vec<u8>,
    /// Content type, e.g. `text/html` (required).
    pub content_type: String,
    /// Original URL (helps the AI with context).
    pub url: Option<String>,
    /// Character set.
    pub charset: Option<String>,
    /// Saved extraction template name.
    pub extraction_template: Option<String>,
    /// Inline (ephemeral) template.
    pub extraction_ephemeral_template: Option<serde_json::Value>,
    /// AI extraction prompt.
    pub extraction_prompt: Option<String>,
    /// Extraction model.
    pub extraction_model: Option<ExtractionModel>,
    /// Body is compressed.
    pub is_document_compressed: bool,
    /// Compression format.
    pub document_compression_format: Option<CompressionFormat>,
    /// Webhook name.
    pub webhook: Option<String>,
    /// Maximum time in seconds for extraction processing.
    pub timeout: Option<u32>,
}

impl ExtractionConfig {
    /// Start a builder.
    pub fn builder(body: Vec<u8>, content_type: impl Into<String>) -> ExtractionConfigBuilder {
        ExtractionConfigBuilder {
            cfg: ExtractionConfig {
                body,
                content_type: content_type.into(),
                ..Default::default()
            },
        }
    }

    /// Query params (key is added separately by the client).
    pub fn to_query_pairs(&self) -> Result<Vec<(String, String)>, ScrapflyError> {
        if self.body.is_empty() {
            return Err(ScrapflyError::Config("body is required".into()));
        }
        if self.content_type.is_empty() {
            return Err(ScrapflyError::Config("content_type is required".into()));
        }
        let tpl_count = [
            self.extraction_template.is_some(),
            self.extraction_ephemeral_template.is_some(),
        ]
        .iter()
        .filter(|x| **x)
        .count();
        if tpl_count > 1 {
            return Err(ScrapflyError::Config(
                "cannot use both extraction_template and extraction_ephemeral_template".into(),
            ));
        }

        let mut out = Vec::new();
        out.push(("content_type".into(), self.content_type.clone()));
        if let Some(u) = &self.url {
            out.push(("url".into(), u.clone()));
        }
        if let Some(c) = &self.charset {
            out.push(("charset".into(), c.clone()));
        }
        if let Some(t) = &self.extraction_template {
            out.push(("extraction_template".into(), t.clone()));
        }
        if let Some(t) = &self.extraction_ephemeral_template {
            let s = serde_json::to_string(t)?;
            out.push((
                "extraction_template".into(),
                format!("ephemeral:{}", url_safe_b64_encode(&s)),
            ));
        }
        if let Some(p) = &self.extraction_prompt {
            out.push(("extraction_prompt".into(), p.clone()));
        }
        if let Some(m) = self.extraction_model {
            out.push(("extraction_model".into(), m.as_str().into()));
        }
        if let Some(wh) = &self.webhook {
            out.push(("webhook_name".into(), wh.clone()));
        }
        if let Some(t) = self.timeout {
            out.push(("timeout".into(), t.to_string()));
        }
        Ok(out)
    }
}

/// Builder for [`ExtractionConfig`].
#[derive(Debug, Clone)]
pub struct ExtractionConfigBuilder {
    cfg: ExtractionConfig,
}

impl ExtractionConfigBuilder {
    /// Original URL.
    pub fn url(mut self, v: impl Into<String>) -> Self {
        self.cfg.url = Some(v.into());
        self
    }
    /// Character set.
    pub fn charset(mut self, v: impl Into<String>) -> Self {
        self.cfg.charset = Some(v.into());
        self
    }
    /// Saved template name.
    pub fn extraction_template(mut self, v: impl Into<String>) -> Self {
        self.cfg.extraction_template = Some(v.into());
        self
    }
    /// Inline template.
    pub fn extraction_ephemeral_template(mut self, v: serde_json::Value) -> Self {
        self.cfg.extraction_ephemeral_template = Some(v);
        self
    }
    /// AI prompt.
    pub fn extraction_prompt(mut self, v: impl Into<String>) -> Self {
        self.cfg.extraction_prompt = Some(v.into());
        self
    }
    /// Model.
    pub fn extraction_model(mut self, v: ExtractionModel) -> Self {
        self.cfg.extraction_model = Some(v);
        self
    }
    /// Body is compressed.
    pub fn is_document_compressed(mut self, v: bool) -> Self {
        self.cfg.is_document_compressed = v;
        self
    }
    /// Compression format.
    pub fn document_compression_format(mut self, v: CompressionFormat) -> Self {
        self.cfg.document_compression_format = Some(v);
        self
    }
    /// Webhook name.
    pub fn timeout(mut self, v: u32) -> Self {
        self.cfg.timeout = Some(v);
        self
    }
    /// Set webhook name for post-extraction notification.
    pub fn webhook(mut self, v: impl Into<String>) -> Self {
        self.cfg.webhook = Some(v.into());
        self
    }
    /// Finalize the builder.
    pub fn build(self) -> Result<ExtractionConfig, ScrapflyError> {
        if self.cfg.body.is_empty() {
            return Err(ScrapflyError::Config("body is required".into()));
        }
        if self.cfg.content_type.is_empty() {
            return Err(ScrapflyError::Config("content_type is required".into()));
        }
        Ok(self.cfg)
    }
}
