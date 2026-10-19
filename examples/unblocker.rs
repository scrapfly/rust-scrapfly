//! Unblocker (anti-bot bypass) example. Run with
//! `SCRAPFLY_KEY=scp-... cargo run --example unblocker`.
//!
//! `proxy_pool` takes `ProxyPool::Basic`, `ProxyPool::Unblocker`,
//! `ProxyPool::UnblockerUltra` or `ProxyPool::Auto`. The `unblocker` toggle
//! and its `asp` alias are deprecated: use `proxy_pool`
//! (`ProxyPool::UnblockerUltra` for the Unblocker). The migration guide covers
//! requests written before the proxy pools:
//! <https://scrapfly.io/new-pricing#legacy-spellings>

use scrapfly_sdk::{Client, CrawlerConfig, ProxyPool, ScrapeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("SCRAPFLY_KEY").expect("SCRAPFLY_KEY env var required");
    let client = Client::builder().api_key(key).build()?;

    let cfg = ScrapeConfig::builder("https://web-scraping.dev/products")
        .proxy_pool(ProxyPool::Unblocker)
        .country("US")
        .build()?;
    let result = client.scrape(&cfg).await?;
    println!(
        "status={} size={} pool={}",
        result.result.status_code,
        result.result.content.len(),
        result.context["proxy"]["pool"]
    );

    // A target or anti-bot protection that has a price of its own answers
    // basic and unblocker with ERR::PROXY::UNBLOCKER_ULTRA_REQUIRED.
    let mut ultra = cfg.clone();
    ultra.proxy_pool = Some(ProxyPool::UnblockerUltra);
    println!("ultra proxy_pool={:?}", ultra.proxy_pool);

    // Same knob on a crawl.
    let crawl_cfg = CrawlerConfig::builder("https://web-scraping.dev/products")
        .proxy_pool(ProxyPool::Unblocker)
        .page_limit(3)
        .build()?;
    println!("crawl proxy_pool={:?}", crawl_cfg.proxy_pool);

    Ok(())
}
