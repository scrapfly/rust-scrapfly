//! Use msgpack wire encoding for per-part bodies in a batch.
//!
//! Msgpack produces slightly smaller payloads than JSON and decodes
//! faster. Pass `BatchOptions { format: BatchFormat::Msgpack }` via
//! `scrape_batch_with_options` to opt in — the SDK handles encoding
//! and decoding transparently.
//!
//! Run:
//!   SCRAPFLY_API_KEY=<your-key> cargo run --example batch_scrape_msgpack

use futures_util::stream::StreamExt;
use scrapfly_sdk::batch::{BatchFormat, BatchOptions, BatchOutcome};
use scrapfly_sdk::{Client, ScrapeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("SCRAPFLY_API_KEY").expect("SCRAPFLY_API_KEY must be set");

    let client = Client::builder().api_key(key).build()?;

    let configs = vec![
        ScrapeConfig::builder("https://web-scraping.dev/product/1")
            .correlation_id("product-1")
            .build()?,
        ScrapeConfig::builder("https://web-scraping.dev/product/2")
            .correlation_id("product-2")
            .build()?,
    ];

    let mut stream = client
        .scrape_batch_with_options(
            &configs,
            BatchOptions {
                format: BatchFormat::Msgpack,
            },
        )
        .await?;

    while let Some((correlation_id, outcome)) = stream.next().await {
        match outcome {
            BatchOutcome::Scrape(r) => {
                println!("{}: status={}", correlation_id, r.result.status_code);
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
