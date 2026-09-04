//! Offline unit tests — serialization, bounds validation, parser, error classifier.

use scrapfly_sdk::config::crawler::{REFRESH_MAX_INTERVAL, REFRESH_MIN_INTERVAL};
use scrapfly_sdk::config::scrape::ScrapeConfig;
use scrapfly_sdk::result::crawler::CrawlerUrls;
use scrapfly_sdk::{
    BrowserConfig, Client, CrawlRefreshSettings, CrawlSearchOptions, CrawlerConfig,
    CrawlerPromptDone, CrawlerPromptEvent, CrawlerRefreshState, CrawlerSearchMode,
    CrawlerSearchResponse, CrawlerStatus, CrawlerWebhook, CrawlerWebhookEvent, ScrapflyError,
};

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

#[test]
fn crawler_search_serializes_to_wire_payload() {
    let cfg = CrawlerConfig::builder("https://example.com")
        .search(true)
        .build()
        .expect("valid config");
    let body: serde_json::Value =
        serde_json::from_slice(&cfg.to_json_body().expect("serializable")).expect("valid json");
    assert_eq!(body["search"], serde_json::Value::Bool(true));
}

#[test]
fn crawler_search_omitted_when_off() {
    // Unset means server default: never emit a field to send its default.
    let cfg = CrawlerConfig::builder("https://example.com")
        .build()
        .expect("valid config");
    let body: serde_json::Value =
        serde_json::from_slice(&cfg.to_json_body().expect("serializable")).expect("valid json");
    assert!(body.get("search").is_none());
}

#[test]
fn crawler_search_webhook_events_round_trip() {
    let cfg = CrawlerConfig::builder("https://example.com")
        .webhook_name("hook")
        .webhook_event(CrawlerWebhookEvent::CrawlerSearchReady)
        .webhook_event(CrawlerWebhookEvent::CrawlerSearchFailed)
        .build()
        .expect("valid config");
    let body: serde_json::Value =
        serde_json::from_slice(&cfg.to_json_body().expect("serializable")).expect("valid json");
    assert_eq!(
        body["webhook_events"],
        serde_json::json!(["crawler_search_ready", "crawler_search_failed"])
    );

    // The wire name comes from serde's snake_case rename; as_str() must agree
    // or a webhook name would differ depending on which path produced it.
    assert_eq!(
        CrawlerWebhookEvent::CrawlerSearchReady.as_str(),
        "crawler_search_ready"
    );
    assert_eq!(
        CrawlerWebhookEvent::CrawlerSearchFailed.as_str(),
        "crawler_search_failed"
    );
}

#[test]
fn crawler_search_webhook_events_deserialize() {
    let event: CrawlerWebhookEvent =
        serde_json::from_str("\"crawler_search_ready\"").expect("known event");
    assert_eq!(event, CrawlerWebhookEvent::CrawlerSearchReady);
    let event: CrawlerWebhookEvent =
        serde_json::from_str("\"crawler_search_failed\"").expect("known event");
    assert_eq!(event, CrawlerWebhookEvent::CrawlerSearchFailed);
}

#[test]
fn crawler_updated_webhook_event_round_trips() {
    let cfg = CrawlerConfig::builder("https://example.com")
        .webhook_name("hook")
        .webhook_event(CrawlerWebhookEvent::CrawlerUpdated)
        .build()
        .expect("valid config");
    let body: serde_json::Value =
        serde_json::from_slice(&cfg.to_json_body().expect("serializable")).expect("valid json");
    assert_eq!(
        body["webhook_events"],
        serde_json::json!(["crawler_updated"])
    );

    assert_eq!(
        CrawlerWebhookEvent::CrawlerUpdated.as_str(),
        "crawler_updated"
    );
    let event: CrawlerWebhookEvent =
        serde_json::from_str("\"crawler_updated\"").expect("known event");
    assert_eq!(event, CrawlerWebhookEvent::CrawlerUpdated);
}

