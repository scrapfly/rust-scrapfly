//! Cloud Browser API — port of `sdk/go/cloud_browser.go`.

use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{from_response, ScrapflyError};

/// Configuration for a Cloud Browser WebSocket session (passed to
/// [`cloud_browser_url`]).
#[derive(Debug, Clone, Default, Serialize)]
pub struct BrowserConfig {
    /// Proxy pool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_pool: Option<String>,
    /// OS fingerprint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    /// Proxy country.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// Session name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Session timeout (seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
    /// Block images.
    #[serde(skip_serializing_if = "is_false")]
    pub block_images: bool,
    /// Block stylesheets.
    #[serde(skip_serializing_if = "is_false")]
    pub block_styles: bool,
    /// Block fonts.
    #[serde(skip_serializing_if = "is_false")]
    pub block_fonts: bool,
    /// Block media.
    #[serde(skip_serializing_if = "is_false")]
    pub block_media: bool,
    /// Enable screenshot capability.
    #[serde(skip_serializing_if = "is_false")]
    pub screenshot: bool,
    /// Enable cache.
    #[serde(skip_serializing_if = "is_false")]
    pub cache: bool,
    /// Enable blacklist.
    #[serde(skip_serializing_if = "is_false")]
    pub blacklist: bool,
    /// Debug.
    #[serde(skip_serializing_if = "is_false")]
    pub debug: bool,
    /// Resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    /// Browser brand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_brand: Option<String>,
    /// BYOP proxy URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byop_proxy: Option<String>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// Normalize an arbitrary Cloud Browser host to a `wss://` URL, regardless of
/// the scheme the caller configured. Accepted input schemes: `https://`
/// (default), `wss://`, `ws://`, `http://`, and bare `host[:port]`. Mirrors
/// `sdk/go/cloud_browser.go::wsBase`.
fn ws_base(host: &str) -> String {
    if let Some(rest) = host.strip_prefix("wss://") {
        format!("wss://{}", rest)
    } else if let Some(rest) = host.strip_prefix("ws://") {
        format!("ws://{}", rest)
    } else if let Some(rest) = host.strip_prefix("https://") {
        format!("wss://{}", rest)
    } else if let Some(rest) = host.strip_prefix("http://") {
        format!("ws://{}", rest)
    } else {
        format!("wss://{}", host)
    }
}

/// Normalize an arbitrary Cloud Browser host to its REST form (`https://` or
/// `http://`). Callers typically configure a `wss://` / `ws://` host (the CDP
/// entry point); the REST endpoints (`/unblock`, `/session/.../stop`) live on
/// the HTTP-equivalent origin. Mirrors `sdk/go/cloud_browser.go::restBase`.
fn rest_base(host: &str) -> String {
    if let Some(rest) = host.strip_prefix("wss://") {
        format!("https://{}", rest)
    } else if let Some(rest) = host.strip_prefix("ws://") {
        format!("http://{}", rest)
    } else if host.starts_with("https://") || host.starts_with("http://") {
        host.to_string()
    } else {
        format!("https://{}", host)
    }
}

/// Unblock request body.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UnblockConfig {
    /// Target URL.
    pub url: String,
    /// Proxy country.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// Navigation timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
    /// Browser session timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_timeout: Option<u32>,
}

/// Response from `POST /unblock`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct UnblockResult {
    /// WebSocket URL to connect to.
    #[serde(default)]
    pub ws_url: String,
    /// Session id.
    #[serde(default)]
    pub session_id: String,
    /// Run id.
    #[serde(default)]
    pub run_id: String,
}

impl Client {
    /// Build the WebSocket URL for a new Cloud Browser session.
    pub fn cloud_browser_url(&self, config: &BrowserConfig) -> String {
        let ws_host = ws_base(self.cloud_browser_host());
        let mut pairs: Vec<(String, String)> = vec![("api_key".into(), self.api_key().into())];
        if let Some(v) = &config.proxy_pool {
            pairs.push(("proxy_pool".into(), v.clone()));
        }
        if let Some(v) = &config.os {
            pairs.push(("os".into(), v.clone()));
        }
        if let Some(v) = &config.country {
            pairs.push(("country".into(), v.clone()));
        }
        if let Some(v) = &config.session {
            pairs.push(("session".into(), v.clone()));
        }
        if let Some(v) = config.timeout {
            pairs.push(("timeout".into(), v.to_string()));
        }
        if config.block_images {
            pairs.push(("block_images".into(), "true".into()));
        }
        if config.block_styles {
            pairs.push(("block_styles".into(), "true".into()));
        }
        if config.block_fonts {
            pairs.push(("block_fonts".into(), "true".into()));
        }
        if config.block_media {
            pairs.push(("block_media".into(), "true".into()));
        }
        if config.screenshot {
            pairs.push(("screenshot".into(), "true".into()));
        }
        if config.cache {
            pairs.push(("cache".into(), "true".into()));
        }
        if config.blacklist {
            pairs.push(("blacklist".into(), "true".into()));
        }
        if config.debug {
            pairs.push(("debug".into(), "true".into()));
        }
        if let Some(v) = &config.resolution {
            pairs.push(("resolution".into(), v.clone()));
        }
        if let Some(v) = &config.browser_brand {
            pairs.push(("browser_brand".into(), v.clone()));
        }
        if let Some(v) = &config.byop_proxy {
            pairs.push(("byop_proxy".into(), v.clone()));
        }
        let qs = serde_urlencoded::to_string(&pairs).unwrap_or_default();
        format!("{}?{}", ws_host, qs)
    }

