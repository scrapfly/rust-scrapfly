use scrapfly_sdk::{Client, ScrapeConfig, ScrapflyError};

#[tokio::main]
async fn main() {
    let client = Client::builder()
        .api_key("scp-live-YOUR_API_KEY_HERE")
        .host("https://api.scrapfly.local")
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let cfg = ScrapeConfig::builder("https://httpbin.dev/status/403")
        .build()
        .unwrap();
    match client.scrape(&cfg).await {
        Ok(r) => {
            println!(
                "OK: status_code={} success={} status={:?}",
                r.result.status_code, r.result.success, r.result.status
            );
        }
        Err(ScrapflyError::UpstreamClient(e)) => println!("UpstreamClient: {}", e),
        Err(e) => println!("Err: {:?}", e),
    }
}