#[test]
fn crawler_search_response_parses_the_envelope() {
    let body = br#"{
      "query": "TLS fingerprint",
      "mode": "hybrid",
      "limit": 20,
      "completeness": "exact",
      "crawls": [{"crawler_uuid": "0198aaaa", "documents": 412, "vectors": 18432, "index": "IVF_PQ"}],
      "skipped": [{"crawler_uuid": "0198bbbb", "reason": "search_not_ready", "status": "BUILDING"}],
      "results": [{
        "rank": 1, "score": 0.927,
        "scores": {"vector": 0.91, "fts": 12.4, "rrf": 0.0312},
        "crawler_uuid": "0198aaaa", "url": "https://example.com/foo", "title": "Foo Product",
        "source_format": "markdown", "content_type": "application/markdown",
        "chunk_id": 3, "text": "the matched chunk",
        "warc_offset": 728271, "warc_end": 746643,
        "contents_url": "https://api.scrapfly.io/crawl/0198aaaa/contents?url=x"
      }],
      "stats": {"duration_ms": 412, "crawls_searched": 1, "candidates": 150, "gcs_gets": 27},
      "crawls_requested": 2, "crawls_searched": 1, "theta": 0.42, "cursor": null,
      "crawls_skipped_deadline": [], "crawls_failed": []
    }"#;
    let parsed: CrawlerSearchResponse = serde_json::from_slice(body).expect("valid envelope");

    assert!(parsed.is_exact());
    assert_eq!(parsed.results.len(), 1);
    assert_eq!(parsed.results[0].url, "https://example.com/foo");
    assert_eq!(parsed.results[0].scores.rrf, Some(0.0312));
    assert_eq!(parsed.results[0].warc_end, Some(746643));
    assert_eq!(parsed.skipped[0].reason, "search_not_ready");
    assert_eq!(parsed.crawls[0].vectors, 18432);
    assert_eq!(parsed.cursor, None);
    assert!(parsed.crawls_skipped_deadline.is_empty());
    assert!(parsed.crawls_failed.is_empty());
}

#[test]
fn crawler_search_response_names_the_deadline_and_failed_crawls() {
    // Both fields are lists on the wire: they name the crawls that did not
    // contribute so the caller can retry them. Typing either as a counter is
    // not a lossy decode, it is a hard serde error on every real response.
    let body = br#"{
      "query": "q", "mode": "hybrid", "limit": 20, "completeness": "partial",
      "crawls": [], "skipped": [], "results": [],
      "stats": {"duration_ms": 5000, "crawls_searched": 0, "candidates": 0, "gcs_gets": 0},
      "crawls_requested": 3, "crawls_searched": 0, "crawls_pruned_exact": 0,
      "crawls_skipped_deadline": ["0198cccc", "0198dddd"],
      "crawls_failed": [{"crawler_uuid": "0198eeee", "reason": "incompatible_index",
                         "status": "READY"}],
      "theta": null, "max_ub_unsearched": null, "cursor": null
    }"#;
    let parsed: CrawlerSearchResponse = serde_json::from_slice(body).expect("valid envelope");

    assert!(!parsed.is_exact());
    assert_eq!(
        parsed.crawls_skipped_deadline,
        vec!["0198cccc".to_string(), "0198dddd".to_string()]
    );
    assert_eq!(parsed.crawls_failed.len(), 1);
    assert_eq!(parsed.crawls_failed[0].crawler_uuid, "0198eeee");
    assert_eq!(parsed.crawls_failed[0].reason, "incompatible_index");
    // null is not the claim that the bound was zero.
    assert_eq!(parsed.theta, None);
    assert_eq!(parsed.max_ub_unsearched, None);
}

#[test]
fn crawler_status_parses_the_search_block() {
    let body = br#"{
      "crawler_uuid": "abc", "status": "DONE", "is_finished": true, "is_success": true,
      "state": {"urls_visited": 1, "urls_extracted": 1, "duration": 1},
      "search": {"status": "READY", "documents": 412, "vectors": 18432,
                 "index": "IVF_PQ", "generation": 1}
    }"#;
    let parsed: CrawlerStatus = serde_json::from_slice(body).expect("valid status");
    let search = parsed.search.expect("search block present");
    assert!(search.is_searchable());
    assert_eq!(search.vectors, 18432);
    assert_eq!(search.generation, Some(1));
}

#[test]
fn crawler_status_search_absent_on_a_crawl_without_it() {
    let body = br#"{
      "crawler_uuid": "abc", "status": "DONE", "is_finished": true, "is_success": true,
      "state": {"urls_visited": 1, "urls_extracted": 1, "duration": 1}
    }"#;
    let parsed: CrawlerStatus = serde_json::from_slice(body).expect("valid status");
    assert!(parsed.search.is_none());
}

#[tokio::test]
async fn crawl_search_rejects_bad_input_before_any_request() {
    // The host is unreachable on purpose: a request would fail with a
    // transport error, so a Config error proves nothing was sent.
    let client = Client::builder()
        .api_key("scp-live-key")
        .host("http://127.0.0.1:1")
        .build()
        .expect("client");

    let err = client.crawls_search(&[], "q", None).await.err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));

    let dup = vec!["a".to_string(), "a".to_string()];
    let err = client.crawls_search(&dup, "q", None).await.err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));

    let one = vec!["a".to_string()];
    let err = client.crawls_search(&one, "", None).await.err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));

    let err = client.crawls_prompt(&[], "p", None).await.err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));

    let err = client.crawls_prompt(&one, "", None).await.err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));
}

