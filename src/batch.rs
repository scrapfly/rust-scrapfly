//! Streaming multipart/mixed parser for POST /scrape/batch.
//!
//! The API emits one part per scrape as each completes. This module
//! reads the response body as a stream of `Bytes` chunks and yields
//! `(headers, body)` per part as they arrive.
//!
//! Zero new dependencies — only `reqwest`, `bytes`, and `futures-util`
//! that are already in `Cargo.toml`.

use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use futures_util::stream::Stream;

use crate::error::ScrapflyError;
use crate::result::scrape::ScrapeResult;

const CRLF: &[u8] = b"\r\n";
const DOUBLE_CRLF: &[u8] = b"\r\n\r\n";

/// One multipart part: header map (lowercased keys) plus body bytes.
#[derive(Debug)]
pub struct BatchPart {
    /// Per-part headers, lowercased. `content-type` is always set;
    /// `x-scrapfly-correlation-id` and `x-scrapfly-scrape-status`
    /// are set by the server.
    pub headers: HashMap<String, String>,

    /// Part body bytes (not decoded).
    pub body: Bytes,
}

/// A proxified batch part surfaced as a native Response-like value.
/// The part body is the raw upstream response (HTML, JSON, binary,
/// etc.) — not a JSON envelope. `reqwest::Response` is tied to a
/// live connection so we cannot re-synthesize one from bytes; this
/// struct carries the same fields a caller needs.
#[derive(Debug)]
pub struct BatchProxifiedResponse {
    /// Upstream HTTP status code restored from X-Scrapfly-Scrape-Status.
    pub status: u16,

    /// Response headers: upstream headers (originally prefixed with
    /// X-Scrapfly-Upstream- on the wire, stripped here) PLUS
    /// Scrapfly metadata (X-Scrapfly-Log, X-Scrapfly-Content-Format,
    /// X-Scrapfly-Log-Uuid). `Content-Type` is the upstream's
    /// content-type.
    pub headers: HashMap<String, String>,

    /// Raw upstream body bytes.
    pub body: Bytes,
}

impl BatchProxifiedResponse {
    /// Decode the body as UTF-8 text (mirrors reqwest::Response::text()).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Convenience accessor for the response content-type.
    pub fn content_type(&self) -> Option<&str> {
        self.headers.get("content-type").map(String::as_str)
    }

    /// Scrapfly log UUID (X-Scrapfly-Log if present, else X-Scrapfly-Log-Uuid).
    pub fn scrapfly_log(&self) -> Option<&str> {
        self.headers
            .get("x-scrapfly-log")
            .or_else(|| self.headers.get("x-scrapfly-log-uuid"))
            .map(String::as_str)
    }
}

/// Wire format for the per-part body. JSON is the default;
/// Msgpack matches the Scrapfly API's msgpack negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BatchFormat {
    /// application/json (default).
    #[default]
    Json,
    /// application/msgpack — smaller wire payload.
    Msgpack,
}

impl BatchFormat {
    pub(crate) fn accept_header(self) -> &'static str {
        match self {
            BatchFormat::Json => "application/json",
            BatchFormat::Msgpack => "application/msgpack",
        }
    }
}

/// Options for `Client::scrape_batch_with_options`.
#[derive(Debug, Clone, Default)]
pub struct BatchOptions {
    /// Wire format for per-part bodies. Defaults to JSON.
    pub format: BatchFormat,
}

/// Per-part outcome yielded by `Client::scrape_batch`.
#[derive(Debug)]
pub enum BatchOutcome {
    /// Standard per-part scrape result (JSON envelope decoded).
    Scrape(ScrapeResult),

    /// Proxified part: the upstream's raw response, with status +
    /// headers + body restored from the multipart part metadata.
    /// Surfaces when the originating `ScrapeConfig.proxified_response
    /// == true`. Matches the single-scrape `scrape_proxified()`
    /// return shape as closely as we can without a live connection.
    Proxified(BatchProxifiedResponse),

    /// Per-part error (decode failure, per-scrape upstream error, etc.).
    Err(ScrapflyError),
}

fn find_subslice(buf: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    if buf.len() < needle.len() {
        return None;
    }

    for i in 0..=buf.len() - needle.len() {
        if &buf[i..i + needle.len()] == needle {
            return Some(i);
        }
    }

    None
}

fn parse_content_type(value: &str) -> (String, HashMap<String, String>) {
    if let Some(idx) = value.find(';') {
        let mime = value[..idx].trim().to_lowercase();
        let mut params = HashMap::new();

        for piece in value[idx + 1..].split(';') {
            if let Some(eq) = piece.find('=') {
                let k = piece[..eq].trim().to_lowercase();
                let mut v = piece[eq + 1..].trim().to_string();

                if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
                    v = v[1..v.len() - 1].to_string();
                }

                params.insert(k, v);
            }
        }

        (mime, params)
    } else {
        (value.trim().to_lowercase(), HashMap::new())
    }
}

