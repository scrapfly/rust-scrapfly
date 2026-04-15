//! Monitoring API — aggregated + per-target metrics.
//!
//! Wraps `GET /scrape/monitoring/metrics` and
//! `GET /scrape/monitoring/metrics/target`. Enterprise plan only.
//! See <https://scrapfly.io/docs/monitoring#api>.

use serde::{Deserialize, Serialize};

/// Response format for the Monitoring API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MonitoringDataFormat {
    /// Structured JSON aggregates (default).
    Structured,
    /// Prometheus text exposition.
    Prometheus,
}

impl MonitoringDataFormat {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Structured => "structured",
            Self::Prometheus => "prometheus",
        }
    }
}

/// Pre-defined monitoring time window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MonitoringPeriod {
    /// Last 5 minutes.
    #[serde(rename = "last5m")]
    Last5m,
    /// Last 1 hour.
    #[serde(rename = "last1h")]
    Last1h,
    /// Last 24 hours.
    #[serde(rename = "last24h")]
    Last24h,
    /// Last 7 days.
    #[serde(rename = "last7d")]
    Last7d,
    /// Current subscription period.
    #[serde(rename = "subscription")]
    Subscription,
}

impl MonitoringPeriod {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Last5m => "last5m",
            Self::Last1h => "last1h",
            Self::Last24h => "last24h",
            Self::Last7d => "last7d",
            Self::Subscription => "subscription",
        }
    }
}

/// Aggregation level for `/scrape/monitoring/metrics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MonitoringAggregation {
    /// Account-level totals.
    Account,
    /// Per-project aggregates.
    Project,
    /// Top-100 targets.
    Target,
}

impl MonitoringAggregation {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Project => "project",
            Self::Target => "target",
        }
    }
}

/// Options for the request-based metrics endpoints
/// (`get_*_monitoring_metrics`).
#[derive(Debug, Clone, Default)]
pub struct MonitoringMetricsOptions {
    /// Response format (default: `Structured`).
    pub format: Option<MonitoringDataFormat>,
    /// Pre-defined time window.
    pub period: Option<MonitoringPeriod>,
    /// Aggregation levels to include (combinable).
    pub aggregation: Option<Vec<MonitoringAggregation>>,
    /// Fold events with `origin=WEBHOOK` (callbacks executed by the
    /// webhook worker) into this product's totals. Defaults to `false`
    /// to match the dashboard's default view.
    pub include_webhook: bool,
}

/// Options for the request-based per-target endpoints
/// (`get_*_monitoring_target_metrics`).
///
/// `start` and `end` are pre-formatted UTC strings in the Scrapfly API
/// format `YYYY-MM-DD HH:MM:SS`. They are mutually exclusive with `period`
/// and must be provided together.
#[derive(Debug, Clone)]
pub struct MonitoringTargetMetricsOptions {
    /// Target root domain (e.g. `httpbin.dev`). Required.
    pub domain: String,
    /// Group subdomains under the root. Defaults to `false`.
    pub group_subdomain: bool,
    /// Pre-defined window. Ignored if `start`/`end` are provided.
    pub period: Option<MonitoringPeriod>,
    /// Custom window start (UTC, `YYYY-MM-DD HH:MM:SS`). Must be set with `end`.
    pub start: Option<String>,
    /// Custom window end (UTC, `YYYY-MM-DD HH:MM:SS`). Must be set with `start`.
    pub end: Option<String>,
    /// Fold WEBHOOK origin into this product's totals.
    pub include_webhook: bool,
}

impl MonitoringTargetMetricsOptions {
    /// Convenience constructor: query a single domain with the default
    /// pre-defined window.
    pub fn for_domain(domain: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            group_subdomain: false,
            period: None,
            start: None,
            end: None,
            include_webhook: false,
        }
    }
}

/// Options for the Cloud Browser monitoring endpoints. Cloud Browser is
/// session-based (one allocation = one long-lived browser, billed by
/// runtime + bandwidth) and exposes a distinct shape from the
/// request-based products. There is no `domain`/`target` and no
/// `include_webhook`.
#[derive(Debug, Clone, Default)]
pub struct CloudBrowserMonitoringOptions {
    /// Pre-defined window. Ignored if `start`/`end` are provided.
    pub period: Option<MonitoringPeriod>,
    /// Optional filter to a single proxy pool (e.g. `public_datacenter_pool`).
    pub proxy_pool: Option<String>,
    /// Custom window start (UTC, `YYYY-MM-DD HH:MM:SS`). Must be set with `end`.
    pub start: Option<String>,
    /// Custom window end (UTC, `YYYY-MM-DD HH:MM:SS`). Must be set with `start`.
    pub end: Option<String>,
}