#[test]
fn crawler_search_mode_wire_values() {
    assert_eq!(CrawlerSearchMode::Vector.as_str(), "vector");
    assert_eq!(CrawlerSearchMode::Fts.as_str(), "fts");
    assert_eq!(CrawlerSearchMode::Hybrid.as_str(), "hybrid");
}

#[tokio::test]
async fn crawl_search_options_reject_a_non_object_filter_before_any_request() {
    // CrawlSearchOptions::apply is private, so drive it through the public
    // call: an unreachable host means a Config error can only come from
    // option validation, never from the wire.
    let client = Client::builder()
        .api_key("scp-live-key")
        .host("http://127.0.0.1:1")
        .build()
        .expect("client");
    let ids = vec!["a".to_string()];

    let opts = CrawlSearchOptions {
        filters: Some(serde_json::json!(["url_prefix"])),
        ..Default::default()
    };
    let err = client.crawls_search(&ids, "q", Some(&opts)).await.err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));

    // A well-formed option set gets past validation and fails on transport.
    let opts = CrawlSearchOptions {
        limit: Some(20),
        mode: Some(CrawlerSearchMode::Hybrid),
        filters: Some(serde_json::json!({"url_prefix": "https://example.com/docs/"})),
        cursor: Some("abc".into()),
    };
    let err = client.crawls_search(&ids, "q", Some(&opts)).await.err();
    assert!(!matches!(err, Some(ScrapflyError::Config(_))));
}

#[test]
fn crawler_refresh_serializes_to_wire_payload() {
    let cfg = CrawlerConfig::builder("https://example.com")
        .refresh(true)
        .refresh_interval(86400)
        .build()
        .expect("valid config");
    let body: serde_json::Value =
        serde_json::from_slice(&cfg.to_json_body().expect("serializable")).expect("valid json");
    assert_eq!(body["refresh"], serde_json::Value::Bool(true));
    assert_eq!(body["refresh_interval"], serde_json::json!(86400));
}

#[test]
fn crawler_refresh_omitted_when_off() {
    // Unset means server default: never emit a field to send its default.
    let cfg = CrawlerConfig::builder("https://example.com")
        .build()
        .expect("valid config");
    let body: serde_json::Value =
        serde_json::from_slice(&cfg.to_json_body().expect("serializable")).expect("valid json");
    assert!(body.get("refresh").is_none());
    assert!(body.get("refresh_interval").is_none());

    // Refresh on with no interval leaves the server period alone.
    let cfg = CrawlerConfig::builder("https://example.com")
        .refresh(true)
        .build()
        .expect("valid config");
    let body: serde_json::Value =
        serde_json::from_slice(&cfg.to_json_body().expect("serializable")).expect("valid json");
    assert_eq!(body["refresh"], serde_json::Value::Bool(true));
    assert!(body.get("refresh_interval").is_none());
}

#[test]
fn crawler_refresh_interval_bounds_are_enforced() {
    // The floor decides the cost; reject before a round trip.
    for interval in [1u32, REFRESH_MIN_INTERVAL - 1, REFRESH_MAX_INTERVAL + 1] {
        let err = CrawlerConfig::builder("https://example.com")
            .refresh(true)
            .refresh_interval(interval)
            .build()
            .err();
        assert!(
            matches!(err, Some(ScrapflyError::Config(_))),
            "interval {} accepted",
            interval
        );
    }
    for interval in [REFRESH_MIN_INTERVAL, 86400, REFRESH_MAX_INTERVAL] {
        assert!(
            CrawlerConfig::builder("https://example.com")
                .refresh(true)
                .refresh_interval(interval)
                .build()
                .is_ok(),
            "interval {} rejected",
            interval
        );
    }
    // A period with the feature off would silently never run.
    let err = CrawlerConfig::builder("https://example.com")
        .refresh_interval(86400)
        .build()
        .err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));
}

#[test]
fn crawler_status_parses_the_refresh_block() {
    let body = br#"{
      "crawler_uuid": "abc", "status": "DONE", "is_finished": true, "is_success": true,
      "state": {"urls_visited": 1, "urls_extracted": 1, "duration": 1},
      "refresh": {"enabled": true, "interval_seconds": 86400, "status": "SCHEDULED",
                  "generation": 2, "next_run_at": "2026-09-02T04:00:00Z",
                  "started_at": null, "consecutive_failures": 0,
                  "history": [{"at": "2026-09-01T04:00:00Z", "generation": 2, "added": 3,
                               "updated": 7, "removed": 1, "unchanged": 404,
                               "sample_removed": ["https://example.com/old"]}]}
    }"#;
    let parsed: CrawlerStatus = serde_json::from_slice(body).expect("valid status");
    let refresh = parsed.refresh.expect("refresh block present");
    assert!(refresh.enabled);
    assert_eq!(refresh.interval_seconds, 86400);
    assert!(!refresh.is_running());
    // No run is in flight, so the schedule names its next date and not a start.
    assert_eq!(refresh.started_at, None);
    assert_eq!(refresh.consecutive_failures, 0);
    let last = refresh.last_run().expect("one run recorded");
    assert_eq!(last.changed(), 11);
    assert_eq!(last.unchanged, 404);
    assert_eq!(last.sample_removed, vec!["https://example.com/old"]);
}