/// Stream adapter: wraps a reqwest bytes stream and yields one
/// `BatchPart` per multipart section as the body arrives.
pub struct BatchPartStream<S> {
    inner: S,
    boundary_line: Vec<u8>,
    boundary_sep: Vec<u8>,
    buf: BytesMut,
    state: State,
    done: bool,
}

enum State {
    /// Haven't found the first --boundary yet; discard anything before it.
    FindFirstBoundary,
    /// Just consumed a --boundary; next is either CRLF or "--" (terminator).
    BoundarySuffix,
    /// Reading part headers up to CRLF CRLF.
    Headers,
    /// Reading part body either by Content-Length or up to next boundary.
    Body {
        headers: HashMap<String, String>,
        content_length: Option<usize>,
    },
    /// Body already yielded; scan for the trailing "\r\n--<boundary>"
    /// and discard it before transitioning to BoundarySuffix. This
    /// lets Content-Length framing yield a part the instant its body
    /// bytes arrive, without waiting for the next part's boundary.
    ConsumeSeparator,
    /// Stream is done; no more parts.
    Done,
}

impl<S> BatchPartStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    /// Construct a part stream wrapping `stream` with the given
    /// `boundary` (without the leading `--`).
    pub fn new(stream: S, boundary: &str) -> Self {
        let boundary_line = format!("--{}", boundary).into_bytes();
        let boundary_sep = format!("\r\n--{}", boundary).into_bytes();

        Self {
            inner: stream,
            boundary_line,
            boundary_sep,
            buf: BytesMut::new(),
            state: State::FindFirstBoundary,
            done: false,
        }
    }
}

impl<S> Stream for BatchPartStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<BatchPart, ScrapflyError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // Run the state machine on the current buffer.
            let this = &mut *self;

            match &mut this.state {
                State::Done => return Poll::Ready(None),

                State::FindFirstBoundary => {
                    if let Some(idx) = find_subslice(&this.buf, &this.boundary_line) {
                        let _ = this.buf.split_to(idx + this.boundary_line.len());
                        this.state = State::BoundarySuffix;
                        continue;
                    }
                }

                State::BoundarySuffix => {
                    if this.buf.len() < 2 {
                        // need more
                    } else {
                        let head = &this.buf[..2];

                        if head == b"--" {
                            this.state = State::Done;

                            return Poll::Ready(None);
                        }

                        if head == CRLF {
                            let _ = this.buf.split_to(2);
                            this.state = State::Headers;
                            continue;
                        }

                        // Tolerate LF-only.
                        if this.buf[0] == b'\n' {
                            let _ = this.buf.split_to(1);
                            this.state = State::Headers;
                            continue;
                        }

                        this.state = State::Done;

                        return Poll::Ready(None);
                    }
                }

                State::Headers => {
                    if let Some(idx) = find_subslice(&this.buf, DOUBLE_CRLF) {
                        let header_block = this.buf.split_to(idx).freeze();
                        let _ = this.buf.split_to(DOUBLE_CRLF.len());

                        let mut headers: HashMap<String, String> = HashMap::new();

                        let bytes_ref: &[u8] = header_block.as_ref();

                        for line in bytes_ref.split(|b: &u8| *b == b'\n') {
                            let line: &[u8] = if let Some(l) = line.strip_suffix(&[b'\r'][..]) {
                                l
                            } else {
                                line
                            };

                            if line.is_empty() {
                                continue;
                            }

                            let s = match std::str::from_utf8(line) {
                                Ok(s) => s,
                                Err(_) => continue,
                            };

                            if let Some(colon) = s.find(':') {
                                let k = s[..colon].trim().to_lowercase();
                                let v = s[colon + 1..].trim().to_string();
                                headers.insert(k, v);
                            }
                        }

                        let content_length = headers
                            .get("content-length")
                            .and_then(|v| v.parse::<usize>().ok());

                        this.state = State::Body {
                            headers,
                            content_length,
                        };
                        continue;
                    }
                }

                State::Body {
                    headers,
                    content_length,
                } => {
                    // With Content-Length, yield the part the instant
                    // its body bytes arrive. Consuming the trailing
                    // "\r\n--<boundary>" separator is deferred to the
                    // next poll via State::ConsumeSeparator — that
                    // way the caller observes streaming order even
                    // when the next part is slow to land on the wire.
                    //
                    // Without Content-Length we have no choice but to
                    // scan for the separator (it's how the body ends).
                    let (body_end, consume_sep_after_yield) = match *content_length {
                        Some(cl) if this.buf.len() >= cl => (Some(cl), true),
                        Some(_) => (None, false),
                        None => (find_subslice(&this.buf, &this.boundary_sep), false),
                    };

                    if let Some(end) = body_end {
                        let body = this.buf.split_to(end).freeze();

                        let part = BatchPart {
                            headers: std::mem::take(headers),
                            body,
                        };

                        if consume_sep_after_yield {
                            this.state = State::ConsumeSeparator;
                        } else {
                            // Separator was part of the body_end scan;
                            // drop its bytes and go back to the suffix.
                            let _ = this.buf.split_to(this.boundary_sep.len());
                            this.state = State::BoundarySuffix;
                        }

                        return Poll::Ready(Some(Ok(part)));
                    }
                }

                State::ConsumeSeparator => {
                    if let Some(idx) = find_subslice(&this.buf, &this.boundary_sep) {
                        let _ = this.buf.split_to(idx + this.boundary_sep.len());
                        this.state = State::BoundarySuffix;
                        continue;
                    }
                    // Need more bytes; fall through to the pump block.
                }
            }

            // Need more bytes from the underlying stream.
            if this.done {
                return Poll::Ready(None);
            }

            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    this.done = true;
                    // Let the state machine see EOF on next iteration —
                    // usually it'll go to Done.
                    continue;
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(ScrapflyError::Config(format!(
                        "batch stream error: {}",
                        e
                    )))));
                }
                Poll::Ready(Some(Ok(bytes))) => {
                    this.buf.extend_from_slice(&bytes);
                    continue;
                }
            }
        }
    }
}

