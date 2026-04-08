//! Screenshot result — port of `sdk/go/result_screenshot.go`.

use std::path::{Path, PathBuf};

use bytes::Bytes;

/// Result of a screenshot API call.
#[derive(Debug, Clone)]
pub struct ScreenshotResult {
    /// Raw image bytes.
    pub image: Bytes,
    /// Image metadata.
    pub metadata: ScreenshotMetadata,
}

/// Screenshot metadata parsed from the response headers.
#[derive(Debug, Clone, Default)]
pub struct ScreenshotMetadata {
    /// File extension inferred from Content-Type (`png`, `jpeg`, …).
    pub extension_name: String,
    /// Upstream HTTP status code, from the `x-scrapfly-upstream-http-code` header.
    pub upstream_status_code: u16,
    /// Final upstream URL, from the `x-scrapfly-upstream-url` header.
    pub upstream_url: String,
}

impl ScreenshotResult {
    /// Save the image to disk at `{dir}/{name}.{extension}`. Creates `dir`
    /// (and parents) best-effort. Returns the written path.
    pub fn save(&self, name: &str, dir: Option<&Path>) -> std::io::Result<PathBuf> {
        let dir = dir.unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir)?;
        let ext = if self.metadata.extension_name.is_empty() {
            "bin"
        } else {
            &self.metadata.extension_name
        };
        let path = dir.join(format!("{}.{}", name, ext));
        std::fs::write(&path, &self.image)?;
        Ok(path)
    }
}