#[test]
fn crawler_status_refresh_reports_an_in_flight_run_and_its_failure_streak() {
    // started_at and consecutive_failures reach the customer on the status
    // route only. Dropping them leaves a RUNNING refresh with no start time
    // and a repeatedly failing schedule indistinguishable from a healthy one.
    let body = br#"{
      "crawler_uuid": "abc", "status": "DONE", "is_finished": true, "is_success": true,
      "state": {"urls_visited": 1, "urls_extracted": 1, "duration": 1},
      "refresh": {"enabled": true, "interval_seconds": 3600, "status": "RUNNING",
                  "generation": 4, "last_run_at": "2026-09-03T21:00:00Z",
                  "next_run_at": "2026-09-03T22:00:00Z",
                  "started_at": "2026-09-03T21:31:03.851147Z",
                  "consecutive_failures": 3,
                  "error": "upstream 503", "history": []}
    }"#;
    let parsed: CrawlerStatus = serde_json::from_slice(body).expect("valid status");
    let refresh = parsed.refresh.expect("refresh block present");
    assert!(refresh.is_running());
    assert_eq!(
        refresh.started_at.as_deref(),
        Some("2026-09-03T21:31:03.851147Z")
    );
    assert_eq!(refresh.consecutive_failures, 3);
}

#[test]
fn crawler_status_refresh_absent_on_a_crawl_without_it() {
    let body = br#"{
      "crawler_uuid": "abc", "status": "DONE", "is_finished": true, "is_success": true,
      "state": {"urls_visited": 1, "urls_extracted": 1, "duration": 1}
    }"#;
    let parsed: CrawlerStatus = serde_json::from_slice(body).expect("valid status");
    assert!(parsed.refresh.is_none());
}

#[tokio::test]
async fn crawl_refresh_rejects_bad_input_before_any_request() {
    // The host is unreachable on purpose: a request would fail with a
    // transport error, so a Config error proves nothing was sent.
    let client = Client::builder()
        .api_key("scp-live-key")
        .host("http://127.0.0.1:1")
        .build()
        .expect("client");

    let err = client.crawl_refresh_now("").await.err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));

    let empty = CrawlRefreshSettings::default();
    let err = client.crawl_refresh_settings("abc", &empty).await.err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));

    let too_short = CrawlRefreshSettings {
        interval_seconds: Some(60),
        ..Default::default()
    };
    let err = client.crawl_refresh_settings("abc", &too_short).await.err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));

    let err = client.crawl_refresh_history("", None).await.err();
    assert!(matches!(err, Some(ScrapflyError::Config(_))));

    // A well-formed patch gets past validation and fails on transport.
    let ok = CrawlRefreshSettings {
        enabled: Some(true),
        interval_seconds: Some(86400),
    };
    let err = client.crawl_refresh_settings("abc", &ok).await.err();
    assert!(!matches!(err, Some(ScrapflyError::Config(_))));
}

#[test]
fn crawler_refresh_envelope_accepts_both_shapes() {
    // The refresh endpoints answer with the state at the top level; /status
    // nests it under "refresh". Both must decode to the same thing.
    let flat_body = br#"{
      "enabled": true, "interval_seconds": 86400, "status": "SCHEDULED", "generation": 2,
      "history": [{"generation": 2, "added": 3, "updated": 7, "removed": 1, "unchanged": 404}]
    }"#;
    let flat = CrawlerRefreshState::from_envelope(flat_body).expect("flat envelope");

    let nested_body = br#"{
      "crawler_uuid": "abc",
      "refresh": {
        "enabled": true, "interval_seconds": 86400, "status": "SCHEDULED", "generation": 2,
        "history": [{"generation": 2, "added": 3, "updated": 7, "removed": 1, "unchanged": 404}]
      }
    }"#;
    let nested = CrawlerRefreshState::from_envelope(nested_body).expect("nested envelope");

    assert_eq!(flat.generation, nested.generation);
    assert_eq!(flat.interval_seconds, nested.interval_seconds);
    assert_eq!(flat.history.len(), nested.history.len());
    assert_eq!(nested.last_run().expect("one run").changed(), 11);

    // The refresh routes render a typed block that has no started_at and no
    // consecutive_failures. One type serves both routes, so both fields have
    // to survive their absence rather than reject the flat envelope.
    assert_eq!(flat.started_at, None);
    assert_eq!(flat.consecutive_failures, 0);

    // A crawl with no refresh block reads as disabled, never as enabled.
    let absent = CrawlerRefreshState::from_envelope(b"{}").expect("empty envelope");
    assert!(!absent.enabled);
    assert_eq!(absent.interval_seconds, 0);
    assert!(absent.next_run_at.is_none());
    assert!(absent.last_run().is_none());
}

