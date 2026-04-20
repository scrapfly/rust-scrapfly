//! Mix proxified and JSON-envelope scrapes in a single batch.
//!
//! A config with `proxified_response=true` returns the raw upstream
//! response (HTML, JSON, image bytes, etc.) instead of Scrapfly's JSON
//! envelope. In a batch, proxified parts surface as
//! `BatchOutcome::Proxified` while normal parts surface as
//! `BatchOutcome::Scrape`.
//!
//! Run:
//!   SCRAPFLY_API_KEY=<your-key> cargo run --example batch_scrape_proxified

use futures_util::stream::StreamExt;
use scrapfly_sdk::batch::BatchOutcome;
use scrapfly_sdk::{Client, ScrapeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("SCRAPFLY_API_KEY").expect("SCRAPFLY_API_KEY must be set");

    let client = Client::builder().api_key(key).build()?;

    let configs = vec![
        // Proxified: returns raw upstream HTML bytes + upstream headers.
        ScrapeConfig::builder("https://web-scraping.dev/product/1")
            .correlation_id("html")
            .proxified_response()
            .build()?,
        // Normal: returns Scrapfly's JSON envelope with result, config, context.
        ScrapeConfig::builder("https://web-scraping.dev/api/products")
            .correlation_id("api")
            .build()?,
    ];

    let mut stream = client.scrape_batch(&configs).await?;

    while let Some((correlation_id, outcome)) = stream.next().await {
        match outcome {
            BatchOutcome::Proxified(r) => {
                // Raw upstream response: r.body is the upstream bytes,
                // r.headers carries upstream headers + Scrapfly metadata.
                println!(
                    "{}: proxified status={} content-type={:?} body={} bytes",
                    correlation_id,
                    r.status,
                    r.content_type(),
                    r.body.len()
                );
            }
            BatchOutcome::Scrape(r) => {
                println!(
                    "{}: scrape status={} size={} bytes",
                    correlation_id,
                    r.result.status_code,
                    r.result.content.len()
                );
            }
            BatchOutcome::Err(e) => {
                eprintln!("{}: error {}", correlation_id, e);
            }
        }
    }

    Ok(())
}
