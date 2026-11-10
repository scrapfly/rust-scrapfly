//! Retrieve the files a Cloud Browser session downloaded.
//!
//! Connect with [`BrowserConfig::downloads_stream`](crate::BrowserConfig): `ScrapiumBrowser.getDownloads`
//! then returns each small file inline and each large one as an IO stream handle, which
//! [`CloudBrowserDownloads`] reads in chunks with `IO.read`, so no CDP message is larger than one
//! chunk. Without the option every file comes back inline, and the helpers read that shape too.
//!
//! A session is any [`CdpSender`]: the chromiumoxide adapter [`ChromiumoxideCdp`] (feature
//! `chromiumoxide`), or a closure `Fn(String, Value) -> Future<Output = Result<Value, CdpError>>`
//! over a CDP client of your own.
//!
//! ```ignore
//! let downloads = CloudBrowserDownloads::new(ChromiumoxideCdp::new(page));
//! downloads.wait_for("report.pdf", Duration::from_secs(60)).await?;
//! let paths = downloads.save_all("downloads").await?;
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use base64::Engine;
use futures_util::future::BoxFuture;
use serde::Deserialize;
use serde_json::{json, Value};

/// Base64 characters per `IO.read`: an 8 MiB message carrying 6 MiB of the file.
pub const DEFAULT_CHUNK_SIZE: usize = 8 * 1024 * 1024;
/// The largest chunk the Cloud Browser serves per `IO.read`.
pub const MAX_CHUNK_SIZE: usize = 32 * 1024 * 1024;

const WAIT_INTERVAL: Duration = Duration::from_millis(500);

/// The error of one CDP command, as the client reports it.
pub type CdpError = Box<dyn std::error::Error + Send + Sync>;

/// Sends one CDP command on a Cloud Browser session and resolves to its result.
pub trait CdpSender: Send + Sync {
    /// Send `method` with `params` and resolve to the command's result.
    fn send_cdp<'a>(
        &'a self,
        method: &'a str,
        params: Value,
    ) -> BoxFuture<'a, Result<Value, CdpError>>;
}

impl<F, Fut> CdpSender for F
where
    F: Fn(String, Value) -> Fut + Send + Sync,
    Fut: Future<Output = Result<Value, CdpError>> + Send + 'static,
{
    fn send_cdp<'a>(
        &'a self,
        method: &'a str,
        params: Value,
    ) -> BoxFuture<'a, Result<Value, CdpError>> {
        Box::pin((self)(method.to_string(), params))
    }
}

/// Sends through a chromiumoxide [`Page`](chromiumoxide::Page), on that page's session.
///
/// chromiumoxide's `tokio-runtime` feature carries no TLS: to reach the `wss://` Cloud Browser,
/// also depend on `async-tungstenite` with one of its tokio TLS features.
#[cfg(feature = "chromiumoxide")]
pub struct ChromiumoxideCdp {
    page: chromiumoxide::Page,
}

#[cfg(feature = "chromiumoxide")]
impl ChromiumoxideCdp {
    /// Read downloads through `page`.
    pub fn new(page: chromiumoxide::Page) -> Self {
        Self { page }
    }
}

/// A CDP command named at run time, for chromiumoxide's typed `execute`.
#[cfg(feature = "chromiumoxide")]
#[derive(Debug, serde::Serialize)]
#[serde(transparent)]
struct RawCommand {
    #[serde(skip)]
    method: String,
    params: Value,
}

#[cfg(feature = "chromiumoxide")]
impl chromiumoxide::Method for RawCommand {
    fn identifier(&self) -> chromiumoxide::types::MethodId {
        self.method.clone().into()
    }
}

#[cfg(feature = "chromiumoxide")]
impl chromiumoxide::Command for RawCommand {
    type Response = Value;
}

#[cfg(feature = "chromiumoxide")]
impl CdpSender for ChromiumoxideCdp {
    fn send_cdp<'a>(
        &'a self,
        method: &'a str,
        params: Value,
    ) -> BoxFuture<'a, Result<Value, CdpError>> {
        Box::pin(async move {
            let command = RawCommand {
                method: method.to_string(),
                params,
            };
            let response = self.page.execute(command).await?;
            Ok(response.result)
        })
    }
}

