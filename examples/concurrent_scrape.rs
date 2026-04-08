//! Concurrent scrape example via `buffer_unordered`.

use futures_util::StreamExt;
use scrapfly_sdk::{Client, ScrapeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("SCRAPFLY_KEY").expect("SCRAPFLY_KEY env var required");
    let client = Client::builder().api_key(key).build()?;
    let configs: Vec<ScrapeConfig> = (1..=5)
        .map(|i| {
            ScrapeConfig::builder(format!("https://httpbin.dev/anything?i={}", i))
                .build()
                .expect("build")
        })
        .collect();
    let mut stream = client.concurrent_scrape(configs, 3);
    while let Some(result) = stream.next().await {
        match result {
            Ok(r) => println!("ok: {}", r.result.status_code),
            Err(e) => eprintln!("err: {}", e),
        }
    }
    Ok(())
}