    /// Call `POST /unblock` to bypass anti-bot protection.
    pub async fn cloud_browser_unblock(
        &self,
        config: &UnblockConfig,
    ) -> Result<UnblockResult, ScrapflyError> {
        let url = format!(
            "{}/unblock?key={}",
            rest_base(self.cloud_browser_host()),
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid unblock url: {}", e)))?;
        let body = serde_json::to_vec(config)?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let resp = self
            .send_with_retry(Method::POST, url, Some(headers), Some(body))
            .await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// List browser extensions for the account.
    pub async fn cloud_browser_extension_list(
        &self,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/extension?key={}",
            rest_base(self.cloud_browser_host()),
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid extension url: {}", e)))?;
        let resp = self.send_with_retry(Method::GET, url, None, None).await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Get details of a specific browser extension.
    pub async fn cloud_browser_extension_get(
        &self,
        extension_id: &str,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/extension/{}?key={}",
            rest_base(self.cloud_browser_host()),
            extension_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid extension url: {}", e)))?;
        let resp = self.send_with_retry(Method::GET, url, None, None).await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Upload a browser extension from a local file (.zip or .crx).
    pub async fn cloud_browser_extension_upload(
        &self,
        file_path: &std::path::Path,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/extension?key={}",
            rest_base(self.cloud_browser_host()),
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid extension url: {}", e)))?;
        let file_bytes = std::fs::read(file_path).map_err(|e| {
            ScrapflyError::Config(format!("failed to read extension file: {}", e))
        })?;
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("extension.zip")
            .to_string();
        // Build multipart body manually (reqwest multipart feature not enabled)
        let boundary = format!("----ScrapflyBoundary{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis());
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n\
                 Content-Type: application/octet-stream\r\n\r\n",
                file_name
            )
            .as_bytes(),
        );
        body.extend_from_slice(&file_bytes);
        body.extend_from_slice(format!("\r\n--{}--\r\n", boundary).as_bytes());
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(&format!("multipart/form-data; boundary={}", boundary))
                .map_err(|e| ScrapflyError::Config(format!("invalid content-type: {}", e)))?,
        );
        let resp = self
            .send_with_retry(Method::POST, url, Some(headers), Some(body))
            .await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 && status != 201 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Delete a browser extension by ID.
    pub async fn cloud_browser_extension_delete(
        &self,
        extension_id: &str,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/extension/{}?key={}",
            rest_base(self.cloud_browser_host()),
            extension_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid extension url: {}", e)))?;
        let resp = self
            .send_with_retry(Method::DELETE, url, None, None)
            .await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Get debug recording playback metadata for a run.
    pub async fn cloud_browser_playback(
        &self,
        run_id: &str,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/run/{}/playback?key={}",
            rest_base(self.cloud_browser_host()),
            run_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid playback url: {}", e)))?;
        let resp = self.send_with_retry(Method::GET, url, None, None).await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Terminate a Cloud Browser session.
    pub async fn cloud_browser_session_stop(&self, session_id: &str) -> Result<(), ScrapflyError> {
        if session_id.is_empty() {
            return Err(ScrapflyError::Config("session_id is required".into()));
        }
        let url = format!(
            "{}/session/{}/stop?key={}",
            rest_base(self.cloud_browser_host()),
            session_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid session url: {}", e)))?;
        let resp = self.send_with_retry(Method::POST, url, None, None).await?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
            return Err(from_response(status, &body, 0, false));
        }
        Ok(())
    }

    /// List all running Cloud Browser sessions.
    pub async fn cloud_browser_sessions(&self) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/sessions?key={}",
            rest_base(self.cloud_browser_host()),
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid sessions url: {}", e)))?;
        let resp = self.send_with_retry(Method::GET, url, None, None).await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Download a debug session recording video (raw bytes).
    pub async fn cloud_browser_video(&self, run_id: &str) -> Result<Vec<u8>, ScrapflyError> {
        let url = format!(
            "{}/run/{}/video?key={}",
            rest_base(self.cloud_browser_host()),
            run_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid video url: {}", e)))?;
        let resp = self.send_with_retry(Method::GET, url, None, None).await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(body.to_vec())
    }
}