/// Why a download could not be listed or read whole.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    /// The CDP client or the browser refused a command.
    #[error("{method}: {source}")]
    Cdp {
        /// The command that failed.
        method: String,
        /// The client's error.
        source: CdpError,
    },
    /// A result did not have the expected shape.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Content that is not valid base64.
    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),
    /// Writing the file failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// The stream ended with another size than the download's.
    #[error("{name}: read {read} bytes, expected {expected}")]
    Incomplete {
        /// The download's name.
        name: String,
        /// Bytes read.
        read: u64,
        /// The download's size.
        expected: u64,
    },
    /// The file was not listed with a settled size in time.
    #[error("{0} did not finish downloading in time")]
    Timeout(String),
    /// A chunk size outside 4 to [`MAX_CHUNK_SIZE`].
    #[error("chunk size must be between 4 and {MAX_CHUNK_SIZE} base64 characters, got {0}")]
    ChunkSize(usize),
}

/// One file of the session's download directory. A small file carries its base64 content in
/// `data`. A large one, with `downloads_stream`, carries the IO handle to read in `stream` instead,
/// and its size in bytes in `size`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserDownload {
    /// The file name, as Chrome wrote it.
    pub name: String,
    /// The download GUID, when the browser recorded one.
    pub guid: Option<String>,
    /// The size in bytes.
    pub size: Option<u64>,
    /// The base64 content of an inline file.
    pub data: Option<String>,
    /// The IO handle of a streamed file.
    pub stream: Option<String>,
}

impl BrowserDownload {
    /// Whether the download is read through an IO stream handle.
    pub fn streamed(&self) -> bool {
        self.stream.is_some()
    }
}

#[derive(Deserialize)]
struct StreamEntry {
    stream: String,
    size: Option<u64>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct GetDownloadsResult {
    #[serde(default)]
    files: HashMap<String, String>,
    #[serde(default)]
    files_by_guid: HashMap<String, String>,
    guid_names: Option<HashMap<String, String>>,
    streams: Option<HashMap<String, StreamEntry>>,
}

/// The downloads of a `ScrapiumBrowser.getDownloads` result, in either of its shapes, sorted by name.
pub fn parse_downloads(result: Value) -> Result<Vec<BrowserDownload>, DownloadError> {
    let result: GetDownloadsResult = serde_json::from_value(result)?;
    let mut downloads = Vec::new();

    if result.guid_names.is_some() || result.streams.is_some() {
        let guid_names = result.guid_names.unwrap_or_default();
        for (guid, data) in result.files_by_guid {
            let name = guid_names
                .get(&guid)
                .cloned()
                .unwrap_or_else(|| guid.clone());
            downloads.push(BrowserDownload {
                name,
                size: Some(decoded_size(&data)),
                guid: Some(guid),
                data: Some(data),
                stream: None,
            });
        }
        for (guid, entry) in result.streams.unwrap_or_default() {
            let name = entry
                .name
                .or_else(|| guid_names.get(&guid).cloned())
                .unwrap_or_else(|| guid.clone());
            downloads.push(BrowserDownload {
                name,
                guid: Some(guid),
                size: entry.size,
                data: None,
                stream: Some(entry.stream),
            });
        }
        // A file on disk with no download GUID, such as one finished before this session attached.
        for (name, data) in result.files {
            downloads.push(BrowserDownload {
                name,
                guid: None,
                size: Some(decoded_size(&data)),
                data: Some(data),
                stream: None,
            });
        }
    } else {
        // Without downloads_stream every file is inline under `files`, and again under
        // `files_by_guid` with no name: `files` alone holds each file once.
        for (name, data) in result.files {
            downloads.push(BrowserDownload {
                name,
                guid: None,
                size: Some(decoded_size(&data)),
                data: Some(data),
                stream: None,
            });
        }
    }

    downloads.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(downloads)
}

fn decoded_size(data: &str) -> u64 {
    let padding = if data.ends_with("==") {
        2
    } else if data.ends_with('=') {
        1
    } else {
        0
    };
    (data.len() / 4 * 3 - padding) as u64
}

fn target_path(download: &BrowserDownload, path: Option<&Path>) -> PathBuf {
    // The name comes from the remote browser: keep its last component only.
    let normalized = download.name.replace('\\', "/");
    let mut name = normalized.rsplit('/').next().unwrap_or("").to_string();
    if name.is_empty() || name == "." || name == ".." {
        name = download
            .guid
            .clone()
            .unwrap_or_else(|| "download".to_string());
    }
    match path {
        None => PathBuf::from(name),
        Some(path) if path.is_dir() => path.join(name),
        Some(path) => path.to_path_buf(),
    }
}

#[derive(Deserialize)]
struct ReadReply {
    #[serde(default)]
    data: String,
    #[serde(rename = "base64Encoded")]
    base64_encoded: Option<bool>,
    #[serde(default)]
    eof: bool,
}

/// Reads the downloads of a Cloud Browser session through a [`CdpSender`].
pub struct CloudBrowserDownloads<S: CdpSender> {
    sender: S,
    chunk_size: usize,
}

impl<S: CdpSender> CloudBrowserDownloads<S> {
    /// Read downloads through `sender`, [`DEFAULT_CHUNK_SIZE`] base64 characters per `IO.read`.
    pub fn new(sender: S) -> Self {
        Self {
            sender,
            chunk_size: DEFAULT_CHUNK_SIZE,
        }
    }