/// One-shot HTTP server: accepts a single request, returns `response_body` as
/// JSON, and hands back the raw request bytes it read.
async fn capture_one_request(
    listener: tokio::net::TcpListener,
    response_body: &'static str,
) -> String {
    capture_one_request_as(listener, "application/json", response_body).await
}

/// One-shot HTTP server answering with an arbitrary `Content-Type`, so the
/// SSE path can be driven through the same accept-read-reply shape.
async fn capture_one_request_as(
    listener: tokio::net::TcpListener,
    content_type: &'static str,
    response_body: &'static str,
) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (mut socket, _) = listener.accept().await.expect("accept");
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];

    // Headers then body. reqwest sends a Content-Length, so read until the
    // body is as long as that header says rather than until EOF: the client
    // keeps the connection open waiting for our response.
    loop {
        let n = socket.read(&mut buf).await.expect("read");
        raw.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&raw).to_string();
        if let Some(headers_end) = text.find("\r\n\r\n") {
            let want = text
                .to_ascii_lowercase()
                .find("content-length:")
                .map(|i| {
                    text[i + "content-length:".len()..]
                        .lines()
                        .next()
                        .unwrap_or("0")
                        .trim()
                        .parse::<usize>()
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            if raw.len() - (headers_end + 4) >= want {
                break;
            }
        }
        if n == 0 {
            break;
        }
    }

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        content_type,
        response_body.len(),
        response_body
    );
    socket.write_all(response.as_bytes()).await.expect("write");
    socket.flush().await.expect("flush");

    String::from_utf8_lossy(&raw).to_string()
}

