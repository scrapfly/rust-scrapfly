//! Classify request + result types.
//!
//! Mirrors the `POST /classify` wire contract. See
//! <https://scrapfly.io/docs/scrape-api/classify>.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Describes an HTTP response to classify.
#[derive(Debug, Clone, Serialize)]
pub struct ClassifyRequest {
    /// Final URL the response came from.
    pub url: String,
    /// HTTP status code (100-599) of the response.
    pub status_code: u16,
    /// Response headers (case-insensitive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    /// Response body as text. Binary bodies should be passed as empty/None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// HTTP method the caller used. Defaults to `GET` server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
}

/// `POST /classify` response.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClassifyResult {
    /// `true` when Scrapfly detected an anti-bot block on the upstream response.
    #[serde(default)]
    pub blocked: bool,
    /// Name of the anti-bot product that matched, when one was detected.
    #[serde(default)]
    pub antibot: Option<String>,
    /// API credits charged for this call.
    #[serde(default)]
    pub cost: u32,
}
