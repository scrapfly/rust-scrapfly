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

## Proxy pools

`proxy_pool` picks the proxy pool of the Web Scraping API, the Crawler and the
Screenshot API. Each pool sets both the proxy network and the anti-bot bypass
(the Unblocker):

| Variant | Wire value | What it runs |
|---|---|---|
| `ProxyPool::Basic` | `basic` | datacenter proxies, Unblocker off |
| `ProxyPool::Unblocker` | `unblocker` | residential proxies, Unblocker on |
| `ProxyPool::UnblockerUltra` | `unblocker_ultra` | residential proxies, Unblocker on, the only pool accepted by targets and anti-bot protections that have a price of their own |
| `ProxyPool::Auto` | `auto` | the API picks the pool before the request, from the target's profile: the Unblocker on datacenter proxies, or the pool the target needs; accepted on every target, the ones that require `unblocker_ultra` included, where their price is added; priced as the pool it runs on |

```rust
let cfg = ScrapeConfig::builder("https://example.com")
    .proxy_pool(ProxyPool::Unblocker)
    .build()?;
```

`CrawlerConfig::builder("https://example.com").proxy_pool(ProxyPool::Unblocker)`
does the same for a crawl. A request on `basic` or `unblocker` against a target
or anti-bot protection that has a price of its own fails with
`ERR::PROXY::UNBLOCKER_ULTRA_REQUIRED` (HTTP 422, not billed); retry it with
`ProxyPool::UnblockerUltra`. Without `proxy_pool` the SDK sends none.
`ProxyPool::PublicTorPool` (`public_tor_pool`) routes through the Tor network,
the only pool that reaches `.onion` sites.

The answer names the pool you asked for the way your account reads it.
`result.config["proxy_pool"]` and
`result.context["proxy"]["pool"]` read `public_tor_pool` for Tor and one of
your own pools by its name. Otherwise, when the Unblocker ran (asked, or forced
by a target that requires it), `basic` and `unblocker` read `unblocker` and any
other pool `unblocker_ultra`. Without the Unblocker a Scrapfly pool reads by its
network (`basic`, `unblocker`, `mobile`, `isp`) and any other name as sent.
`result.context["proxy"]` also gives the network and country.

The Screenshot API takes these four pools only and ignores any other value.
Cloud Browser keeps its own pools, `datacenter`, `residential`, `tor` and
`byop`, and refuses these four.

`ProxyPool` is `#[non_exhaustive]`: a `match` on it needs a wildcard arm.

### Deprecated: `unblocker` and `asp`

The `unblocker` toggle and its `asp` alias are deprecated: use `proxy_pool`
(`ProxyPool::UnblockerUltra` for the Unblocker). The
[migration guide](https://scrapfly.io/new-pricing#legacy-spellings) covers the
requests written before the proxy pools, the pool names
`ProxyPool::PublicDatacenterPool` and `ProxyPool::PublicResidentialPool`
included.

`asp` is the previous name of the toggle. `ScrapeConfigBuilder::asp` and
`CrawlerConfigBuilder::asp` are its deprecated input aliases. When both names
are supplied, `asp` wins in either call order, so an explicit `.asp(false)`
still turns the feature off. The two names are never OR-ed.

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
