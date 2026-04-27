//! Public schedule client — wraps `/scrape/schedules`, `/screenshot/schedules`,
//! `/crawl/schedules` and the cross-kind `/schedules` endpoints.
//!
//! Mirrors the Go and Python SDKs: the kind-specific configuration is supplied
//! to the matching `create_*_schedule` method; cross-kind list/get/update/
//! delete/pause/resume/execute work on any schedule by id.

use std::collections::HashMap;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::Client;
use crate::error::{from_response, ScrapflyError};

/// Bounds a recurring schedule by either a date or a fire count.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScheduleEnd {
    /// End mode: `"date"` (stop at `date`) or `"count"` (stop after `count` fires).
    #[serde(rename = "type")]
    pub kind: String,
    /// ISO8601 datetime at which the schedule stops firing (when `kind == "date"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Number of remaining fires before the schedule stops (when `kind == "count"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
}

/// When a schedule fires next. Cron mode wins when `cron` is set; otherwise
/// `interval` + `unit` drive the cadence.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScheduleRecurrence {
    /// Cron expression evaluated in UTC. Wins over `interval`/`unit` when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    /// Numeric component of the cadence (e.g. `5` for "every 5 minutes").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<i64>,
    /// Cadence unit: `"minute" | "hour" | "day" | "week" | "month"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Optional weekday filter (e.g. `["mon", "wed", "fri"]`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<Vec<String>>,
    /// Optional end-of-recurrence condition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ends: Option<ScheduleEnd>,
}

/// Public-facing request envelope for creating a schedule. The kind-specific
/// config is supplied as a separate argument.
#[derive(Debug, Clone, Serialize, Default)]
pub struct CreateScheduleRequest {
    /// Name of the webhook to deliver each fire's result to.
    pub webhook_name: String,
    /// Recurring cadence. Mutually exclusive with `scheduled_date` (one-shot).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<ScheduleRecurrence>,
    /// One-shot fire datetime (ISO8601). Mutually exclusive with `recurrence`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_date: Option<String>,
    /// When true, multiple fires of the same schedule may run concurrently.
    pub allow_concurrency: bool,
    /// When true, failed fires are retried up to `max_retries` times.
    pub retry_on_failure: bool,
    /// Cap on retries per fire when `retry_on_failure` is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<i64>,
    /// Free-form description shown on the dashboard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// PATCH payload. Only fields explicitly set are forwarded.
#[derive(Debug, Clone, Serialize, Default)]
pub struct UpdateScheduleRequest {
    /// Replace the recurrence cadence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<ScheduleRecurrence>,
    /// Replace the one-shot fire datetime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_date: Option<String>,
    /// Replace the concurrency flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_concurrency: Option<bool>,
    /// Replace the retry-on-failure flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_on_failure: Option<bool>,
    /// Replace the per-fire retry cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<i64>,
    /// Replace the dashboard notes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Replace the scrape config (only valid for scrape schedules).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scrape_config: Option<HashMap<String, Value>>,
    /// Replace the screenshot config (only valid for screenshot schedules).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot_config: Option<HashMap<String, Value>>,
    /// Replace the crawler config (only valid for crawler schedules).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crawler_config: Option<HashMap<String, Value>>,
}

/// Server-side schedule record. Returned by every read or mutation endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct Schedule {
    /// Server-issued schedule UUID.
    pub id: String,
    /// Kind of the underlying job: `"scrape" | "screenshot" | "crawler"`.
    pub kind: String,
    /// Lifecycle status: `"ACTIVE" | "PAUSED" | "CANCELLED"`.
    pub status: String,
    /// ISO8601 datetime of the next planned fire (recurring schedules).
    #[serde(default)]
    pub next_scheduled_date: Option<String>,
    /// ISO8601 datetime of the one-shot fire (one-shot schedules).
    #[serde(default)]
    pub scheduled_date: Option<String>,
    /// Recurrence cadence for recurring schedules.
    #[serde(default)]
    pub recurrence: Option<ScheduleRecurrence>,
    /// Free-form server-side metadata bag.
    #[serde(default)]
    pub metadata: Option<HashMap<String, Value>>,
    /// Free-form notes attached at create / update time.
    #[serde(default)]
    pub notes: Option<String>,
    /// User UUID that authored the schedule.
    #[serde(default)]
    pub created_by: Option<String>,
    /// ISO8601 datetime of creation.
    pub created_at: String,
    /// ISO8601 datetime of the last update.
    pub updated_at: String,
    /// ISO8601 datetime of cancellation (set when `status == "CANCELLED"`).
    #[serde(default)]
    pub cancelled_at: Option<String>,
    /// Whether overlapping fires are permitted.
    pub allow_concurrency: bool,
    /// Whether failed fires are retried.
    pub retry_on_failure: bool,
    /// Per-fire retry cap when `retry_on_failure` is set.
    pub max_retries: i64,
    /// UUID of the webhook receiving each fire's result.
    #[serde(default)]
    pub webhook_uuid: Option<String>,
    /// UUID of the owning user.
    #[serde(default)]
    pub user_uuid: Option<String>,
    /// Consecutive failure counter (auto-cancels after a server-defined cap).
    #[serde(default)]
    pub consecutive_failures: Option<i64>,
}

