//! Extraction example.

use scrapfly_sdk::{Client, ExtractionConfig, ExtractionModel};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("SCRAPFLY_KEY").expect("SCRAPFLY_KEY env var required");
    let client = Client::builder().api_key(key).build()?;
    let html = b"<html><body><h1>hello</h1></body></html>".to_vec();
    let cfg = ExtractionConfig::builder(html, "text/html")
        .extraction_model(ExtractionModel::Article)
        .build()?;
    let result = client.extract(&cfg).await?;
    println!("{}", serde_json::to_string_pretty(&result.data)?);
    Ok(())
}