    /// Read `chunk_size` base64 characters per `IO.read`, rounded down to a multiple of 4.
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Result<Self, DownloadError> {
        if !(4..=MAX_CHUNK_SIZE).contains(&chunk_size) {
            return Err(DownloadError::ChunkSize(chunk_size));
        }
        self.chunk_size = chunk_size - chunk_size % 4;
        Ok(self)
    }

    async fn send(&self, method: &str, params: Value) -> Result<Value, DownloadError> {
        self.sender
            .send_cdp(method, params)
            .await
            .map_err(|source| DownloadError::Cdp {
                method: method.to_string(),
                source,
            })
    }

    /// The size in bytes of every completed download, by filename.
    pub async fn metadata(&self) -> Result<HashMap<String, u64>, DownloadError> {
        #[derive(Deserialize)]
        struct Metadata {
            #[serde(default)]
            metadata: HashMap<String, u64>,
        }
        let reply: Metadata = serde_json::from_value(
            self.send("ScrapiumBrowser.getDownloadsMetadatas", json!({}))
                .await?,
        )?;
        Ok(reply.metadata)
    }

    /// Wait until `filename` is listed with the same size twice in a row, and return that size.
    pub async fn wait_for(&self, filename: &str, timeout: Duration) -> Result<u64, DownloadError> {
        self.wait_for_every(filename, timeout, WAIT_INTERVAL).await
    }

    /// [`wait_for`](Self::wait_for), checking every `interval`.
    pub async fn wait_for_every(
        &self,
        filename: &str,
        timeout: Duration,
        interval: Duration,
    ) -> Result<u64, DownloadError> {
        let deadline = Instant::now() + timeout;
        let mut last = None;
        while Instant::now() < deadline {
            let size = self.metadata().await?.get(filename).copied();
            if size.is_some() && size == last {
                return Ok(size.unwrap_or_default());
            }
            last = size;
            tokio::time::sleep(interval).await;
        }
        Err(DownloadError::Timeout(filename.to_string()))
    }

    /// Every download of the session: small ones with their content, large ones as handles.
    pub async fn list(&self) -> Result<Vec<BrowserDownload>, DownloadError> {
        parse_downloads(self.send("ScrapiumBrowser.getDownloads", json!({})).await?)
    }

    /// Write the bytes of a download to `writer` one decoded chunk at a time, and return how many
    /// were written. A stream handle is read once and always closed.
    pub async fn write_to<W: Write>(
        &self,
        download: &BrowserDownload,
        writer: &mut W,
    ) -> Result<u64, DownloadError> {
        let handle = match &download.stream {
            None => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(download.data.as_deref().unwrap_or(""))?;
                writer.write_all(&bytes)?;
                return Ok(bytes.len() as u64);
            }
            Some(handle) => handle,
        };

        let read = self.drain(handle, writer).await;
        let closed = self.send("IO.close", json!({ "handle": handle })).await;
        let written = read?;
        closed?;

        if let Some(expected) = download.size {
            if written != expected {
                return Err(DownloadError::Incomplete {
                    name: download.name.clone(),
                    read: written,
                    expected,
                });
            }
        }
        Ok(written)
    }

