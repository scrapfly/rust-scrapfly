//! Unblocker (anti-bot bypass) example. Run with
//! `SCRAPFLY_KEY=scp-... cargo run --example unblocker`.
//!
//! `unblocker` is the current name for this feature. `asp` is the deprecated
//! alias: it keeps working, and when both names are supplied it wins, in
//! either call order.

use scrapfly_sdk::{Client, CrawlerConfig, ScrapeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("SCRAPFLY_KEY").expect("SCRAPFLY_KEY env var required");
    let client = Client::builder().api_key(key).build()?;

    let cfg = ScrapeConfig::builder("https://web-scraping.dev/products")
        .unblocker(true)
        .country("US")
        .build()?;
    let result = client.scrape(&cfg).await?;
    println!(
        "status={} size={}",
        result.result.status_code,
        result.result.content.len()
    );

    // A built config can still be flipped: `set_unblocker` writes the same
    // single slot the deprecated `asp` field is, so opting one request out of
    // a shared template really turns the feature off.
    let mut cheap = cfg.clone();
    cheap.set_unblocker(false);
    println!("cheap unblocker={}", cheap.unblocker_enabled());

    // Same knob on a crawl.
    let crawl_cfg = CrawlerConfig::builder("https://web-scraping.dev/products")
        .unblocker(true)
        .page_limit(3)
        .build()?;
    println!("crawl unblocker={}", crawl_cfg.unblocker_enabled());

    Ok(())
}
