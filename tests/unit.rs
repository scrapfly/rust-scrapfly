//! Offline unit tests — serialization, bounds validation, parser, error classifier.

use scrapfly_sdk::config::scrape::ScrapeConfig;
use scrapfly_sdk::result::crawler::CrawlerUrls;
use scrapfly_sdk::{BrowserConfig, Client, CrawlerConfig, ScrapflyError};

#[test]
fn scrape_config_query_pairs_basic() {
    let cfg = ScrapeConfig::builder("https://example.com")
        .render_js(true)
        .asp(true)
        .country("US")
        .header("X-Test", "value")
        .cookie("sid", "abc")
        .tag("a")
        .tag("b")
        .build()
        .expect("build");
    let pairs = cfg.to_query_pairs().expect("pairs");
    let as_map: std::collections::HashMap<_, _> = pairs.iter().cloned().collect();
    assert_eq!(as_map.get("url"), Some(&"https://example.com".to_string()));
    assert_eq!(as_map.get("render_js"), Some(&"true".to_string()));
    assert_eq!(as_map.get("asp"), Some(&"true".to_string()));
    assert_eq!(as_map.get("country"), Some(&"us".to_string()));
    assert_eq!(as_map.get("headers[x-test]"), Some(&"value".to_string()));
    assert_eq!(as_map.get("headers[cookie]"), Some(&"sid=abc".to_string()));
    assert_eq!(as_map.get("tags"), Some(&"a,b".to_string()));
}

#[test]
fn scrape_config_cookie_merges_with_existing_cookie_header() {
    let cfg = ScrapeConfig::builder("https://example.com")
        .header("Cookie", "existing=1")
        .cookie("sid", "abc")
        .build()
        .expect("build");
    let pairs = cfg.to_query_pairs().expect("pairs");
    let cookie = pairs
        .iter()
        .find(|(k, _)| k == "headers[cookie]")
        .map(|(_, v)| v.clone())
        .expect("cookie header");
    assert!(cookie.contains("existing=1"));
    assert!(cookie.contains("sid=abc"));
}

#[test]
fn scrape_config_js_base64() {
    let cfg = ScrapeConfig::builder("https://example.com")
        .render_js(true)
        .js("window.foo = 1;")
        .build()
        .expect("build");
    let pairs = cfg.to_query_pairs().expect("pairs");
    let js_val = pairs
        .iter()
        .find(|(k, _)| k == "js")
        .map(|(_, v)| v.clone())
        .expect("js param");
    // base64url without padding
    assert!(!js_val.contains('='));
    assert!(!js_val.contains('+'));
    assert!(!js_val.contains('/'));
}

#[test]
fn scrape_config_extraction_mutual_exclusion() {
    let err = ScrapeConfig::builder("https://example.com")
        .extraction_prompt("x")
        .extraction_template("y")
        .build()
        .err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));
}

#[test]
fn crawler_bounds_max_duration_out_of_range() {
    let err = CrawlerConfig::builder("https://x.com")
        .max_duration(5)
        .build()
        .err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));
    let err = CrawlerConfig::builder("https://x.com")
        .max_duration(99999)
        .build()
        .err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));
}

#[test]
fn crawler_bounds_rendering_delay() {
    let err = CrawlerConfig::builder("https://x.com")
        .rendering_delay(30000)
        .build()
        .err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));
}

#[test]
fn crawler_bounds_exclude_paths_limit() {
    let paths: Vec<String> = (0..101).map(|i| format!("/p{}", i)).collect();
    let err = CrawlerConfig::builder("https://x.com")
        .exclude_paths(paths)
        .build()
        .err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));
}

#[test]
fn crawler_urls_parse_visited() {
    let body = "https://a.com/1\nhttps://a.com/2\n\nhttps://a.com/3\n";
    let parsed = CrawlerUrls::from_text(body, "visited", 1, 100);
    assert_eq!(parsed.urls.len(), 3);
    assert_eq!(parsed.urls[0].url, "https://a.com/1");
    assert_eq!(parsed.urls[0].status, "visited");
    assert_eq!(parsed.urls[0].reason, "");
}

#[test]
fn crawler_urls_parse_failed_with_reason() {
    let body = "https://a.com/1,connect timeout\nhttps://a.com/2,dns error\n";
    let parsed = CrawlerUrls::from_text(body, "failed", 2, 50);
    assert_eq!(parsed.urls.len(), 2);
    assert_eq!(parsed.urls[0].url, "https://a.com/1");
    assert_eq!(parsed.urls[0].reason, "connect timeout");
    assert_eq!(parsed.page, 2);
    assert_eq!(parsed.per_page, 50);
}

#[test]
fn error_classifier_429() {
    let body = br#"{"message":"slow down","code":"ERR::THROTTLE::TOO_MANY_REQUESTS"}"#;
    let err = scrapfly_sdk::error::from_response(429, body, 5000, false);
    assert!(matches!(err, ScrapflyError::TooManyRequests(_)));
}

#[test]
fn error_classifier_401() {
    let body = br#"{"message":"bad key","code":"ERR::AUTH::INVALID_KEY"}"#;
    let err = scrapfly_sdk::error::from_response(401, body, 0, false);
    assert!(matches!(err, ScrapflyError::ApiClient(_)));
}