/// Optional filters for [`Client::list_schedules`].
#[derive(Debug, Clone, Default)]
pub struct ListSchedulesOptions {
    /// Restrict to a single status (`"ACTIVE" | "PAUSED" | "CANCELLED"`).
    pub status: Option<String>,
    /// Restrict to a single kind (`"scrape" | "screenshot" | "crawler"`).
    pub kind: Option<String>,
}

impl Client {
    /// Create a Web Scraping API schedule.
    pub async fn create_scrape_schedule(
        &self,
        scrape_config: HashMap<String, Value>,
        request: &CreateScheduleRequest,
    ) -> Result<Schedule, ScrapflyError> {
        self.create_schedule_inner("/scrape/schedules", "scrape_config", scrape_config, request)
            .await
    }

    /// Create a Screenshot API schedule.
    pub async fn create_screenshot_schedule(
        &self,
        screenshot_config: HashMap<String, Value>,
        request: &CreateScheduleRequest,
    ) -> Result<Schedule, ScrapflyError> {
        self.create_schedule_inner(
            "/screenshot/schedules",
            "screenshot_config",
            screenshot_config,
            request,
        )
        .await
    }

    /// Create a Crawler API schedule.
    pub async fn create_crawler_schedule(
        &self,
        crawler_config: HashMap<String, Value>,
        request: &CreateScheduleRequest,
    ) -> Result<Schedule, ScrapflyError> {
        self.create_schedule_inner(
            "/crawl/schedules",
            "crawler_config",
            crawler_config,
            request,
        )
        .await
    }

    /// Return a schedule by id (works across all kinds).
    pub async fn get_schedule(&self, id: &str) -> Result<Schedule, ScrapflyError> {
        let path = format!("/schedules/{}", url_path_escape(id));
        self.schedule_request_json::<Schedule>(Method::GET, &path, &[], None)
            .await
    }

    /// List every schedule on the account, optionally filtered by kind/status.
    pub async fn list_schedules(
        &self,
        opts: Option<&ListSchedulesOptions>,
    ) -> Result<Vec<Schedule>, ScrapflyError> {
        self.list_schedules_inner("/schedules", opts).await
    }

