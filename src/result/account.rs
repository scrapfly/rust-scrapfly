//! Account result — port of `sdk/go/result_account.go`.

use serde::Deserialize;

/// `GET /account` response.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AccountData {
    /// Account-level info.
    #[serde(default)]
    pub account: serde_json::Value,
    /// Active project info.
    #[serde(default)]
    pub project: serde_json::Value,
    /// Subscription info (includes usage counters).
    #[serde(default)]
    pub subscription: serde_json::Value,
}

impl AccountData {
    /// Extract `subscription.usage.scrape.concurrent_limit`, best-effort.
    /// Returns 0 if the field is missing or not an integer.
    pub fn concurrent_limit(&self) -> u32 {
        self.subscription
            .pointer("/usage/scrape/concurrent_limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(0)
    }
}

/// Response from API-key verification calls.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct VerifyApiKeyResult {
    /// Whether the key is valid.
    #[serde(default)]
    pub valid: bool,
}