#[tokio::test]
async fn crawl_refresh_settings_patches_the_public_field_names() {
    // The API decodes this body with unknown fields rejected, and its public
    // keys are the ones POST /crawl takes. Sending the state block's own
    // spelling (enabled / interval_seconds) is a 400, not a no-op.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = tokio::spawn(capture_one_request(
        listener,
        r#"{"crawler_uuid":"abc","refresh":{"enabled":true,"interval_seconds":86400,"status":"SCHEDULED"}}"#,
    ));

    let client = Client::builder()
        .api_key("scp-live-key")
        .host(format!("http://127.0.0.1:{}", port))
        .build()
        .expect("client");

    let state = client
        .crawl_refresh_settings(
            "abc",
            &CrawlRefreshSettings {
                enabled: Some(true),
                interval_seconds: Some(86400),
            },
        )
        .await
        .expect("patch");

    let raw = server.await.expect("server");
    assert!(raw.starts_with("PATCH /crawl/abc/refresh"), "raw: {}", raw);
    assert!(
        raw.contains(r#""refresh":true"#),
        "public key `refresh` missing from the body: {}",
        raw
    );
    assert!(
        raw.contains(r#""refresh_interval":86400"#),
        "public key `refresh_interval` missing from the body: {}",
        raw
    );
    assert!(
        !raw.contains(r#""enabled""#) && !raw.contains(r#""interval_seconds""#),
        "state-block spelling leaked into the request body: {}",
        raw
    );

    // The answer is the nested envelope, decoded through from_envelope.
    assert!(state.enabled);
    assert_eq!(state.interval_seconds, 86400);
}

#[tokio::test]
async fn crawl_refresh_settings_disable_does_not_leak_an_interval() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = tokio::spawn(capture_one_request(
        listener,
        r#"{"crawler_uuid":"abc","refresh":{"enabled":false,"interval_seconds":86400,"status":"DISABLED"}}"#,
    ));

    let client = Client::builder()
        .api_key("scp-live-key")
        .host(format!("http://127.0.0.1:{}", port))
        .build()
        .expect("client");

    client
        .crawl_refresh_settings(
            "abc",
            &CrawlRefreshSettings {
                enabled: Some(false),
                interval_seconds: None,
            },
        )
        .await
        .expect("patch");

    let raw = server.await.expect("server");
    assert!(raw.contains(r#""refresh":false"#), "raw: {}", raw);
    assert!(
        !raw.contains("refresh_interval"),
        "interval leaked into a disable-only patch: {}",
        raw
    );
}

/// The SSE body the API relays from the engine frame for frame: sources, then
/// tokens, then one terminal `done`.
const PROMPT_STREAM_BODY: &str = concat!(
    "event: source\n",
    "data: {\"id\": 1, \"crawler_uuid\": \"0198aaaa\", \"url\": \"https://example.com/foo\", \"title\": \"Foo\", \"score\": 0.91}\n",
    "\n",
    "event: token\n",
    "data: \"Foo \"\n",
    "\n",
    "event: token\n",
    "data: \"is a product.\"\n",
    "\n",
    "event: done\n",
    "data: {\"usage\": {\"prompt_token_count\": 4312, \"candidates_token_count\": 96, \"thoughts_token_count\": 512, \"total_token_count\": 4920, \"cost\": {\"input\": 0.000431, \"output\": 0.000243}, \"model\": \"gemini-2.5-flash\"}, \"sources_used\": [1], \"sources_dropped\": 2, \"truncated\": false}\n",
    "\n",
);

#[tokio::test]
async fn crawl_prompt_done_frame_carries_thinking_tokens_and_dropped_sources() {
    use futures_util::StreamExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = tokio::spawn(capture_one_request_as(
        listener,
        "text/event-stream",
        PROMPT_STREAM_BODY,
    ));

    let client = Client::builder()
        .api_key("scp-live-key")
        .host(format!("http://127.0.0.1:{}", port))
        .build()
        .expect("client");

    let stream = client
        .crawl_prompt("abc", "what is foo?", None)
        .await
        .expect("stream opened");
    let mut stream = Box::pin(stream);

    let mut sources = Vec::new();
    let mut answer = String::new();
    let mut done: Option<CrawlerPromptDone> = None;
    while let Some(item) = stream.next().await {
        match item.expect("no error frame") {
            CrawlerPromptEvent::Source(source) => sources.push(source),
            CrawlerPromptEvent::Token(token) => answer.push_str(&token),
            CrawlerPromptEvent::Done(frame) => done = Some(frame),
        }
    }

    let raw = server.await.expect("server");
    assert!(raw.starts_with("POST /crawl/prompt"), "raw: {}", raw);

    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].url, "https://example.com/foo");
    assert_eq!(answer, "Foo is a product.");

    let done = done.expect("terminal done frame");
    assert_eq!(done.sources_used, vec![1]);
    // Retrieval found more than the context budget took, and the answer was
    // written from the subset. A caller that cannot see the drop cannot tell
    // a fully-grounded answer from a partly-grounded one.
    assert_eq!(done.sources_dropped, 2);
    // The frame reports the flat price and nothing about how the answer was
    // produced. The fixture still sends usage/tokens/cost/model because an
    // older API will; a type with no such field is what drops them.
    let rendered = format!("{:?}", done);
    assert!(!rendered.contains("gemini"), "done frame leaks the model: {}", rendered);
    assert!(!rendered.contains("token_count"), "done frame leaks tokens: {}", rendered);
}

#[test]
fn crawler_search_response_tolerates_null_lists() {
    // The API renders CrawlSearchResponse from Go slices declared without
    // omitempty, so a nil slice reaches the wire as null. Decoding that as a
    // hard type error would turn a successful search into a client-side
    // failure on the first engine build that omits one key.
    let body = br#"{
      "query": "q", "mode": "hybrid", "limit": 20, "completeness": "exact",
      "crawls": null, "skipped": null, "results": null,
      "stats": {"duration_ms": 1, "crawls_searched": 0, "candidates": 0, "gcs_gets": 0},
      "crawls_requested": 1, "crawls_searched": 0,
      "crawls_skipped_deadline": null, "crawls_failed": null,
      "theta": null, "max_ub_unsearched": null, "cursor": null
    }"#;
    let parsed: CrawlerSearchResponse = serde_json::from_slice(body).expect("valid envelope");

    assert!(parsed.crawls.is_empty());
    assert!(parsed.skipped.is_empty());
    assert!(parsed.results.is_empty());
    assert!(parsed.crawls_skipped_deadline.is_empty());
    assert!(parsed.crawls_failed.is_empty());
}

#[test]
fn crawler_refresh_timeline_tolerates_null_lists() {
    // sample_updated and sample_removed are not normalized server-side the
    // way history is, so a run that touched nothing sends null there.
    let body = br#"{
      "crawler_uuid": "abc", "status": "DONE", "is_finished": true, "is_success": true,
      "state": {"urls_visited": 1},
      "refresh": {"enabled": true, "interval_seconds": 3600, "status": "SCHEDULED",
                  "generation": 1,
                  "history": [{"generation": 1, "added": 0, "updated": 0, "removed": 0,
                               "unchanged": 5, "sample_updated": null,
                               "sample_removed": null}]}
    }"#;
    let parsed: CrawlerStatus = serde_json::from_slice(body).expect("valid status");
    let refresh = parsed.refresh.expect("refresh block present");
    let last = refresh.last_run().expect("one run recorded");
    assert!(last.sample_updated.is_empty());
    assert!(last.sample_removed.is_empty());

    let no_history = br#"{"enabled": false, "interval_seconds": 0, "status": "DISABLED",
                          "history": null}"#;
    let state = CrawlerRefreshState::from_envelope(no_history).expect("flat envelope");
    assert!(state.history.is_empty());
    assert!(state.last_run().is_none());
}

