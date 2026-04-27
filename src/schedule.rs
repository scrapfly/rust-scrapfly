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
    #[serde(rename = "type")]
    pub kind: String, // "date" | "count"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
}

/// When a schedule fires next. Cron mode wins when `cron` is set; otherwise
/// `interval` + `unit` drive the cadence.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScheduleRecurrence {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>, // "minute" | "hour" | "day" | "week" | "month"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ends: Option<ScheduleEnd>,
}

/// Public-facing request envelope for creating a schedule. The kind-specific
/// config is supplied as a separate argument.
#[derive(Debug, Clone, Serialize, Default)]
pub struct CreateScheduleRequest {
    pub webhook_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<ScheduleRecurrence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_date: Option<String>,
    pub allow_concurrency: bool,
    pub retry_on_failure: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// PATCH payload. Only fields explicitly set are forwarded.
#[derive(Debug, Clone, Serialize, Default)]
pub struct UpdateScheduleRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<ScheduleRecurrence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_concurrency: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_on_failure: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scrape_config: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot_config: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crawler_config: Option<HashMap<String, Value>>,
}

/// Server-side schedule record. Returned by every read or mutation endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub kind: String,
    pub status: String,
    #[serde(default)]
    pub next_scheduled_date: Option<String>,
    #[serde(default)]
    pub scheduled_date: Option<String>,
    #[serde(default)]
    pub recurrence: Option<ScheduleRecurrence>,
    #[serde(default)]
    pub metadata: Option<HashMap<String, Value>>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub cancelled_at: Option<String>,
    pub allow_concurrency: bool,
    pub retry_on_failure: bool,
    pub max_retries: i64,
    #[serde(default)]
    pub webhook_uuid: Option<String>,
    #[serde(default)]
    pub user_uuid: Option<String>,
    #[serde(default)]
    pub consecutive_failures: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct ListSchedulesOptions {
    pub status: Option<String>,
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

    pub async fn pause_schedule(&self, id: &str) -> Result<Schedule, ScrapflyError> {
        let path = format!("/schedules/{}/pause", url_path_escape(id));
        self.schedule_request_json::<Schedule>(Method::POST, &path, &[], None)
            .await
    }

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
