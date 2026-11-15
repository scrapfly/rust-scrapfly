# Changelog

## 0.7.0 (2026-11-15)

### Breaking changes

- `ScreenshotResult` has a new public field, `job_uuid: Option<String>`.
  Migration: a `ScreenshotResult { .. }` literal adds `job_uuid: None`.
- `BrowserConfig` has a new public field, `downloads_stream: bool`.
  Migration: a `BrowserConfig { .. }` literal adds `downloads_stream: false` or ends with `..Default::default()`.
- A large binary result, fetched from its large-object URL by `scrape()`, holds its bytes in `result.content` as base64,
  as an inline `binary` result does. It held them as lossy UTF-8 text, which lost bytes.
  Migration: base64-decode `content` (standard alphabet) when `result.format` is `binary`.
- `scrape_batch` sends the `method` and `body` of each config. A config with a method other than GET, or a body, ran as
  a GET with no body.
  Migration: none for a GET config; check that each config's method and body are the ones you want sent.
- In `scrape_batch`, a config with a webhook yields `BatchOutcome::Scrape` with its queued result, read from the
  enqueue answer.
  Migration: read `result.result.status` (`QUEUED`); the scrape's result goes to the webhook.
- `scrape()` of a config with a webhook returns `Ok` with the queued result (`result.result.status == "QUEUED"`,
  `success` false) instead of an error.
  Migration: check `result.result.status` instead of matching the error.
- `screenshot()` of a config with a webhook returns `Ok` with `job_uuid` set and an empty `image` and `metadata`,
  instead of an error.
  Migration: read `job_uuid`; the image goes to the webhook.
- `screenshot()` returns an error when the API answers 200 with a JSON body, which was returned as the image.
  Migration: handle the error; such a body was never an image.
- `ScreenshotResult::save` returns an `InvalidData` error for an empty image instead of writing an empty file.
  Migration: check `job_uuid` or `image.is_empty()` before saving.
- `Client::cloud_browser_url` writes a path before the query (`wss://host/?...` instead of `wss://host?...`) and joins a
  host that carries a query of its own with `&`. The server receives the same request.
  Migration: a test comparing the exact URL string updates its expected value.
- An error answer's `error` field is read: an `ERR::` value fills `ApiError.code` and any other text fills
  `ApiError.message`, each only when the answer left it empty. A message under `error` was read as the code, and an
  answer carrying both `code` and `error` gave an empty error.
  Migration: match `code` on `ERR::` codes and read the text from `message`.
- `ScrapflyError::Transport` prints its request URL with `key=REDACTED` in place of the API key, and `url()` of the
  wrapped `reqwest::Error` returns that URL. The `reqwest::Error` stays the error's `source()`.
  Migration: take the API key from your own configuration, never from an error.

### Added

- `Client::crawls_extract`, `Client::crawl_extract` and `Crawl::extract`: an object shaped by a JSON Schema, extracted
  from the content of one or more crawls; `CrawlExtractOptions`, `CrawlerExtractResponse` and its `decode`.
- `Client::crawls_prompt_answer`, `Client::crawl_prompt_answer` and `Crawl::prompt_answer`: the whole answer to a
  prompt in one object; `CrawlerPromptAnswer`, `CrawlerPromptSearch`.
- `CloudBrowserDownloads`, `BrowserDownload`, `parse_downloads`, `CdpSender`, `CdpError` and `DownloadError`: read the
  files a Cloud Browser session downloaded over any CDP client, a large one in chunks from its stream handle;
  `BrowserConfig::downloads_stream` asks for stream handles.
- The `chromiumoxide` feature: `ChromiumoxideCdp`, a `CdpSender` over a chromiumoxide `Page`.

### Changed

- The docs name the proxy pools the API accepts: `ProxyPool::PublicDatacenterPool` (the default),
  `PublicResidentialPool` and `PublicTorPool`. `ProxyPool::Basic`, `Unblocker`, `UnblockerUltra` and `Auto` stay,
  documented as pools the API does not accept yet: a request naming one fails with `ERR::PROXY::POOL_NOT_FOUND`.
- The `unblocker` builder methods are no longer documented as deprecated; `asp` stays their deprecated alias.
- `ScreenshotConfig::proxy_pool` is documented as a value the Screenshot API ignores.
- `ResultData.success` reads a JSON `null` as false instead of failing to decode.
- The schedule docs: kinds such as `api.scrape`, a list scoped to the API key's project and environment, pause and
  resume refused with a 409 on a schedule in the wrong state.
- README, crate docs and the `unblocker` example updated to the above.