#[test]
fn crawler_prompt_done_tolerates_a_null_citation_list() {
    let frame = br#"{"usage": {"prompt_token_count": 10, "candidates_token_count": 2,
                               "total_token_count": 12, "model": "gemini-2.5-flash"},
                     "sources_used": null, "sources_dropped": 0, "truncated": false}"#;
    let done: CrawlerPromptDone = serde_json::from_slice(frame).expect("valid done frame");
    assert!(done.sources_used.is_empty());
    // An absent citation list is not evidence the model cited nothing, but it
    // must not cost the caller the rest of the frame.
    assert_eq!(done.sources_dropped, 0);
    assert!(!done.truncated);
}

/// A `crawler_updated` delivery as the webhook sender emits it.
const CRAWLER_UPDATED_WEBHOOK: &[u8] = br#"{
  "event": "crawler_updated",
  "payload": {
    "crawler_uuid": "b4867c50-318c-47cd-bfc9-bed67f24771a",
    "project": "default",
    "env": "LIVE",
    "action": "updated",
    "seed_url": "https://web-scraping.dev/products",
    "state": {"urls_visited": 5},
    "refresh": {"at": "2026-09-03T04:12:46.912430Z", "generation": 12, "added": 1,
                "updated": 2, "removed": 1, "unchanged": 2, "failed": 0,
                "duration_ms": 41870, "search_status": "READY"},
    "documents": {"updated": ["https://web-scraping.dev/product/25",
                              "https://web-scraping.dev/products"],
                  "removed": ["https://web-scraping.dev/product/9"],
                  "truncated": true},
    "links": {"status": "https://api.scrapfly.io/crawl/b4867c50/status"}
  }
}"#;

#[test]
fn crawler_webhook_decodes_the_updated_delivery() {
    let webhook = CrawlerWebhook::from_slice(CRAWLER_UPDATED_WEBHOOK).expect("known event");
    assert_eq!(webhook.event(), CrawlerWebhookEvent::CrawlerUpdated);
    assert_eq!(
        webhook.common().crawler_uuid,
        "b4867c50-318c-47cd-bfc9-bed67f24771a"
    );

    let CrawlerWebhook::CrawlerUpdated(payload) = webhook else {
        panic!("crawler_updated must decode into its own payload");
    };
    assert_eq!(payload.seed_url, "https://web-scraping.dev/products");
    assert_eq!(payload.common.state.urls_visited, 5);
    assert_eq!(payload.refresh.generation, 12);
    assert_eq!(payload.refresh.changed(), 4);
    assert_eq!(payload.documents.updated.len(), 2);
    assert_eq!(payload.documents.removed.len(), 1);
    // Truncated is the only signal that the counts outrun the lists.
    assert!(payload.documents.truncated);
    assert!(!payload.links.status.is_empty());
}

#[test]
fn crawler_webhook_decodes_the_lifecycle_deliveries() {
    // The four lifecycle events are one payload shape and differ only by
    // event name, so the variant is what the caller matches on.
    for (event, expected) in [
        ("crawler_started", CrawlerWebhookEvent::CrawlerStarted),
        ("crawler_stopped", CrawlerWebhookEvent::CrawlerStopped),
        ("crawler_cancelled", CrawlerWebhookEvent::CrawlerCancelled),
        ("crawler_finished", CrawlerWebhookEvent::CrawlerFinished),
    ] {
        let body = format!(
            r#"{{"event": "{}", "payload": {{"crawler_uuid": "abc", "project": "default",
                 "env": "TEST", "action": "finished", "seed_url": "https://example.com",
                 "state": {{"urls_visited": 3, "urls_failed": 1}},
                 "links": {{"status": "https://api.scrapfly.io/crawl/abc/status"}}}}}}"#,
            event
        );
        let webhook = CrawlerWebhook::from_slice(body.as_bytes()).expect("known event");
        assert_eq!(webhook.event(), expected);
        assert_eq!(webhook.common().state.urls_visited, 3);
        assert_eq!(webhook.common().env, "TEST");
    }
}

