//! Minimal scrape example. Run with
//! `SCRAPFLY_KEY=scp-... cargo run --example scrape_basic`.

use scrapfly_sdk::{Client, ScrapeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("SCRAPFLY_KEY").expect("SCRAPFLY_KEY env var required");
    let client = Client::builder().api_key(key).build()?;
    let cfg = ScrapeConfig::builder("https://httpbin.dev/html").build()?;
    let result = client.scrape(&cfg).await?;
    println!(
        "status={} size={} content=[{}…]",
        result.result.status_code,
        result.result.content.len(),
        result.result.content.chars().take(120).collect::<String>()
    );
    Ok(())
}
