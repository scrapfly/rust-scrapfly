//! Monitoring API example. Run with
//! `SCRAPFLY_KEY=scp-... cargo run --example monitoring`.
//!
//! Enterprise plan only. See <https://scrapfly.io/docs/monitoring#api>.

use scrapfly_sdk::{
    Client, MonitoringAggregation, MonitoringMetricsOptions, MonitoringPeriod,
    MonitoringTargetMetricsOptions,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("SCRAPFLY_KEY").expect("SCRAPFLY_KEY env var required");
    let client = Client::builder().api_key(key).build()?;

    let account_stats = client
        .get_monitoring_metrics(&MonitoringMetricsOptions {
            aggregation: Some(vec![MonitoringAggregation::Account]),
            period: Some(MonitoringPeriod::Last24h),
            ..Default::default()
        })
        .await?;
    println!("==== Account Metrics ====");
    println!(
        "{}",
        serde_json::to_string_pretty(
            account_stats
                .get("account_metrics")
                .unwrap_or(&account_stats)
        )?
    );

    let target_stats = client
        .get_monitoring_target_metrics(&MonitoringTargetMetricsOptions {
            domain: "httpbin.dev".into(),
            group_subdomain: false,
            period: Some(MonitoringPeriod::Last24h),
            start: None,
            end: None,
            include_webhook: false,
        })
        .await?;
    println!("==== Target Metrics on httpbin.dev ====");
    println!("{}", serde_json::to_string_pretty(&target_stats)?);

    Ok(())
}
