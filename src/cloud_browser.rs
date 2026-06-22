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
    /// Browser UI language — the singular `navigator.language` base tag
    /// (e.g. "en"). `None` lets the server derive it from `country`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Ordered language preference list driving `navigator.languages` and the
    /// q-weighted `Accept-Language` header (e.g. `["fr-FR", "fr", "en-US"]`).
    /// Sent comma-joined on the wire; capped server-side at 3 entries.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
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
    /// Enable MCP (Model Context Protocol) support.
    #[serde(skip_serializing_if = "is_false")]
    pub enable_mcp: bool,
    /// Arm Scrapium's built-in captcha detector + solver on the first page attach.
    /// Turnstile, DataDome slider, reCAPTCHA, GeeTest, PerimeterX hold, and
    /// puzzle captchas are handled automatically. Billed per solve; failures
    /// cost nothing. See <https://scrapfly.io/docs/cloud-browser-api/captcha-solver>.
    #[serde(skip_serializing_if = "is_false")]
    pub solve_captcha: bool,
    /// Cloud Browser credential vault NAME to attach to the session — the
    /// alphanumeric name given at create time. The server resolves the name
    /// to the vault scoped to the api-key's project and environment, decrypts
    /// its items with the caller-supplied [`BrowserConfig::vault_key`], and
    /// pushes them via CDP before the customer takes over. Pair with
    /// `vault_key`. See <https://scrapfly.io/docs/cloud-browser-api/vault>.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<String>,
    /// Customer-held base64-encoded 32-byte vault key. Forwarded transiently
    /// on the WebSocket query string and zeroed by the server after items
    /// are decrypted. The SDK never logs or echoes this value — see the
    /// E2EE boundary documentation in the project's HIPAA evidence pack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault_key: Option<String>,

    /// Enable the human-in-the-loop VNC channel.
    #[serde(skip_serializing_if = "is_false")]
    pub enable_vnc: bool,
    /// Customer-chosen VNC password. Required when `enable_vnc` is true and
    /// `hitl_allowed_networks` is empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vnc_password: Option<String>,
    /// Enable the human-in-the-loop WebRTC channel.
    #[serde(skip_serializing_if = "is_false")]
    pub enable_rtc: bool,
    /// Optional WebRTC username (defaults to "scrapfly" server-side).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtc_username: Option<String>,
    /// Customer-chosen WebRTC password. Required when `enable_rtc` is true
    /// and `hitl_allowed_networks` is empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtc_password: Option<String>,
    /// Source IPs / CIDRs trusted to attach to HITL channels (VNC + WebRTC +
    /// downloads) without credentials. Sent on the wire as a comma-separated
    /// string.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hitl_allowed_networks: Vec<String>,
}

/// Return the deterministic project salt for an api key (`sha256(api_key)[:8]`).
/// Matches the `X-Browser-Project-Salt` response header returned on a
/// successful Cloud Browser WebSocket upgrade.
pub fn project_salt(api_key: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(api_key.as_bytes());
    hex::encode(&digest[..4])
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
    /// Fingerprint OS: `linux`, `windows`, `macos`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    /// Fingerprint browser brand: `chrome`, `edge`, `brave`, `opera`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_brand: Option<String>,
    /// Named session for reconnection — reuses an existing ASP session and
    /// disables auto-close on disconnect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Navigation timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
    /// Browser session timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_timeout: Option<u32>,
    /// Enable MCP support in the browser.
    #[serde(skip_serializing_if = "is_false")]
    pub enable_mcp: bool,
    /// Record the session for replay via [`Client::cloud_browser_playback`] /
    /// [`Client::cloud_browser_video`].
    #[serde(skip_serializing_if = "is_false")]
    pub debug: bool,
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
    /// MCP endpoint (only when enable_mcp was set).
    #[serde(default)]
    pub mcp_endpoint: String,
}

impl Client {
    /// Build the WebSocket URL for a new Cloud Browser session.
    ///
    /// On rejection the server sends a JSON error frame then a close frame
    /// with code 1008/1011/1013 and a "ERR::BROWSER::CODE: reason" string.
    /// See <https://scrapfly.io/docs/cloud-browser-api/errors#websocket-close-frame>
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
        if let Some(v) = &config.lang {
            pairs.push(("lang".into(), v.clone()));
        }
        if !config.languages.is_empty() {
            pairs.push(("languages".into(), config.languages.join(",")));
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
        if config.enable_mcp {
            pairs.push(("enable_mcp".into(), "true".into()));
        }
        if config.solve_captcha {
            pairs.push(("solve_captcha".into(), "true".into()));
        }
        if let Some(v) = &config.vault {
            pairs.push(("vault".into(), v.clone()));
        }
        if let Some(v) = &config.vault_key {
            pairs.push(("vault_key".into(), v.clone()));
        }
        if config.enable_vnc {
            pairs.push(("enable_vnc".into(), "true".into()));
        }
        if let Some(v) = &config.vnc_password {
            pairs.push(("vnc_password".into(), v.clone()));
        }
        if config.enable_rtc {
            pairs.push(("enable_rtc".into(), "true".into()));
        }
        if let Some(v) = &config.rtc_username {
            pairs.push(("rtc_username".into(), v.clone()));
        }
        if let Some(v) = &config.rtc_password {
            pairs.push(("rtc_password".into(), v.clone()));
        }
        if !config.hitl_allowed_networks.is_empty() {
            pairs.push((
                "hitl_allowed_networks".into(),
                config.hitl_allowed_networks.join(","),
            ));
        }
        let qs = serde_urlencoded::to_string(&pairs).unwrap_or_default();
        format!("{}?{}", ws_host, qs)
    }