    async fn drain<W: Write>(&self, handle: &str, writer: &mut W) -> Result<u64, DownloadError> {
        let mut written = 0u64;
        loop {
            let reply: ReadReply = serde_json::from_value(
                self.send(
                    "IO.read",
                    json!({ "handle": handle, "size": self.chunk_size }),
                )
                .await?,
            )?;
            if !reply.data.is_empty() {
                let bytes = if reply.base64_encoded.unwrap_or(true) {
                    base64::engine::general_purpose::STANDARD.decode(&reply.data)?
                } else {
                    reply.data.into_bytes()
                };
                writer.write_all(&bytes)?;
                written += bytes.len() as u64;
            }
            if reply.eof {
                return Ok(written);
            }
        }
    }

    /// The whole content of a download, in memory.
    pub async fn read(&self, download: &BrowserDownload) -> Result<Vec<u8>, DownloadError> {
        let mut bytes = Vec::new();
        self.write_to(download, &mut bytes).await?;
        Ok(bytes)
    }

    /// Write a download to `path`, a file or a directory (the current one when `None`), chunk by
    /// chunk, and return the path written.
    pub async fn save(
        &self,
        download: &BrowserDownload,
        path: Option<&Path>,
    ) -> Result<PathBuf, DownloadError> {
        let target = target_path(download, path);
        let mut file = std::fs::File::create(&target)?;
        self.write_to(download, &mut file).await?;
        file.flush()?;
        Ok(target)
    }

