//! `basic`, `unblocker`, `unblocker_ultra` and `auto` ride `proxy_pool`. The
//! API resolves each to its proxies and the Unblocker, so the SDK only has to
//! emit the exact wire value; the pool names written before them stay members,
//! unchanged.

use scrapfly_sdk::{CrawlerConfig, ProxyPool, ScrapeConfig, ScreenshotConfig};

const POOLS: [(ProxyPool, &str); 4] = [
    (ProxyPool::Basic, "basic"),
    (ProxyPool::Unblocker, "unblocker"),
    (ProxyPool::UnblockerUltra, "unblocker_ultra"),
    (ProxyPool::Auto, "auto"),
];

const LEGACY: [(ProxyPool, &str); 2] = [
    (ProxyPool::PublicDatacenterPool, "public_datacenter_pool"),
    (ProxyPool::PublicResidentialPool, "public_residential_pool"),
];

/// The Tor pool is a current pool, not a name written before the proxy pools.
const TOR: (ProxyPool, &str) = (ProxyPool::PublicTorPool, "public_tor_pool");

fn value<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[test]
fn pool_wire_values_and_serde_agree() {
    for (pool, wire) in POOLS.iter().chain(LEGACY.iter()).chain([TOR].iter()) {
        assert_eq!(pool.as_str(), *wire);
        assert_eq!(serde_json::to_value(pool).unwrap(), serde_json::json!(wire));
        let parsed: ProxyPool = serde_json::from_value(serde_json::json!(wire)).unwrap();
        assert_eq!(parsed, *pool);
    }
}

#[test]
fn is_legacy_separates_legacy_names_from_the_new_pools() {
    for (pool, _) in POOLS {
        assert!(!pool.is_legacy(), "{pool:?} is not a legacy pool name");
    }
    for (pool, _) in LEGACY {
        assert!(pool.is_legacy(), "{pool:?} is a legacy pool name");
    }
    assert!(!TOR.0.is_legacy(), "the Tor pool is a current pool");
}

#[test]
fn scrape_sends_the_pool_without_asp() {
    for (pool, wire) in POOLS {
        let cfg = ScrapeConfig::builder("https://example.com")
            .proxy_pool(pool)
            .build()
            .expect("build");
        let pairs = cfg.to_query_pairs().expect("pairs");
        assert_eq!(value(&pairs, "proxy_pool"), Some(wire));
        assert_eq!(value(&pairs, "asp"), None, "{wire} emitted asp");
    }
    let cfg = ScrapeConfig::builder("https://example.com")
        .build()
        .expect("build");
    let pairs = cfg.to_query_pairs().expect("pairs");
    assert_eq!(
        value(&pairs, "proxy_pool"),
        None,
        "the SDK sends no default pool"
    );
}

#[test]
fn crawler_takes_the_pool_enum_as_its_pool_string() {
    for (pool, wire) in POOLS {
        let cfg = CrawlerConfig::builder("https://example.com")
            .proxy_pool(pool)
            .build()
            .expect("build");
        let body: serde_json::Value =
            serde_json::from_slice(&cfg.to_json_body().expect("body")).expect("json");
        assert_eq!(body["proxy_pool"], serde_json::json!(wire));
        assert!(body.get("asp").is_none(), "{wire} emitted asp");
    }
}

#[test]
fn legacy_pool_with_unblocker_is_unchanged() {
    let cfg = ScrapeConfig::builder("https://example.com")
        .proxy_pool(ProxyPool::PublicResidentialPool)
        .unblocker(true)
        .build()
        .expect("build");
    let pairs = cfg.to_query_pairs().expect("pairs");
    assert_eq!(value(&pairs, "proxy_pool"), Some("public_residential_pool"));
    assert_eq!(value(&pairs, "asp"), Some("true"));
}

#[test]
fn screenshot_sends_the_pool_as_proxy_pool() {
    for (pool, wire) in POOLS {
        let cfg = ScreenshotConfig::builder("https://example.com")
            .proxy_pool(pool)
            .build()
            .expect("build");
        let pairs = cfg.to_query_pairs().expect("pairs");
        assert_eq!(value(&pairs, "proxy_pool"), Some(wire));
    }
    let cfg = ScreenshotConfig::builder("https://example.com")
        .build()
        .expect("build");
    let pairs = cfg.to_query_pairs().expect("pairs");
    assert_eq!(value(&pairs, "proxy_pool"), None, "no pool, no proxy_pool");
}