    /// Return the deterministic project salt for this client's api key.
    /// Matches the `X-Browser-Project-Salt` response header.
    pub fn cloud_browser_project_salt(&self) -> String {
        project_salt(self.api_key())
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
    pub async fn cloud_browser_extension_list(&self) -> Result<serde_json::Value, ScrapflyError> {
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
        let file_bytes = std::fs::read(file_path)
            .map_err(|e| ScrapflyError::Config(format!("failed to read extension file: {}", e)))?;
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("extension.zip")
            .to_string();
        // Build multipart body manually (reqwest multipart feature not enabled)
        let boundary = format!(
            "----ScrapflyBoundary{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
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

    // ------------------------------------------------------------------
    // Cloud Browser credential vault — E2EE customer-held keys.
    //
    // The vault key returned by `cloud_browser_vault_create` /
    // `cloud_browser_vault_rotate` is the ONLY copy. The SDK forwards it
    // transiently in the `X-Vault-Key` header on the few endpoints that
    // need it (item create/update with secret rotation, vault rotate)
    // and never logs, prints, formats, or otherwise persists it. Loud
    // rule documented at:
    //   /root/.claude/projects/-root-scrapfly-apps/memory/agent_secret_tokenization_boundary.md
    // ------------------------------------------------------------------

    /// Create a new credential vault. The server returns a freshly
    /// generated vault key under the `key` field of the response — this
    /// is the only time the key is shown. Save it locally; the server
    /// cannot recover it.
    pub async fn cloud_browser_vault_create(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/vault?key={}",
            rest_base(self.cloud_browser_host()),
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid vault url: {}", e)))?;
        let body = serde_json::json!({
            "name": name,
            "description": description.unwrap_or(""),
        });
        let body_bytes = serde_json::to_vec(&body)?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let resp = self
            .send_with_retry(Method::POST, url, Some(headers), Some(body_bytes))
            .await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 && status != 201 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// List all credential vaults for the account.
    pub async fn cloud_browser_vault_list(&self) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/vault?key={}",
            rest_base(self.cloud_browser_host()),
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid vault url: {}", e)))?;
        let resp = self.send_with_retry(Method::GET, url, None, None).await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Get vault metadata. Does NOT include any secret material.
    pub async fn cloud_browser_vault_get(
        &self,
        vault_id: &str,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/vault/{}?key={}",
            rest_base(self.cloud_browser_host()),
            vault_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid vault url: {}", e)))?;
        let resp = self.send_with_retry(Method::GET, url, None, None).await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Rename a vault (and/or update its description). Only the
    /// non-`None` fields are sent.
    pub async fn cloud_browser_vault_update(
        &self,
        vault_id: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/vault/{}?key={}",
            rest_base(self.cloud_browser_host()),
            vault_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid vault url: {}", e)))?;
        let mut body_map = serde_json::Map::new();
        if let Some(v) = name {
            body_map.insert("name".into(), serde_json::Value::String(v.into()));
        }
        if let Some(v) = description {
            body_map.insert("description".into(), serde_json::Value::String(v.into()));
        }
        let body_bytes = serde_json::to_vec(&serde_json::Value::Object(body_map))?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let resp = self
            .send_with_retry(Method::PATCH, url, Some(headers), Some(body_bytes))
            .await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Delete a vault by id (cascades to all items).
    pub async fn cloud_browser_vault_delete(
        &self,
        vault_id: &str,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/vault/{}?key={}",
            rest_base(self.cloud_browser_host()),
            vault_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid vault url: {}", e)))?;
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

    /// Rotate a vault's key. Requires the CURRENT key in the
    /// `X-Vault-Key` header; the server returns a fresh key in the
    /// response body. After this call the old key is permanently
    /// invalid for this vault. The current key is forwarded to the
    /// server transiently and never logged by the SDK.
    pub async fn cloud_browser_vault_rotate(
        &self,
        vault_id: &str,
        current_vault_key: &str,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/vault/{}/rotate?key={}",
            rest_base(self.cloud_browser_host()),
            vault_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid vault url: {}", e)))?;
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Vault-Key",
            HeaderValue::from_str(current_vault_key)
                // Generic message — never echo the key value back.
                .map_err(|_| ScrapflyError::Config("X-Vault-Key contained invalid bytes".into()))?,
        );
        let resp = self
            .send_with_retry(Method::POST, url, Some(headers), None)
            .await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// List items in a vault. Items include the encrypted `secret_blob`
    /// but not plaintext — the blob is meaningless without the
    /// customer-held key.
    pub async fn cloud_browser_vault_item_list(
        &self,
        vault_id: &str,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/vault/{}/item?key={}",
            rest_base(self.cloud_browser_host()),
            vault_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid vault url: {}", e)))?;
        let resp = self.send_with_retry(Method::GET, url, None, None).await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Create a new item in a vault. The vault key is required to
    /// envelope-encrypt the per-item DEK; it is forwarded transiently
    /// in the `X-Vault-Key` header. Caller-supplied `item` JSON shape
    /// follows the documented contract (see itemCreateRequest in
    /// pkg/vault/controller.go) — typically:
    ///
    /// ```json
    /// {
    ///   "type": "password",
    ///   "label": "...",
    ///   "origin": "https://example.com",
    ///   "username": "user",
    ///   "secret": {"password": "hunter2"}
    /// }
    /// ```
    pub async fn cloud_browser_vault_item_create(
        &self,
        vault_id: &str,
        vault_key: &str,
        item: serde_json::Value,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/vault/{}/item?key={}",
            rest_base(self.cloud_browser_host()),
            vault_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid vault url: {}", e)))?;
        let body_bytes = serde_json::to_vec(&item)?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "X-Vault-Key",
            HeaderValue::from_str(vault_key)
                .map_err(|_| ScrapflyError::Config("X-Vault-Key contained invalid bytes".into()))?,
        );
        let resp = self
            .send_with_retry(Method::POST, url, Some(headers), Some(body_bytes))
            .await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 && status != 201 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Patch an existing item. `vault_key` is REQUIRED iff the patch
    /// rotates the secret payload (i.e. `secret` is present in
    /// `patch`); for pure metadata edits (label, origin, username,
    /// metadata) the server accepts the request without an
    /// `X-Vault-Key` header. The SDK forwards the key only when
    /// supplied — it does not auto-detect the patch shape.
    pub async fn cloud_browser_vault_item_update(
        &self,
        vault_id: &str,
        item_id: &str,
        vault_key: Option<&str>,
        patch: serde_json::Value,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/vault/{}/item/{}?key={}",
            rest_base(self.cloud_browser_host()),
            vault_id,
            item_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid vault url: {}", e)))?;
        let body_bytes = serde_json::to_vec(&patch)?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(k) = vault_key {
            headers.insert(
                "X-Vault-Key",
                HeaderValue::from_str(k).map_err(|_| {
                    ScrapflyError::Config("X-Vault-Key contained invalid bytes".into())
                })?,
            );
        }
        let resp = self
            .send_with_retry(Method::PATCH, url, Some(headers), Some(body_bytes))
            .await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(ScrapflyError::Transport)?;
        if status != 200 {
            return Err(from_response(status, &body, 0, false));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Delete a single vault item.
    pub async fn cloud_browser_vault_item_delete(
        &self,
        vault_id: &str,
        item_id: &str,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let url = format!(
            "{}/vault/{}/item/{}?key={}",
            rest_base(self.cloud_browser_host()),
            vault_id,
            item_id,
            self.api_key()
        );
        let url = Url::parse(&url)
            .map_err(|e| ScrapflyError::Config(format!("invalid vault url: {}", e)))?;
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

    /// Get debug recording playback metadata for a run. The response carries
    /// `available`, `status` (one of `ready`, `uploading`, `unavailable`,
    /// `disabled`), `metadata`, `video_url`, and `retry_after_ms`.
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

    /// Poll the playback endpoint until the recording resolves to a terminal
    /// state (`ready` or `unavailable`) or the timeout elapses. Honours the
    /// server's `retry_after_ms` hint when present.
    pub async fn cloud_browser_wait_for_playback(
        &self,
        run_id: &str,
        timeout: std::time::Duration,
        fallback_interval: std::time::Duration,
    ) -> Result<serde_json::Value, ScrapflyError> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let playback = self.cloud_browser_playback(run_id).await?;
            let status = playback
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if status != "uploading" {
                return Ok(playback);
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Ok(playback);
            }
            let retry_after_ms = playback
                .get("retry_after_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let next = if retry_after_ms > 0 {
                std::time::Duration::from_millis(retry_after_ms)
            } else {
                fallback_interval
            };
            tokio::time::sleep(next.min(remaining)).await;
        }
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
