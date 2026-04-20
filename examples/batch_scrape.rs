//! Scrape multiple URLs in a single request using the Batch Scraping API.
//!
//! The `scrape_batch` method accepts up to 100 `ScrapeConfig`s and streams
//! each result back as soon as it's ready. Results arrive OUT OF ORDER —
//! use `correlation_id` on each config to match the result back to its
//! originating request.
//!
//! Run:
//!   SCRAPFLY_API_KEY=<your-key> cargo run --example batch_scrape

use futures_util::stream::StreamExt;
use scrapfly_sdk::batch::BatchOutcome;
use scrapfly_sdk::{Client, ScrapeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("SCRAPFLY_API_KEY").expect("SCRAPFLY_API_KEY must be set");

    let client = Client::builder().api_key(key).build()?;

    // Every config in a batch MUST carry a unique `correlation_id` —
    // the SDK uses it to match each streamed result back to its
    // originating config (parts arrive out of order).
    let configs = vec![
        ScrapeConfig::builder("https://web-scraping.dev/product/1")
            .correlation_id("product-1")
            .build()?,
        ScrapeConfig::builder("https://web-scraping.dev/product/2")
            .correlation_id("product-2")
            .build()?,
        ScrapeConfig::builder("https://web-scraping.dev/product/3")
            .correlation_id("product-3")
            .build()?,
    ];

    let mut stream = client.scrape_batch(&configs).await?;

    while let Some((correlation_id, outcome)) = stream.next().await {
        match outcome {
            BatchOutcome::Scrape(r) => {
                println!(
                    "{}: status={} size={} bytes",
                    correlation_id,
                    r.result.status_code,
                    r.result.content.len()
                );
            }
            BatchOutcome::Proxified(r) => {
                println!("{}: proxified status={}", correlation_id, r.status);
            }
            BatchOutcome::Err(e) => {
                eprintln!("{}: error {}", correlation_id, e);
            }
        }
    }

    Ok(())
}
