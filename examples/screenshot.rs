//! Screenshot example.

use scrapfly_sdk::{Client, ScreenshotConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("SCRAPFLY_KEY").expect("SCRAPFLY_KEY env var required");
    let client = Client::builder().api_key(key).build()?;
    let cfg = ScreenshotConfig::builder("https://web-scraping.dev/product/1").build()?;
    let result = client.screenshot(&cfg).await?;
    let path = result.save("product", None)?;
    println!("saved: {} ({} bytes)", path.display(), result.image.len());
    Ok(())
}
