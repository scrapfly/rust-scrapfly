# scrapfly-sdk

Async Rust client for the [Scrapfly](https://scrapfly.io) web scraping, screenshot,
extraction and crawler APIs. Mirrors the shape of the official Python, TypeScript
and Go SDKs.

- Single shared `reqwest::Client` with `rustls` TLS
- Typed builders for every config (`ScrapeConfig`, `ScreenshotConfig`,
  `ExtractionConfig`, `CrawlerConfig`)
- High-level `Crawl` wrapper with `start` / `wait` / `urls` / `read` / `warc` / `har`
- `concurrent_scrape` returns a `Stream` powered by `buffer_unordered`
- Categorized `ScrapflyError` with sentinel variants for rate-limit, upstream 4xx/5xx,
  crawler failure/cancel/timeout, etc.
- Zero `unwrap()` / `expect()` in library code
- No HTML parser bundled — bring your own (e.g. `scraper`, `kuchiki`)

## Install

```sh
cargo add scrapfly-sdk
cargo add tokio --features full
```

## Quick start

```rust
use scrapfly_sdk::{Client, ScrapeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .api_key(std::env::var("SCRAPFLY_KEY")?)
        .build()?;

    let result = client
        .scrape(&ScrapeConfig::builder("https://httpbin.dev/html").build()?)
        .await?;

    println!("status={} size={}", result.result.status_code, result.result.content.len());
    Ok(())
}
```

## Unblocker

The Unblocker is Scrapfly's anti-bot bypass. Turn it on per request:

```rust
let cfg = ScrapeConfig::builder("https://example.com")
    .unblocker(true)
    .build()?;
```

`CrawlerConfig::builder("https://example.com").unblocker(true)` does the same for
a crawl.

`asp` is the previous name for this feature. `ScrapeConfigBuilder::asp` and
`CrawlerConfigBuilder::asp` are deprecated input aliases that keep working and
are not going away — existing code needs no edit. When both names are supplied,
`asp` wins in either call order, so an explicit `.asp(false)` still turns the
feature off. The two names are never OR-ed.

On the built structs there is a single field, `ScrapeConfig::asp` /
`CrawlerConfig::asp`. It keeps the old name because that name is the wire key
and renaming a public field would stop existing callers from compiling; there
is deliberately no second `unblocker` field for it to disagree with. Read it
with `unblocker_enabled()` and write it with `set_unblocker()`, so a config can
still be flipped after `build()`:

```rust
let mut cfg = ScrapeConfig::builder("https://example.com")
    .unblocker(true)
    .build()?;
cfg.set_unblocker(false); // same slot as `cfg.asp = false`; feature is off
```

The request itself continues to carry the parameter as `asp`. That is a
server-compatibility detail of this release, not something to depend on.

The failure variant keeps its name too: `ScrapflyError::AspBypassFailed` is
dispatched from the literal `ERR::ASP::*` codes the API returns. Rust cannot
alias an enum variant, so the current name reaches the error surface as a
predicate — `err.is_unblocker_failure()` is exactly a match on that variant, and
is the counterpart of Go's `ErrUnblockerBypassFailed` and the TypeScript and
Python SDKs' `ScrapflyUnblockerError`.

See `examples/unblocker.rs`, and `examples/` for screenshot, extraction, crawler
lifecycle and concurrent scrape.

MSRV: 1.75. See <https://scrapfly.io/docs> for the full API reference.
