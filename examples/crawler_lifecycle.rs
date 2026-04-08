//! High-level crawler lifecycle example.

use std::time::Duration;

use scrapfly_sdk::{Client, Crawl, CrawlerConfig, WaitOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("SCRAPFLY_KEY").expect("SCRAPFLY_KEY env var required");
    let client = Client::builder().api_key(key).build()?;
    let config = CrawlerConfig::builder("https://web-scraping.dev/products")
        .page_limit(3)
        .max_depth(1)
        .build()?;
    let mut crawl = Crawl::new(&client, config);
    crawl.start().await?;
    println!("uuid={}", crawl.uuid());
    crawl
        .wait(WaitOptions {
            poll_interval: Duration::from_secs(3),
            max_wait: Some(Duration::from_secs(150)),
            ..Default::default()
        })
        .await?;
    let urls = crawl.urls(Some("visited"), 1, 50).await?;
    println!("visited {} urls", urls.urls.len());
    Ok(())
}