/// Convenience: take a reqwest `Response` whose Content-Type is
/// multipart/mixed and return a typed `Stream<Item=BatchPart>`.
pub fn parts_from_response(
    resp: reqwest::Response,
) -> Result<BatchPartStream<impl Stream<Item = Result<Bytes, reqwest::Error>>>, ScrapflyError> {
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let (mime, params) = parse_content_type(&ct);

    if mime != "multipart/mixed" {
        return Err(ScrapflyError::Config(format!(
            "scrape_batch: expected Content-Type multipart/mixed, got {:?}",
            ct
        )));
    }

    let boundary = params.get("boundary").cloned().ok_or_else(|| {
        ScrapflyError::Config(format!(
            "scrape_batch: Content-Type multipart/mixed missing boundary: {:?}",
            ct
        ))
    })?;

    Ok(BatchPartStream::new(resp.bytes_stream(), &boundary))
}

/// Header prefix used by the server to forward upstream response
/// headers on proxified batch parts.
const UPSTREAM_PREFIX: &str = "x-scrapfly-upstream-";

/// Synthesize a `BatchProxifiedResponse` from a proxified batch part.
/// Restores the upstream HTTP status from `X-Scrapfly-Scrape-Status`,
/// merges upstream headers (after stripping the `X-Scrapfly-Upstream-`
/// prefix) with Scrapfly metadata headers, and exposes the raw body.
pub fn build_proxified_response(part: BatchPart) -> BatchProxifiedResponse {
    let status: u16 = part
        .headers
        .get("x-scrapfly-scrape-status")
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);

    let mut out_headers: HashMap<String, String> = HashMap::new();

    for (key, value) in &part.headers {
        if key == "content-type" {
            out_headers.insert("content-type".into(), value.clone());
        } else if key.starts_with(UPSTREAM_PREFIX) {
            let stripped = key[UPSTREAM_PREFIX.len()..].to_string();
            out_headers.insert(stripped, value.clone());
        } else if key.starts_with("x-scrapfly-") {
            out_headers.insert(key.clone(), value.clone());
        }
    }

    // Normalize X-Scrapfly-Log-Uuid → X-Scrapfly-Log for parity with
    // the single-scrape proxified response.
    if !out_headers.contains_key("x-scrapfly-log") {
        if let Some(log_uuid) = out_headers.get("x-scrapfly-log-uuid").cloned() {
            out_headers.insert("x-scrapfly-log".into(), log_uuid);
        }
    }

    BatchProxifiedResponse {
        status,
        headers: out_headers,
        body: part.body,
    }
}

/// Decode a part body according to its Content-Type. Supports
/// `application/json` (default) and `application/msgpack`.
pub fn decode_part_body<T: serde::de::DeserializeOwned>(part: &BatchPart) -> Result<T, ScrapflyError> {
    let ct = part
        .headers
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| "application/json".to_string());

    if ct.starts_with("application/json") {
        return serde_json::from_slice::<T>(&part.body)
            .map_err(|e| ScrapflyError::Config(format!("scrape_batch: decode JSON part: {}", e)));
    }

    if ct.starts_with("application/msgpack") || ct.starts_with("application/x-msgpack") {
        return rmp_serde::from_slice::<T>(&part.body).map_err(|e| {
            ScrapflyError::Config(format!("scrape_batch: decode msgpack part: {}", e))
        });
    }

    Err(ScrapflyError::Config(format!(
        "scrape_batch: unsupported part Content-Type: {:?}",
        ct
    )))
}