    /// List scrape schedules, optionally filtered by `status`.
    pub async fn list_scrape_schedules(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<Schedule>, ScrapflyError> {
        self.list_schedules_inner(
            "/scrape/schedules",
            status
                .map(|s| ListSchedulesOptions {
                    status: Some(s.into()),
                    kind: None,
                })
                .as_ref(),
        )
        .await
    }

    /// List screenshot schedules, optionally filtered by `status`.
    pub async fn list_screenshot_schedules(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<Schedule>, ScrapflyError> {
        self.list_schedules_inner(
            "/screenshot/schedules",
            status
                .map(|s| ListSchedulesOptions {
                    status: Some(s.into()),
                    kind: None,
                })
                .as_ref(),
        )
        .await
    }

    /// List crawler schedules, optionally filtered by `status`.
    pub async fn list_crawler_schedules(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<Schedule>, ScrapflyError> {
        self.list_schedules_inner(
            "/crawl/schedules",
            status
                .map(|s| ListSchedulesOptions {
                    status: Some(s.into()),
                    kind: None,
                })
                .as_ref(),
        )
        .await
    }

    /// Patch an active schedule. Only fields set in `request` change.
    pub async fn update_schedule(
        &self,
        id: &str,
        request: &UpdateScheduleRequest,
    ) -> Result<Schedule, ScrapflyError> {
        let path = format!("/schedules/{}", url_path_escape(id));
        let body = serde_json::to_vec(request)?;
        self.schedule_request_json::<Schedule>(Method::PATCH, &path, &[], Some(body))
            .await
    }

    /// Cancel a schedule. Cancellation is terminal (returns no body).
    pub async fn cancel_schedule(&self, id: &str) -> Result<(), ScrapflyError> {
        let path = format!("/schedules/{}", url_path_escape(id));
        self.schedule_request_empty(Method::DELETE, &path, &[], None)
            .await
    }

    /// Pause an active schedule. Idempotent on already-paused schedules.
    pub async fn pause_schedule(&self, id: &str) -> Result<Schedule, ScrapflyError> {
        let path = format!("/schedules/{}/pause", url_path_escape(id));
        self.schedule_request_json::<Schedule>(Method::POST, &path, &[], None)
            .await
    }

    /// Resume a paused schedule. Idempotent on already-active schedules.
    pub async fn resume_schedule(&self, id: &str) -> Result<Schedule, ScrapflyError> {
        let path = format!("/schedules/{}/resume", url_path_escape(id));
        self.schedule_request_json::<Schedule>(Method::POST, &path, &[], None)
            .await
    }

    /// Fire a schedule immediately, regardless of `next_scheduled_date`.
    pub async fn execute_schedule(&self, id: &str) -> Result<Schedule, ScrapflyError> {
        let path = format!("/schedules/{}/execute", url_path_escape(id));
        self.schedule_request_json::<Schedule>(Method::POST, &path, &[], None)
            .await
    }

    // ---- internals ------------------------------------------------------

    async fn create_schedule_inner(
        &self,
        path: &str,
        config_key: &'static str,
        config: HashMap<String, Value>,
        request: &CreateScheduleRequest,
    ) -> Result<Schedule, ScrapflyError> {
        let mut body = serde_json::Map::new();
        body.insert(config_key.to_string(), Value::Object(map_to_object(config)));
        body.insert(
            "webhook_name".into(),
            Value::String(request.webhook_name.clone()),
        );
        body.insert(
            "allow_concurrency".into(),
            Value::Bool(request.allow_concurrency),
        );
        body.insert(
            "retry_on_failure".into(),
            Value::Bool(request.retry_on_failure),
        );
        if let Some(rec) = &request.recurrence {
            body.insert("recurrence".into(), serde_json::to_value(rec)?);
        }
        if let Some(d) = &request.scheduled_date {
            body.insert("scheduled_date".into(), Value::String(d.clone()));
        }
        if let Some(n) = request.max_retries {
            body.insert("max_retries".into(), Value::Number(n.into()));
        }
        if let Some(n) = &request.notes {
            body.insert("notes".into(), Value::String(n.clone()));
        }
        let payload = serde_json::to_vec(&Value::Object(body))?;
        self.schedule_request_json::<Schedule>(Method::POST, path, &[], Some(payload))
            .await
    }

    async fn list_schedules_inner(
        &self,
        path: &str,
        opts: Option<&ListSchedulesOptions>,
    ) -> Result<Vec<Schedule>, ScrapflyError> {
        let mut query: Vec<(String, String)> = Vec::new();
        if let Some(o) = opts {
            if let Some(s) = &o.status {
                query.push(("status".into(), s.clone()));
            }
            if let Some(k) = &o.kind {
                query.push(("kind".into(), k.clone()));
            }
        }
        self.schedule_request_json::<Vec<Schedule>>(Method::GET, path, &query, None)
            .await
    }

    async fn schedule_request_json<T: for<'de> serde::Deserialize<'de>>(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Vec<u8>>,
    ) -> Result<T, ScrapflyError> {
        let (status, body_bytes) = self.schedule_send(method, path, query, body).await?;
        if status == 204 {
            return Err(ScrapflyError::Config(
                "schedule endpoint returned 204 but a JSON body was expected".into(),
            ));
        }
        if status >= 400 {
            return Err(from_response(status, &body_bytes, 0, false));
        }
        Ok(serde_json::from_slice(&body_bytes)?)
    }

    async fn schedule_request_empty(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Vec<u8>>,
    ) -> Result<(), ScrapflyError> {
        let (status, body_bytes) = self.schedule_send(method, path, query, body).await?;
        if status >= 400 {
            return Err(from_response(status, &body_bytes, 0, false));
        }
        Ok(())
    }

    async fn schedule_send(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Vec<u8>>,
    ) -> Result<(u16, bytes::Bytes), ScrapflyError> {
        let url = self.build_url_public(path, query)?;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        if body.is_some() {
            headers.insert(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_static("application/json"),
            );
        }
        let resp = self
            .send_simple_public(method, url, Some(headers), body)
            .await?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        Ok((status, bytes))
    }
}

fn map_to_object(map: HashMap<String, Value>) -> serde_json::Map<String, Value> {
    map.into_iter().collect()
}

// url_path_escape is a minimal percent-encoder for path segments. We escape
// the chars that would corrupt a URL (`/?#`, whitespace, control chars).
// Server-issued IDs are UUIDs in practice; this exists for defense.
fn url_path_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