#[test]
fn error_classifier_scrape_failed() {
    let body = br#"{"message":"target refused","code":"ERR::SCRAPE::NETWORK_ERROR"}"#;
    let err = scrapfly_sdk::error::from_response(400, body, 0, false);
    assert!(matches!(err, ScrapflyError::ScrapeFailed(_)));
}

#[test]
fn error_classifier_crawler() {
    let body = br#"{"message":"not found","code":"ERR::CRAWLER::NOT_FOUND"}"#;
    let err = scrapfly_sdk::error::from_response(404, body, 0, true);
    assert!(matches!(err, ScrapflyError::CrawlerFailed(_)));
}

#[test]
fn error_classifier_5xx() {
    let err = scrapfly_sdk::error::from_response(503, b"{}", 0, false);
    assert!(matches!(err, ScrapflyError::ApiServer(_)));
}

#[test]
fn session_sticky_proxy_false_is_sent() {
    // false must reach the wire — omitting it lets the API default to
    // sticky=true with a session, so the user could never disable it.
    let cfg = ScrapeConfig::builder("https://example.com")
        .session("s1")
        .session_sticky_proxy(false)
        .build()
        .expect("build");
    let pairs = cfg.to_query_pairs().expect("pairs");
    let as_map: std::collections::HashMap<_, _> = pairs.iter().cloned().collect();
    assert_eq!(
        as_map.get("session_sticky_proxy"),
        Some(&"false".to_string())
    );
}

#[test]
fn session_sticky_proxy_true_is_sent() {
    let cfg = ScrapeConfig::builder("https://example.com")
        .session("s1")
        .session_sticky_proxy(true)
        .build()
        .expect("build");
    let pairs = cfg.to_query_pairs().expect("pairs");
    let as_map: std::collections::HashMap<_, _> = pairs.iter().cloned().collect();
    assert_eq!(
        as_map.get("session_sticky_proxy"),
        Some(&"true".to_string())
    );
}

#[test]
fn session_sticky_proxy_default_is_true() {
    // Builder defaults sticky on; a session config that never sets it must
    // still send true (matches the API default).
    let cfg = ScrapeConfig::builder("https://example.com")
        .session("s1")
        .build()
        .expect("build");
    let pairs = cfg.to_query_pairs().expect("pairs");
    let as_map: std::collections::HashMap<_, _> = pairs.iter().cloned().collect();
    assert_eq!(
        as_map.get("session_sticky_proxy"),
        Some(&"true".to_string())
    );
}

#[test]
fn session_sticky_proxy_omitted_without_session() {
    let cfg = ScrapeConfig::builder("https://example.com")
        .session_sticky_proxy(true)
        .build()
        .expect("build");
    let pairs = cfg.to_query_pairs().expect("pairs");
    assert!(!pairs.iter().any(|(k, _)| k == "session_sticky_proxy"));
}

// The API stores "<project_salt>-<vnc_password>" and native VNC clients must
// send that exact string, so the separator and the 8-char salt width are a wire
// contract. VNC_TEST_SALT is hardcoded rather than recomputed so a change to the
// derivation fails the test instead of moving with it.
const VNC_TEST_API_KEY: &str = "scp-test-0000000000000000000000000000000000";
const VNC_TEST_SALT: &str = "701018da";

#[test]
fn vnc_client_password_matches_server_salting() {
    let cfg = BrowserConfig {
        enable_vnc: true,
        vnc_password: Some("hunter2".into()),
        ..Default::default()
    };
    assert_eq!(
        cfg.vnc_client_password(VNC_TEST_API_KEY),
        Some(format!("{VNC_TEST_SALT}-hunter2"))
    );
}

#[test]
fn vnc_client_password_none_when_server_would_not_salt() {
    let cases = [
        ("password unset", true, None),
        ("password empty", true, Some(String::new())),
        ("vnc disabled", false, Some("hunter2".to_string())),
    ];

    for (case, enable_vnc, vnc_password) in cases {
        let cfg = BrowserConfig {
            enable_vnc,
            vnc_password,
            ..Default::default()
        };
        assert_eq!(cfg.vnc_client_password(VNC_TEST_API_KEY), None, "{case}");
    }
}

// target_url is what lets the server pick a proxy that serves the destination. A
// session that omits it is routed blind, and an upstream provider refusing the
// target fails the whole run at CONNECT time, so the parameter has to reach the
// wire rather than merely being stored on the config.
#[test]
fn cloud_browser_url_sends_target_url() {
    let client = Client::builder()
        .api_key(VNC_TEST_API_KEY.to_string())
        .build()
        .expect("client");
    let cfg = BrowserConfig {
        target_url: Some("https://web-scraping.dev/products".to_string()),
        ..Default::default()
    };

    let url = client.cloud_browser_url(&cfg);

    assert!(
        url.contains("target_url=https%3A%2F%2Fweb-scraping.dev%2Fproducts"),
        "target_url missing or unescaped in {url}"
    );
}

#[test]
fn cloud_browser_url_omits_target_url_when_unset() {
    let client = Client::builder()
        .api_key(VNC_TEST_API_KEY.to_string())
        .build()
        .expect("client");

    let url = client.cloud_browser_url(&BrowserConfig::default());

    assert!(
        !url.contains("target_url"),
        "target_url must not be sent when unset, got {url}"
    );
}
