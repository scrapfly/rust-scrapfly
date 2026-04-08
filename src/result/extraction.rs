//! Extraction result — port of `ExtractionResult` in `sdk/go/result_scrape.go`.

use serde::Deserialize;

/// Response envelope from `POST /extraction`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExtractionResult {
    /// Extracted data (shape depends on the template/prompt).
    #[serde(default)]
    pub data: serde_json::Value,
    /// Content type of the input document.
    #[serde(default)]
    pub content_type: String,
    /// Quality/confidence marker (shape depends on the model).
    #[serde(default)]
    pub data_quality: serde_json::Value,
}