#[test]
fn crawler_webhook_decodes_the_search_deliveries_without_an_action() {
    // The index publishes after the crawl's own classification, so these two
    // are emitted outside the lifecycle and carry no action tag.
    let body = br#"{
      "event": "crawler_search_ready",
      "payload": {"crawler_uuid": "abc", "project": "default", "env": "LIVE",
                  "seed_url": "https://example.com",
                  "state": {"urls_visited": 5},
                  "links": {"status": "https://api.scrapfly.io/crawl/abc/status"},
                  "search": {"status": "READY", "documents": 5, "vectors": 41,
                             "embedding_model": "gemini-embedding-001",
                             "embedding_dimension": 1536}}
    }"#;
    let webhook = CrawlerWebhook::from_slice(body).expect("known event");
    assert_eq!(webhook.event(), CrawlerWebhookEvent::CrawlerSearchReady);
    assert!(webhook.common().action.is_empty());

    let CrawlerWebhook::CrawlerSearchReady(payload) = webhook else {
        panic!("crawler_search_ready must decode into the search payload");
    };
    assert!(payload.search.is_searchable());
    assert_eq!(payload.search.vectors, 41);
    // Which model we embed with is not the customer's business.
    let search_rendered = format!("{:?}", payload.search);
    assert!(!search_rendered.contains("gemini"), "status leaks the embedding model: {}", search_rendered);
}

#[test]
fn crawler_webhook_decodes_a_failed_url_with_a_null_log_link() {
    // links.log is null when the attempt died before a log was written, and
    // scrape_config carries the whole scrape surface rather than a
    // crawler-specific shape.
    let body = br#"{
      "event": "crawler_url_failed",
      "payload": {"crawler_uuid": "abc", "project": "default", "env": "LIVE",
                  "action": "failed", "state": {"urls_failed": 1},
                  "url": "https://example.com/gone",
                  "error": "ERR::SCRAPE::BAD_UPSTREAM_RESPONSE",
                  "scrape_config": {"render_js": true, "country": "us"},
                  "links": {"log": null}}
    }"#;
    let webhook = CrawlerWebhook::from_slice(body).expect("known event");
    assert_eq!(webhook.event(), CrawlerWebhookEvent::CrawlerUrlFailed);

    let CrawlerWebhook::CrawlerUrlFailed(payload) = webhook else {
        panic!("crawler_url_failed must decode into its own payload");
    };
    assert_eq!(payload.error, "ERR::SCRAPE::BAD_UPSTREAM_RESPONSE");
    assert!(payload.links.log.is_empty());
    assert_eq!(payload.scrape_config["country"], "us");
}

#[test]
fn crawler_webhook_decodes_the_url_deliveries() {
    // The markdown body carries a heading, so the literal takes the wider
    // raw-string fence: "# closes a br#"..."# on the first stored format.
    let visited = br##"{
      "event": "crawler_url_visited",
      "payload": {"crawler_uuid": "abc", "project": "default", "env": "LIVE",
                  "action": "visited", "state": {"urls_visited": 1},
                  "url": "https://example.com/a",
                  "scrape": {"status_code": 200, "country": "us",
                             "content": {"markdown": "# a", "extracted_data": null}}}
    }"##;
    let webhook = CrawlerWebhook::from_slice(visited).expect("known event");
    let CrawlerWebhook::CrawlerUrlVisited(payload) = webhook else {
        panic!("crawler_url_visited must decode into its own payload");
    };
    assert_eq!(payload.scrape.status_code, 200);
    assert_eq!(payload.scrape.content["markdown"], "# a");
    // A format the page could not produce arrives null; the caller checks
    // emptiness rather than presence.
    assert_eq!(payload.scrape.content["extracted_data"], "");

    let skipped = br#"{
      "event": "crawler_url_skipped",
      "payload": {"crawler_uuid": "abc", "state": {},
                  "urls": {"https://example.com/b": "page_limit"}}
    }"#;
    let webhook = CrawlerWebhook::from_slice(skipped).expect("known event");
    let CrawlerWebhook::CrawlerUrlSkipped(payload) = webhook else {
        panic!("crawler_url_skipped must decode into its own payload");
    };
    assert_eq!(payload.urls["https://example.com/b"], "page_limit");

    let discovered = br#"{
      "event": "crawler_url_discovered",
      "payload": {"crawler_uuid": "abc", "state": {},
                  "origin": "https://example.com/",
                  "discovered_urls": null}
    }"#;
    let webhook = CrawlerWebhook::from_slice(discovered).expect("known event");
    let CrawlerWebhook::CrawlerUrlDiscovered(payload) = webhook else {
        panic!("crawler_url_discovered must decode into its own payload");
    };
    assert_eq!(payload.origin, "https://example.com/");
    assert!(payload.discovered_urls.is_empty());
}

#[test]
fn crawler_webhook_rejects_a_body_it_cannot_type() {
    // An event this SDK does not model must not decode into a neighbouring
    // variant: the payload shapes differ, so a silent match would hand the
    // caller fields that describe a different delivery.
    let unknown = br#"{"event": "crawler_teleported", "payload": {"crawler_uuid": "abc"}}"#;
    assert!(CrawlerWebhook::from_slice(unknown).is_err());

    let no_event = br#"{"payload": {"crawler_uuid": "abc"}}"#;
    assert!(CrawlerWebhook::from_slice(no_event).is_err());
}
