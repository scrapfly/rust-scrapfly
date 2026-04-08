//! Typed config builders for every Scrapfly endpoint.
//!
//! - `scrape` → query-param based (`to_query_pairs`)
//! - `screenshot` → query-param based
//! - `extraction` → JSON body + query params
//! - `crawler` → JSON body with bounds validation

pub mod crawler;
pub mod extraction;
pub mod scrape;
pub mod screenshot;

use base64::Engine;

/// URL-safe base64 without padding, matching `sdk/go/utils.go::urlSafeB64Encode`.
pub(crate) fn url_safe_b64_encode(data: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data.as_bytes())
}
