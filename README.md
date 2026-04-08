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

See `examples/` for screenshot, extraction, crawler lifecycle and concurrent scrape.

MSRV: 1.75. See <https://scrapfly.io/docs> for the full API reference.