    /// Write every download of the session into `dir` and return their paths.
    pub async fn save_all(&self, dir: impl AsRef<Path>) -> Result<Vec<PathBuf>, DownloadError> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)?;
        let mut paths = Vec::new();
        for download in self.list().await? {
            paths.push(self.save(&download, Some(dir)).await?);
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const SMALL: &[u8] = b"%PDF-1.7 small inline file";

    fn large() -> Vec<u8> {
        // Not a multiple of 3 bytes, so its base64 ends with padding and the last chunk is short.
        let mut state: u32 = 0x9E37_79B9;
        (0..3 * 1024 * 1024 + 7)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state & 0xFF) as u8
            })
            .collect()
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// The Cloud Browser's side of the download commands, as a CDP session answers them.
    #[derive(Default)]
    struct FakeBrowser {
        legacy: bool,
        orphans: Vec<(String, Vec<u8>)>,
        ready_after: usize,
        metadata_polls: usize,
        large: Vec<u8>,
        streams: HashMap<String, (String, usize)>,
        reads: Vec<Value>,
        closes: Vec<String>,
    }

    impl FakeBrowser {
        fn new() -> Arc<Mutex<Self>> {
            Arc::new(Mutex::new(FakeBrowser {
                large: large(),
                ..Default::default()
            }))
        }

        fn answer(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
            match method {
                "ScrapiumBrowser.getDownloadsMetadatas" => {
                    self.metadata_polls += 1;
                    if self.metadata_polls <= self.ready_after {
                        return Ok(json!({ "metadata": {} }));
                    }
                    Ok(
                        json!({ "metadata": { "small.pdf": SMALL.len(), "large.bin": self.large.len() } }),
                    )
                }
                "ScrapiumBrowser.getDownloads" => Ok(self.get_downloads()),
                "IO.read" => self.read(params),
                "IO.close" => {
                    let handle = params["handle"].as_str().unwrap_or_default().to_string();
                    self.streams.remove(&handle);
                    self.closes.push(handle);
                    Ok(json!({}))
                }
                other => Err(format!("unexpected command {other}").into()),
            }
        }

        fn get_downloads(&mut self) -> Value {
            let large = b64(&self.large);
            if self.legacy {
                return json!({
                    "files": { "small.pdf": b64(SMALL), "large.bin": large },
                    "files_by_guid": { "GUID-SMALL": b64(SMALL), "GUID-LARGE": large },
                });
            }
            self.streams
                .insert("scrapfly-download:1".into(), (large, 0));
            let orphans: HashMap<String, String> = self
                .orphans
                .iter()
                .map(|(name, raw)| (name.clone(), b64(raw)))
                .collect();
            json!({
                "files": orphans,
                "files_by_guid": { "GUID-SMALL": b64(SMALL) },
                "guid_names": { "GUID-SMALL": "small.pdf", "GUID-LARGE": "large.bin" },
                "streams": { "GUID-LARGE": { "stream": "scrapfly-download:1", "size": self.large.len(), "name": "large.bin" } },
            })
        }

        fn read(&mut self, params: Value) -> Result<Value, CdpError> {
            self.reads.push(params.clone());
            if params.get("offset").is_some() {
                return Err(
                    "download streams are sequential: IO.read offset is not supported".into(),
                );
            }
            let handle = params["handle"].as_str().unwrap_or_default().to_string();
            let mut size = params["size"]
                .as_u64()
                .unwrap_or(4 * 1024 * 1024)
                .min(MAX_CHUNK_SIZE as u64) as usize;
            size -= size % 4;
            let Some((data, off)) = self.streams.get_mut(&handle) else {
                return Err(format!("Invalid stream handle: {handle}").into());
            };
            let end = (*off + size).min(data.len());
            let chunk = data[*off..end].to_string();
            *off = end;
            let eof = end >= data.len();
            if eof {
                self.streams.remove(&handle);
            }
            Ok(json!({ "data": chunk, "base64Encoded": true, "eof": eof }))
        }
    }

    fn sender(browser: &Arc<Mutex<FakeBrowser>>) -> impl CdpSender {
        let browser = browser.clone();
        move |method: String, params: Value| {
            let result = browser.lock().unwrap().answer(&method, params);
            async move { result }
        }
    }

    async fn streamed<S: CdpSender>(downloads: &CloudBrowserDownloads<S>) -> BrowserDownload {
        downloads
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|d| d.streamed())
            .unwrap()
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("scrapfly-downloads-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn streamed_shape_lists_small_files_inline_and_large_ones_as_handles() {
        let browser = FakeBrowser::new();
        browser
            .lock()
            .unwrap()
            .orphans
            .push(("early.txt".into(), b"before attach".to_vec()));
        let result = browser.lock().unwrap().get_downloads();
        let downloads = parse_downloads(result).unwrap();
        let names: Vec<_> = downloads.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["early.txt", "large.bin", "small.pdf"]);
        assert!(!downloads[2].streamed());
        assert_eq!(downloads[2].size, Some(SMALL.len() as u64));
        assert_eq!(downloads[2].guid.as_deref(), Some("GUID-SMALL"));
        assert_eq!(downloads[1].stream.as_deref(), Some("scrapfly-download:1"));
        assert_eq!(downloads[1].size, Some(large().len() as u64));
        assert_eq!(downloads[0].guid, None);
        assert_eq!(downloads[0].size, Some(13));
    }

    #[test]
    fn default_shape_lists_each_file_once() {
        let browser = FakeBrowser::new();
        browser.lock().unwrap().legacy = true;
        let result = browser.lock().unwrap().get_downloads();
        let downloads = parse_downloads(result).unwrap();
        let names: Vec<_> = downloads.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["large.bin", "small.pdf"]);
        assert!(downloads.iter().all(|d| !d.streamed()));
    }

    #[tokio::test]
    async fn save_all_writes_every_file_and_reads_a_handle_in_chunks() {
        let dir = temp_dir("save-all");
        let browser = FakeBrowser::new();
        browser
            .lock()
            .unwrap()
            .orphans
            .push(("early.txt".into(), b"before attach".to_vec()));
        let downloads = CloudBrowserDownloads::new(sender(&browser))
            .with_chunk_size(1024 * 1024)
            .unwrap();

        let paths = downloads.save_all(&dir).await.unwrap();

        assert_eq!(paths.len(), 3);
        assert_eq!(std::fs::read(dir.join("small.pdf")).unwrap(), SMALL);
        assert_eq!(
            std::fs::read(dir.join("early.txt")).unwrap(),
            b"before attach"
        );
        assert_eq!(std::fs::read(dir.join("large.bin")).unwrap(), large());
        let state = browser.lock().unwrap();
        let encoded = b64(&state.large).len();
        assert_eq!(state.reads.len(), encoded.div_ceil(1024 * 1024));
        assert!(state
            .reads
            .iter()
            .all(|r| r["handle"] == "scrapfly-download:1" && r["size"] == 1024 * 1024));
        assert_eq!(state.closes, ["scrapfly-download:1"]);
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_chunk_size_off_the_base64_grid_is_rounded_down() {
        let browser = FakeBrowser::new();
        let downloads = CloudBrowserDownloads::new(sender(&browser))
            .with_chunk_size(1_000_003)
            .unwrap();
        let large_download = streamed(&downloads).await;
        assert_eq!(downloads.read(&large_download).await.unwrap(), large());
        assert!(browser
            .lock()
            .unwrap()
            .reads
            .iter()
            .all(|r| r["size"] == 1_000_000));
    }

    #[test]
    fn a_chunk_size_outside_the_served_range_is_refused() {
        let browser = FakeBrowser::new();
        assert!(CloudBrowserDownloads::new(sender(&browser))
            .with_chunk_size(MAX_CHUNK_SIZE + 4)
            .is_err());
        assert!(CloudBrowserDownloads::new(sender(&browser))
            .with_chunk_size(2)
            .is_err());
    }

    #[tokio::test]
    async fn default_shape_saves_without_any_io_read() {
        let dir = temp_dir("legacy");
        let browser = FakeBrowser::new();
        browser.lock().unwrap().legacy = true;
        CloudBrowserDownloads::new(sender(&browser))
            .save_all(&dir)
            .await
            .unwrap();
        assert_eq!(std::fs::read(dir.join("large.bin")).unwrap(), large());
        assert!(browser.lock().unwrap().reads.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_short_stream_fails_and_still_closes_the_handle() {
        let browser = FakeBrowser::new();
        let downloads = CloudBrowserDownloads::new(sender(&browser));
        let mut large_download = streamed(&downloads).await;
        large_download.size = Some(large().len() as u64 + 1);
        assert!(matches!(
            downloads.read(&large_download).await,
            Err(DownloadError::Incomplete { .. })
        ));
        assert_eq!(browser.lock().unwrap().closes, ["scrapfly-download:1"]);
    }

    #[tokio::test]
    async fn a_failed_read_still_closes_the_handle() {
        let browser = FakeBrowser::new();
        let downloads = CloudBrowserDownloads::new(sender(&browser));
        let mut large_download = streamed(&downloads).await;
        large_download.stream = Some("scrapfly-download:404".into());
        assert!(matches!(
            downloads.read(&large_download).await,
            Err(DownloadError::Cdp { .. })
        ));
        assert_eq!(browser.lock().unwrap().closes, ["scrapfly-download:404"]);
    }

    #[tokio::test]
    async fn a_remote_name_cannot_leave_the_target_directory() {
        // Two levels of directories of its own, so "../../" stays inside them.
        let root = temp_dir("escape");
        let dir = root.join("a").join("b");
        std::fs::create_dir_all(&dir).unwrap();
        let browser = FakeBrowser::new();
        browser
            .lock()
            .unwrap()
            .orphans
            .push(("../../escape.txt".into(), b"x".to_vec()));
        let paths = CloudBrowserDownloads::new(sender(&browser))
            .save_all(&dir)
            .await
            .unwrap();
        assert!(paths.contains(&dir.join("escape.txt")));
        assert!(!root.join("escape.txt").exists());
        assert!(!root.join("a").join("escape.txt").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn save_to_an_explicit_file() {
        let dir = temp_dir("explicit");
        let browser = FakeBrowser::new();
        let downloads = CloudBrowserDownloads::new(sender(&browser));
        let small = downloads
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|d| d.name == "small.pdf")
            .unwrap();
        let target = dir.join("renamed.pdf");
        assert_eq!(downloads.save(&small, Some(&target)).await.unwrap(), target);
        assert_eq!(std::fs::read(&target).unwrap(), SMALL);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn wait_for_returns_once_the_size_settles() {
        let browser = FakeBrowser::new();
        browser.lock().unwrap().ready_after = 2;
        let size = CloudBrowserDownloads::new(sender(&browser))
            .wait_for_every(
                "large.bin",
                Duration::from_secs(5),
                Duration::from_millis(1),
            )
            .await
            .unwrap();
        assert_eq!(size, large().len() as u64);
        assert_eq!(browser.lock().unwrap().metadata_polls, 4);
    }

    #[tokio::test]
    async fn wait_for_times_out_on_a_file_never_listed() {
        let browser = FakeBrowser::new();
        let result = CloudBrowserDownloads::new(sender(&browser))
            .wait_for_every(
                "missing.bin",
                Duration::from_millis(50),
                Duration::from_millis(10),
            )
            .await;
        assert!(matches!(result, Err(DownloadError::Timeout(_))));
    }
}
