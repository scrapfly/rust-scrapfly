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
    assert!(
        !rendered.contains("gemini"),
        "done frame leaks the model: {}",
        rendered
    );
    assert!(
        !rendered.contains("token_count"),
        "done frame leaks tokens: {}",
        rendered
    );
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
    assert!(
        !search_rendered.contains("gemini"),
        "status leaks the embedding model: {}",
        search_rendered
    );
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

// ---------------------------------------------------------------------------
// `unblocker` — the customer-facing name for the anti-bot feature. `asp` is
// the deprecated alias that keeps working, and stays the key on the wire.
// ---------------------------------------------------------------------------

fn scrape_pairs(cfg: &ScrapeConfig) -> std::collections::HashMap<String, String> {
    cfg.to_query_pairs()
        .expect("pairs")
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>()
}

fn crawl_body(cfg: &CrawlerConfig) -> serde_json::Value {
    serde_json::from_slice(&cfg.to_json_body().expect("serializable")).expect("valid json")
}

#[test]
fn scrape_unblocker_is_sent_under_the_asp_wire_key() {
    let cfg = ScrapeConfig::builder("https://example.com")
        .unblocker(true)
        .build()
        .expect("build");
    let pairs = scrape_pairs(&cfg);
    assert_eq!(pairs.get("asp"), Some(&"true".to_string()));
    // Emitting `unblocker` against a deployment that has not learned it on
    // this parser would silently drop a paid feature. Not this release.
    assert_eq!(pairs.get("unblocker"), None);
}

#[test]
fn scrape_asp_alias_is_still_sent_under_the_asp_wire_key() {
    let cfg = ScrapeConfig::builder("https://example.com")
        .asp(true)
        .build()
        .expect("build");
    let pairs = scrape_pairs(&cfg);
    assert_eq!(pairs.get("asp"), Some(&"true".to_string()));
    assert_eq!(pairs.get("unblocker"), None);
}

#[test]
fn scrape_build_collapses_both_names_onto_one_decision() {
    let cfg = ScrapeConfig::builder("https://example.com")
        .unblocker(true)
        .build()
        .expect("build");
    assert!(
        cfg.asp,
        "the one anti-bot slot must carry the resolved value"
    );
    assert!(cfg.unblocker_enabled());
}

#[test]
fn scrape_explicit_asp_false_vetoes_unblocker_true_in_either_order() {
    for cfg in [
        ScrapeConfig::builder("https://example.com")
            .unblocker(true)
            .asp(false)
            .build()
            .expect("build"),
        ScrapeConfig::builder("https://example.com")
            .asp(false)
            .unblocker(true)
            .build()
            .expect("build"),
    ] {
        assert!(!cfg.unblocker_enabled());
        assert_eq!(scrape_pairs(&cfg).get("asp"), None);
    }
}

#[test]
fn scrape_explicit_asp_true_wins_over_unblocker_false_in_either_order() {
    for cfg in [
        ScrapeConfig::builder("https://example.com")
            .unblocker(false)
            .asp(true)
            .build()
            .expect("build"),
        ScrapeConfig::builder("https://example.com")
            .asp(true)
            .unblocker(false)
            .build()
            .expect("build"),
    ] {
        assert!(cfg.unblocker_enabled());
        assert_eq!(scrape_pairs(&cfg).get("asp"), Some(&"true".to_string()));
    }
}

#[test]
fn scrape_unblocker_false_leaves_the_feature_off() {
    let cfg = ScrapeConfig::builder("https://example.com")
        .unblocker(false)
        .build()
        .expect("build");
    assert_eq!(scrape_pairs(&cfg).get("asp"), None);
}

#[test]
fn scrape_neither_name_leaves_the_feature_off() {
    let cfg = ScrapeConfig::builder("https://example.com")
        .build()
        .expect("build");
    assert_eq!(scrape_pairs(&cfg).get("asp"), None);
}

#[test]
fn scrape_unblocker_works_on_the_struct_literal_path() {
    // One slot, under its wire name. There is deliberately no second
    // `unblocker` field: two fields could disagree, and then neither name
    // alone could turn the feature off.
    let cfg = ScrapeConfig {
        url: "https://example.com".to_string(),
        asp: true,
        ..Default::default()
    };
    assert!(cfg.unblocker_enabled());
    assert_eq!(scrape_pairs(&cfg).get("asp"), Some(&"true".to_string()));
}

#[test]
fn scrape_disabling_after_build_turns_the_feature_off_under_either_name() {
    // A shared template built once and opted out of per request is existing
    // customer code. Whichever name the caller reaches for, the write has to
    // land on the same slot the serializer reads, or the paid feature stays
    // on and is billed against an explicit opt-out.
    let mut legacy = ScrapeConfig::builder("https://example.com")
        .asp(true)
        .build()
        .expect("build");
    legacy.asp = false;
    assert!(!legacy.unblocker_enabled());
    assert_eq!(scrape_pairs(&legacy).get("asp"), None);

    let mut current = ScrapeConfig::builder("https://example.com")
        .unblocker(true)
        .build()
        .expect("build");
    current.set_unblocker(false);
    assert!(!current.unblocker_enabled());
    assert_eq!(scrape_pairs(&current).get("asp"), None);

    // Clone-and-tweak off a template hits the same path.
    let template = ScrapeConfig::builder("https://example.com")
        .unblocker(true)
        .build()
        .expect("build");
    let mut tweaked = template.clone();
    tweaked.set_unblocker(false);
    assert_eq!(scrape_pairs(&tweaked).get("asp"), None);
    assert_eq!(
        scrape_pairs(&template).get("asp"),
        Some(&"true".to_string()),
        "the template itself must be untouched"
    );
}

#[test]
fn scrape_enabling_after_build_turns_the_feature_on_under_either_name() {
    let mut by_field = ScrapeConfig::builder("https://example.com")
        .build()
        .expect("build");
    by_field.asp = true;
    assert_eq!(
        scrape_pairs(&by_field).get("asp"),
        Some(&"true".to_string())
    );

    let mut by_setter = ScrapeConfig::builder("https://example.com")
        .build()
        .expect("build");
    by_setter.set_unblocker(true);
    assert!(by_setter.asp, "set_unblocker writes the one slot");
    assert_eq!(
        scrape_pairs(&by_setter).get("asp"),
        Some(&"true".to_string())
    );
}

#[test]
fn scrape_asp_literal_still_works_and_defaults_stay_off() {
    let on = ScrapeConfig {
        url: "https://example.com".to_string(),
        asp: true,
        ..Default::default()
    };
    assert_eq!(scrape_pairs(&on).get("asp"), Some(&"true".to_string()));

    let off = ScrapeConfig {
        url: "https://example.com".to_string(),
        ..Default::default()
    };
    assert_eq!(scrape_pairs(&off).get("asp"), None);
}

#[test]
fn crawler_unblocker_is_sent_under_the_asp_body_key() {
    let cfg = CrawlerConfig::builder("https://example.com")
        .unblocker(true)
        .build()
        .expect("build");
    let body = crawl_body(&cfg);
    assert_eq!(body["asp"], serde_json::Value::Bool(true));
    assert!(body.get("unblocker").is_none());
    assert!(cfg.unblocker_enabled());
}

#[test]
fn crawler_asp_alias_is_still_sent_under_the_asp_body_key() {
    let cfg = CrawlerConfig::builder("https://example.com")
        .asp(true)
        .build()
        .expect("build");
    let body = crawl_body(&cfg);
    assert_eq!(body["asp"], serde_json::Value::Bool(true));
    assert!(body.get("unblocker").is_none());
}

#[test]
fn crawler_explicit_asp_false_vetoes_unblocker_true_in_either_order() {
    for cfg in [
        CrawlerConfig::builder("https://example.com")
            .unblocker(true)
            .asp(false)
            .build()
            .expect("build"),
        CrawlerConfig::builder("https://example.com")
            .asp(false)
            .unblocker(true)
            .build()
            .expect("build"),
    ] {
        assert!(!cfg.unblocker_enabled());
        assert!(crawl_body(&cfg).get("asp").is_none());
    }
}

#[test]
fn crawler_explicit_asp_true_wins_over_unblocker_false_in_either_order() {
    for cfg in [
        CrawlerConfig::builder("https://example.com")
            .unblocker(false)
            .asp(true)
            .build()
            .expect("build"),
        CrawlerConfig::builder("https://example.com")
            .asp(true)
            .unblocker(false)
            .build()
            .expect("build"),
    ] {
        assert!(cfg.unblocker_enabled());
        assert_eq!(crawl_body(&cfg)["asp"], serde_json::Value::Bool(true));
    }
}

#[test]
fn crawler_unblocker_off_omits_the_key() {
    let cfg = CrawlerConfig::builder("https://example.com")
        .unblocker(false)
        .build()
        .expect("build");
    assert!(crawl_body(&cfg).get("asp").is_none());
}

#[test]
fn crawler_disabling_after_build_turns_the_feature_off_under_either_name() {
    let mut legacy = CrawlerConfig::builder("https://example.com")
        .asp(true)
        .build()
        .expect("build");
    legacy.asp = false;
    assert!(!legacy.unblocker_enabled());
    assert!(crawl_body(&legacy).get("asp").is_none());

    let mut current = CrawlerConfig::builder("https://example.com")
        .unblocker(true)
        .build()
        .expect("build");
    current.set_unblocker(false);
    assert!(!current.unblocker_enabled());
    assert!(crawl_body(&current).get("asp").is_none());
}

#[test]
fn crawler_unblocker_reaches_the_multipart_config_part() {
    let cfg = CrawlerConfig::builder_url_list(["https://example.com/a"])
        .unblocker(true)
        .build()
        .expect("build");
    let (body, _content_type) = cfg.to_multipart_body().expect("multipart");
    let body = String::from_utf8(body).expect("utf8");
    assert!(body.contains("\"asp\":true"), "body was: {}", body);
    assert!(!body.contains("unblocker"), "body was: {}", body);
}

// ---------------------------------------------------------------------------
// `unblocker` / `asp` PARITY MATRIX
//
// The guarantee a customer migrating from `asp` to `unblocker` relies on is
// not "unblocker sets the flag" — it is that the two names are
// INDISTINGUISHABLE. So the tests below compare WHOLE emitted outputs (the
// full query-pair list, the full JSON body, the full multipart body, plus the
// whole stored struct) for two configs that differ only in which name the
// caller reached for. Comparing everything is the point: a divergence in any
// other field fails here, where a targeted assertion on the one key would
// pass. The truth table then walks every combination of the two names,
// including the conflicting rows.
// ---------------------------------------------------------------------------

use scrapfly_sdk::config::crawler::CrawlerConfigBuilder;
use scrapfly_sdk::config::scrape::ScrapeConfigBuilder;
use scrapfly_sdk::{
    CrawlerContentFormat, ExtractionModel, Format, FormatOption, HttpMethod, ProxyPool,
    ScreenshotFlag,
};

/// Which of the two input names a case reaches for.
#[derive(Clone, Copy, Debug)]
enum Input {
    /// The deprecated name, kept working forever.
    Asp,
    /// The current customer-facing name.
    Unblocker,
}

/// Order in which both names are applied when a case supplies both.
#[derive(Clone, Copy, Debug)]
enum Order {
    AspFirst,
    UnblockerFirst,
}

/// One row of the truth table: what the caller supplied under each name, and
/// the outcome the decided precedence produces.
struct ParityRow {
    case: &'static str,
    asp: Option<bool>,
    unblocker: Option<bool>,
    enabled: bool,
}

/// Precedence, identical in every Scrapfly SDK: an explicitly supplied `asp`
/// wins; `unblocker` is consulted only when `asp` was not supplied; the two
/// are never OR-ed.
///
/// CROSS-SDK NOTE on the two conflict rows. Python, TypeScript and Rust all
/// resolve `asp=false, unblocker=true` to OFF, as pinned here. GO ANSWERS ON
/// for that one row: its `ASP` field is a plain `bool`, so a supplied `false`
/// is byte-identical to the zero value and cannot be honoured. That divergence
/// is documented in go/unblocker.go and go/README.md, and the Go test row that
/// pins it is named
/// GO_LANGUAGE_FORCED_EXCEPTION_documented_divergence_not_a_bug. It is the ONLY
/// cell where the four SDKs disagree; nothing here may be "fixed" to match Go.
const PARITY_MATRIX: &[ParityRow] = &[
    ParityRow {
        case: "neither name supplied",
        asp: None,
        unblocker: None,
        enabled: false,
    },
    ParityRow {
        case: "unblocker only = true",
        asp: None,
        unblocker: Some(true),
        enabled: true,
    },
    ParityRow {
        case: "unblocker only = false",
        asp: None,
        unblocker: Some(false),
        enabled: false,
    },
    ParityRow {
        case: "asp only = true",
        asp: Some(true),
        unblocker: None,
        enabled: true,
    },
    ParityRow {
        case: "asp only = false",
        asp: Some(false),
        unblocker: None,
        enabled: false,
    },
    ParityRow {
        case: "both supplied, agreeing on true",
        asp: Some(true),
        unblocker: Some(true),
        enabled: true,
    },
    ParityRow {
        case: "both supplied, agreeing on false",
        asp: Some(false),
        unblocker: Some(false),
        enabled: false,
    },
    ParityRow {
        case: "conflict: asp=false vetoes unblocker=true",
        asp: Some(false),
        unblocker: Some(true),
        enabled: false,
    },
    ParityRow {
        case: "conflict: asp=true wins over unblocker=false",
        asp: Some(true),
        unblocker: Some(false),
        enabled: true,
    },
];

/// Call order only matters when both names are supplied; the precedence is
/// order-independent, so both orders are exercised for those rows.
fn orders(row: &ParityRow) -> &'static [Order] {
    if row.asp.is_some() && row.unblocker.is_some() {
        &[Order::AspFirst, Order::UnblockerFirst]
    } else {
        &[Order::AspFirst]
    }
}

fn apply_scrape(b: ScrapeConfigBuilder, row: &ParityRow, order: Order) -> ScrapeConfigBuilder {
    match (row.asp, row.unblocker, order) {
        (Some(a), Some(u), Order::AspFirst) => b.asp(a).unblocker(u),
        (Some(a), Some(u), Order::UnblockerFirst) => b.unblocker(u).asp(a),
        (Some(a), None, _) => b.asp(a),
        (None, Some(u), _) => b.unblocker(u),
        (None, None, _) => b,
    }
}

fn apply_crawler(b: CrawlerConfigBuilder, row: &ParityRow, order: Order) -> CrawlerConfigBuilder {
    match (row.asp, row.unblocker, order) {
        (Some(a), Some(u), Order::AspFirst) => b.asp(a).unblocker(u),
        (Some(a), Some(u), Order::UnblockerFirst) => b.unblocker(u).asp(a),
        (Some(a), None, _) => b.asp(a),
        (None, Some(u), _) => b.unblocker(u),
        (None, None, _) => b,
    }
}

/// Emitted query pairs in emission order — the whole output, not a lookup.
/// `to_query_pairs` pushes in a fixed sequence and the map-backed fields are
/// `BTreeMap`s, so the order is deterministic and part of the contract.
fn scrape_pairs_ordered(cfg: &ScrapeConfig) -> Vec<(String, String)> {
    cfg.to_query_pairs().expect("pairs")
}

/// The literal query string the request would carry, built the way
/// `Client::build_url` builds it (percent-encoding included).
fn scrape_query_string(cfg: &ScrapeConfig) -> String {
    let mut u = url::Url::parse("https://api.scrapfly.io/scrape").expect("base url");
    {
        let mut q = u.query_pairs_mut();
        for (k, v) in scrape_pairs_ordered(cfg) {
            q.append_pair(&k, &v);
        }
    }
    u.query().unwrap_or_default().to_string()
}

/// A whole-output comparison is only as strong as the output has fields to
/// diverge in. If a fixture is ever trimmed, or a serializer regresses to
/// emitting almost nothing, every equality below would still pass while its
/// name claimed it compared "the whole output". This floor stops that; the Go
/// SDK's matrix carries the same guard (`len(legacy) < 25`).
const MIN_LOADED_PAIRS: usize = 25;

fn assert_not_vacuous<T>(emitted: &[T], what: &str) {
    assert!(
        emitted.len() >= MIN_LOADED_PAIRS,
        "{what} emits only {} entries; a whole-output comparison over that proves \
         almost nothing. Expected at least {MIN_LOADED_PAIRS}.",
        emitted.len()
    );
}

/// A config exercising a wide slice of the surface, so that whole-output
/// equality proves the two input names diverge in NOTHING, not just in the
/// anti-bot key.
fn loaded_scrape(input: Input, v: bool) -> ScrapeConfig {
    let b = loaded_scrape_builder();
    let b = match input {
        Input::Asp => b.asp(v),
        Input::Unblocker => b.unblocker(v),
    };
    b.build().expect("build")
}

/// The loaded scrape surface WITHOUT the anti-bot toggle, so the truth table
/// can apply both names to it in either order.
fn loaded_scrape_builder() -> ScrapeConfigBuilder {
    ScrapeConfig::builder("https://example.com/parity")
        .method(HttpMethod::Post)
        .body("payload=1")
        .header("X-Test", "value")
        .cookie("sid", "abc")
        .country("US")
        .proxy_pool(ProxyPool::PublicResidentialPool)
        .render_js(true)
        .wait_for_selector(".ready")
        .rendering_wait(250)
        .rendering_stage("domcontentloaded")
        .geolocation("48.85,2.35")
        .auto_scroll(true)
        .js("window.x = 1;")
        .js_scenario(serde_json::json!([{"click": {"selector": ".more"}}]))
        .screenshot("hero", "fullpage")
        .screenshot_flag(ScreenshotFlag::HighQuality)
        .cache(true)
        .cache_ttl(60)
        .cache_clear(true)
        .timeout(30000)
        .cost_budget(25)
        .retry(false)
        .session("parity")
        .session_sticky_proxy(false)
        .tag("a")
        .tag("b")
        .webhook("hook")
        .debug(true)
        .ssl(true)
        .dns(true)
        .correlation_id("cid-1")
        .format(Format::Markdown)
        .format_option(FormatOption::OnlyContent)
        .extraction_model(ExtractionModel::Product)
        .os("linux")
        .lang("fr")
        .lang("en")
        .browser_brand("chrome")
        .proxified_response()
}

fn loaded_crawler(input: Input, v: bool) -> CrawlerConfig {
    let b = loaded_crawler_builder();
    let b = match input {
        Input::Asp => b.asp(v),
        Input::Unblocker => b.unblocker(v),
    };
    b.build().expect("build")
}

/// The loaded crawler surface WITHOUT the anti-bot toggle.
fn loaded_crawler_builder() -> CrawlerConfigBuilder {
    CrawlerConfig::builder("https://example.com/parity")
        .page_limit(10)
        .max_depth(3)
        .max_duration(600)
        .max_api_credit(100)
        .exclude_paths(vec!["/private".to_string()])
        .ignore_base_path_restriction(true)
        .follow_external_links(true)
        .allowed_external_domains(vec!["cdn.example.com".to_string()])
        .follow_internal_subdomains(false)
        .allowed_internal_subdomains(vec!["shop.example.com".to_string()])
        .header("X-Test", "value")
        .delay(100)
        .user_agent("parity/1.0")
        .max_concurrency(4)
        .rendering_delay(500)
        .use_sitemaps(true)
        .ignore_no_follow(true)
        .respect_robots_txt(false)
        .cache(true)
        .cache_ttl(60)
        .cache_clear(true)
        .content_format(CrawlerContentFormat::Markdown)
        .extraction_rules(serde_json::json!({"title": "h1"}))
        .search(true)
        .refresh(true)
        .refresh_interval(REFRESH_MIN_INTERVAL)
        .proxy_pool("public_residential_pool")
        .country("us")
        .webhook_name("hook")
        .webhook_event(CrawlerWebhookEvent::CrawlerStarted)
}

fn loaded_crawler_url_list(input: Input, v: bool) -> CrawlerConfig {
    let b = CrawlerConfig::builder_url_list(["https://example.com/a", "https://example.com/b"])
        .page_limit(10)
        .header("X-Test", "value")
        .content_format(CrawlerContentFormat::Markdown)
        .country("us");
    let b = match input {
        Input::Asp => b.asp(v),
        Input::Unblocker => b.unblocker(v),
    };
    b.build().expect("build")
}

/// Multipart body with the random boundary folded to a fixed token, so two
/// bodies from two separate calls are comparable as whole byte strings.
fn multipart_normalized(cfg: &CrawlerConfig) -> (String, String) {
    let (body, content_type) = cfg.to_multipart_body().expect("multipart");
    let boundary = content_type
        .rsplit("boundary=")
        .next()
        .expect("boundary in content-type")
        .to_string();
    let body = String::from_utf8(body).expect("utf8 body");
    (
        body.replace(&boundary, "BOUNDARY"),
        content_type.replace(&boundary, "BOUNDARY"),
    )
}

// --- 1. EQUIVALENCE: whole emitted output, both names, both values ---------

#[test]
fn scrape_asp_and_unblocker_emit_an_identical_whole_output() {
    for v in [true, false] {
        let by_asp = loaded_scrape(Input::Asp, v);
        let by_unblocker = loaded_scrape(Input::Unblocker, v);

        assert_not_vacuous(&scrape_pairs_ordered(&by_asp), "loaded scrape query pairs");

        // The whole pair list, in emission order — not a lookup of one key.
        assert_eq!(
            scrape_pairs_ordered(&by_asp),
            scrape_pairs_ordered(&by_unblocker),
            ".asp({v}) and .unblocker({v}) must emit the same query parameters, all of them"
        );
        // The literal query string the request would carry.
        assert_eq!(
            scrape_query_string(&by_asp),
            scrape_query_string(&by_unblocker),
            ".asp({v}) and .unblocker({v}) must produce the same request URL"
        );
        // The whole stored struct: every field, not only the anti-bot slot.
        assert_eq!(
            format!("{:?}", by_asp),
            format!("{:?}", by_unblocker),
            ".asp({v}) and .unblocker({v}) must store the same config"
        );
    }
}

#[test]
fn crawler_asp_and_unblocker_emit_an_identical_whole_body() {
    for v in [true, false] {
        let by_asp = loaded_crawler(Input::Asp, v);
        let by_unblocker = loaded_crawler(Input::Unblocker, v);

        let keys: Vec<String> = serde_json::to_value(&by_asp)
            .expect("serialize")
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect();
        assert_not_vacuous(&keys, "loaded crawl body");

        // Byte-identical POST /crawl body, every key.
        assert_eq!(
            String::from_utf8(by_asp.to_json_body().expect("body")).expect("utf8"),
            String::from_utf8(by_unblocker.to_json_body().expect("body")).expect("utf8"),
            ".asp({v}) and .unblocker({v}) must serialize the same crawl body"
        );
        assert_eq!(
            format!("{:?}", by_asp),
            format!("{:?}", by_unblocker),
            ".asp({v}) and .unblocker({v}) must store the same config"
        );
    }
}

#[test]
fn crawler_url_list_asp_and_unblocker_emit_an_identical_multipart_body() {
    // The url_list crawl posts multipart, a second emitted form of the same
    // config; the config part must not diverge between the two names either.
    for v in [true, false] {
        let by_asp = multipart_normalized(&loaded_crawler_url_list(Input::Asp, v));
        let by_unblocker = multipart_normalized(&loaded_crawler_url_list(Input::Unblocker, v));
        assert_eq!(
            by_asp, by_unblocker,
            ".asp({v}) and .unblocker({v}) must produce the same multipart body and content type"
        );
    }
}

// --- 2. THE FULL TRUTH TABLE ----------------------------------------------

#[test]
fn scrape_unblocker_truth_table_resolves_and_emits_as_decided() {
    for row in PARITY_MATRIX {
        for &order in orders(row) {
            let cfg = apply_scrape(
                ScrapeConfig::builder("https://example.com/matrix"),
                row,
                order,
            );
            let cfg = cfg.build().expect("build");
            let ctx = format!("{} [{:?}]", row.case, order);

            // Resolved outcome, read under both names.
            assert_eq!(
                cfg.unblocker_enabled(),
                row.enabled,
                "{ctx}: unblocker_enabled()"
            );
            assert_eq!(cfg.asp, row.enabled, "{ctx}: the `asp` slot");

            // Emitted wire key.
            let pairs = scrape_pairs_ordered(&cfg);
            let emitted = pairs
                .iter()
                .find(|(k, _)| k == "asp")
                .map(|(_, v)| v.as_str());
            assert_eq!(
                emitted,
                row.enabled.then_some("true"),
                "{ctx}: emitted `asp` query parameter"
            );
        }
    }
}

#[test]
fn crawler_unblocker_truth_table_resolves_and_emits_as_decided() {
    for row in PARITY_MATRIX {
        for &order in orders(row) {
            let cfg = apply_crawler(
                CrawlerConfig::builder("https://example.com/matrix"),
                row,
                order,
            );
            let cfg = cfg.build().expect("build");
            let ctx = format!("{} [{:?}]", row.case, order);

            assert_eq!(
                cfg.unblocker_enabled(),
                row.enabled,
                "{ctx}: unblocker_enabled()"
            );
            assert_eq!(cfg.asp, row.enabled, "{ctx}: the `asp` slot");

            let body = crawl_body(&cfg);
            let want = row.enabled.then_some(serde_json::Value::Bool(true));
            assert_eq!(
                body.as_object().expect("object body").get("asp"),
                want.as_ref(),
                "{ctx}: emitted `asp` body key"
            );
        }
    }
}

#[test]
fn crawler_unblocker_truth_table_reaches_the_multipart_and_remote_list_paths() {
    // `builder`, `builder_url_list` and `builder_remote_url_list` each
    // initialize the same three suppliedness flags, and `to_multipart_body` is a
    // SECOND serializer of the resolved value. Driving the whole matrix (not
    // just the two equivalence pairs) through both closes the gap where a
    // conflict row could resolve one way in the JSON body and another in the
    // multipart config part.
    for row in PARITY_MATRIX {
        for &order in orders(row) {
            let ctx = format!("{} [{:?}]", row.case, order);

            // In-memory url_list -> multipart.
            let multipart_cfg = apply_crawler(
                CrawlerConfig::builder_url_list(["https://example.com/a", "https://example.com/b"]),
                row,
                order,
            )
            .build()
            .expect("build");
            assert_eq!(
                multipart_cfg.unblocker_enabled(),
                row.enabled,
                "{ctx}: builder_url_list resolution"
            );
            let (multipart, _) = multipart_normalized(&multipart_cfg);
            assert_eq!(
                multipart.contains("\"asp\":true"),
                row.enabled,
                "{ctx}: multipart config part must carry `asp` exactly when the row is on: {multipart}"
            );
            assert!(
                !multipart.contains("unblocker"),
                "{ctx}: the new name reached the multipart body: {multipart}"
            );

            // remote_url_list -> JSON body, third constructor.
            let remote_cfg = apply_crawler(
                CrawlerConfig::builder_remote_url_list("https://example.com/urls.txt"),
                row,
                order,
            )
            .build()
            .expect("build");
            assert_eq!(
                remote_cfg.unblocker_enabled(),
                row.enabled,
                "{ctx}: builder_remote_url_list resolution"
            );
            let want = row.enabled.then_some(serde_json::Value::Bool(true));
            assert_eq!(
                crawl_body(&remote_cfg)
                    .as_object()
                    .expect("object body")
                    .get("asp"),
                want.as_ref(),
                "{ctx}: remote_url_list body key"
            );
        }
    }
}

#[test]
fn unblocker_truth_table_holds_on_the_loaded_configs_too() {
    // The tables above build from a bare `builder(url)`. A defect that only
    // fires when a conflicting name pair coexists with other populated options
    // — a validation branch, a mutually-exclusive-option check, an ordering
    // effect — would be invisible there. The Go matrix seeds every row from its
    // loaded base for exactly this reason; this is the Rust equivalent.
    for row in PARITY_MATRIX {
        for &order in orders(row) {
            let ctx = format!("{} [{:?}]", row.case, order);

            let scrape = apply_scrape(loaded_scrape_builder(), row, order)
                .build()
                .expect("build");
            assert_eq!(scrape.unblocker_enabled(), row.enabled, "{ctx}: loaded scrape");
            let pairs = scrape_pairs_ordered(&scrape);
            assert_not_vacuous(&pairs, "loaded scrape query pairs");
            assert_eq!(
                pairs
                    .iter()
                    .find(|(k, _)| k == "asp")
                    .map(|(_, v)| v.as_str()),
                row.enabled.then_some("true"),
                "{ctx}: loaded scrape emitted `asp`"
            );

            let crawler = apply_crawler(loaded_crawler_builder(), row, order)
                .build()
                .expect("build");
            assert_eq!(crawler.unblocker_enabled(), row.enabled, "{ctx}: loaded crawler");
            let want = row.enabled.then_some(serde_json::Value::Bool(true));
            assert_eq!(
                crawl_body(&crawler)
                    .as_object()
                    .expect("object body")
                    .get("asp"),
                want.as_ref(),
                "{ctx}: loaded crawl body"
            );
        }
    }
}

#[test]
fn unblocker_matrix_rows_with_the_same_outcome_are_indistinguishable() {
    // Stronger than the per-row assertions: every path that resolves to the
    // same outcome must produce the SAME whole output, whichever name (or
    // both, in either order) got the caller there.
    for enabled in [true, false] {
        let mut scrape_outputs: Vec<(String, Vec<(String, String)>)> = Vec::new();
        let mut crawl_outputs: Vec<(String, String)> = Vec::new();
        for row in PARITY_MATRIX.iter().filter(|r| r.enabled == enabled) {
            for &order in orders(row) {
                let ctx = format!("{} [{:?}]", row.case, order);
                let scrape = apply_scrape(
                    ScrapeConfig::builder("https://example.com/matrix"),
                    row,
                    order,
                )
                .build()
                .expect("build");
                scrape_outputs.push((ctx.clone(), scrape_pairs_ordered(&scrape)));

                let crawl = apply_crawler(
                    CrawlerConfig::builder("https://example.com/matrix"),
                    row,
                    order,
                )
                .build()
                .expect("build");
                crawl_outputs.push((
                    ctx,
                    String::from_utf8(crawl.to_json_body().expect("body")).expect("utf8"),
                ));
            }
        }

        let (first_ctx, first) = scrape_outputs.first().cloned().expect("rows");
        for (ctx, out) in &scrape_outputs {
            assert_eq!(
                out, &first,
                "scrape output for {ctx} differs from {first_ctx} although both resolve to enabled={enabled}"
            );
        }
        let (first_ctx, first) = crawl_outputs.first().cloned().expect("rows");
        for (ctx, out) in &crawl_outputs {
            assert_eq!(
                out, &first,
                "crawl body for {ctx} differs from {first_ctx} although both resolve to enabled={enabled}"
            );
        }
    }
}

// --- 3. WIRE KEY -----------------------------------------------------------

#[test]
fn unblocker_never_reaches_the_wire_under_either_input_name() {
    // Emitting the new name, or both names, against a deployment that has not
    // learned it on this parser silently drops a paid feature: the scrape is
    // billed and returns a blocked page. `asp` is the only key that ships.
    for row in PARITY_MATRIX {
        for &order in orders(row) {
            let ctx = format!("{} [{:?}]", row.case, order);

            let scrape = apply_scrape(
                ScrapeConfig::builder("https://example.com/matrix"),
                row,
                order,
            )
            .build()
            .expect("build");
            for (k, _) in scrape_pairs_ordered(&scrape) {
                assert!(
                    !k.contains("unblocker"),
                    "{ctx}: query parameter {k:?} carries the new name"
                );
            }
            assert!(
                !scrape_query_string(&scrape).contains("unblocker"),
                "{ctx}: request URL carries the new name"
            );

            let crawl = apply_crawler(
                CrawlerConfig::builder("https://example.com/matrix"),
                row,
                order,
            )
            .build()
            .expect("build");
            let body = String::from_utf8(crawl.to_json_body().expect("body")).expect("utf8");
            assert!(
                !body.contains("unblocker"),
                "{ctx}: crawl body carries the new name: {body}"
            );
        }
    }

    // The loaded configs and the multipart form too, not just the bare ones.
    for v in [true, false] {
        for input in [Input::Asp, Input::Unblocker] {
            assert!(!scrape_query_string(&loaded_scrape(input, v)).contains("unblocker"));
            let body = String::from_utf8(loaded_crawler(input, v).to_json_body().expect("body"))
                .expect("utf8");
            assert!(!body.contains("unblocker"));
            let (multipart, _) = multipart_normalized(&loaded_crawler_url_list(input, v));
            assert!(!multipart.contains("unblocker"), "{multipart}");
        }
    }
}

// --- 4. POST-CONSTRUCTION PARITY ------------------------------------------

#[test]
fn scrape_post_construction_names_agree_in_both_directions() {
    for v in [true, false] {
        // Write under the old name, read under the new one.
        let mut by_field = ScrapeConfig::builder("https://example.com/mut")
            .build()
            .expect("build");
        by_field.asp = v;
        assert_eq!(
            by_field.unblocker_enabled(),
            v,
            "unblocker_enabled() must report what the `asp` field holds"
        );

        // Write under the new name, read under the old one.
        let mut by_setter = ScrapeConfig::builder("https://example.com/mut")
            .build()
            .expect("build");
        by_setter.set_unblocker(v);
        assert_eq!(
            by_setter.asp, v,
            "set_unblocker() must write the slot the `asp` field reads"
        );

        // And both mutations put the same whole thing on the wire.
        assert_eq!(
            scrape_pairs_ordered(&by_field),
            scrape_pairs_ordered(&by_setter)
        );
        assert_eq!(
            scrape_pairs_ordered(&by_field)
                .iter()
                .find(|(k, _)| k == "asp")
                .map(|(_, val)| val.as_str()),
            v.then_some("true")
        );
    }
}

#[test]
fn scrape_mutating_after_build_changes_the_wire_under_either_name() {
    // A template built once and opted in or out of per request is existing
    // customer code; the write has to reach the serializer under both names.
    for start in [true, false] {
        let flipped = !start;

        let mut by_field = ScrapeConfig::builder("https://example.com/mut")
            .unblocker(start)
            .build()
            .expect("build");
        by_field.asp = flipped;

        let mut by_setter = ScrapeConfig::builder("https://example.com/mut")
            .asp(start)
            .build()
            .expect("build");
        by_setter.set_unblocker(flipped);

        for cfg in [&by_field, &by_setter] {
            assert_eq!(cfg.unblocker_enabled(), flipped);
            assert_eq!(
                scrape_pairs_ordered(cfg)
                    .iter()
                    .find(|(k, _)| k == "asp")
                    .map(|(_, val)| val.as_str()),
                flipped.then_some("true"),
                "the wire must follow the post-build mutation"
            );
        }
        assert_eq!(
            scrape_pairs_ordered(&by_field),
            scrape_pairs_ordered(&by_setter),
            "mutating by field and by setter must leave identical output"
        );
    }
}

#[test]
fn crawler_post_construction_names_agree_in_both_directions() {
    for v in [true, false] {
        let mut by_field = CrawlerConfig::builder("https://example.com/mut")
            .build()
            .expect("build");
        by_field.asp = v;
        assert_eq!(by_field.unblocker_enabled(), v);

        let mut by_setter = CrawlerConfig::builder("https://example.com/mut")
            .build()
            .expect("build");
        by_setter.set_unblocker(v);
        assert_eq!(by_setter.asp, v);

        assert_eq!(
            String::from_utf8(by_field.to_json_body().expect("body")).expect("utf8"),
            String::from_utf8(by_setter.to_json_body().expect("body")).expect("utf8")
        );
        let want = v.then_some(serde_json::Value::Bool(true));
        assert_eq!(
            crawl_body(&by_field)
                .as_object()
                .expect("object body")
                .get("asp"),
            want.as_ref()
        );
    }
}

#[test]
fn crawler_mutating_after_build_changes_the_wire_under_either_name() {
    for start in [true, false] {
        let flipped = !start;

        let mut by_field = CrawlerConfig::builder("https://example.com/mut")
            .unblocker(start)
            .build()
            .expect("build");
        by_field.asp = flipped;

        let mut by_setter = CrawlerConfig::builder("https://example.com/mut")
            .asp(start)
            .build()
            .expect("build");
        by_setter.set_unblocker(flipped);

        let want = flipped.then_some(serde_json::Value::Bool(true));
        for cfg in [&by_field, &by_setter] {
            assert_eq!(cfg.unblocker_enabled(), flipped);
            assert_eq!(
                crawl_body(cfg).as_object().expect("object body").get("asp"),
                want.as_ref(),
                "the wire must follow the post-build mutation"
            );
        }
        assert_eq!(
            String::from_utf8(by_field.to_json_body().expect("body")).expect("utf8"),
            String::from_utf8(by_setter.to_json_body().expect("body")).expect("utf8")
        );
    }
}

// --- The struct-literal path ----------------------------------------------

#[test]
fn scrape_struct_literal_asp_matches_the_unblocker_builder() {
    // Customers who fill the struct directly write `asp`, the one storage
    // slot. That path must land on exactly the output the current name's
    // builder produces. `session_sticky_proxy: true` mirrors the builder's
    // own default (a session keeps one exit IP unless the caller opts out).
    for v in [true, false] {
        let literal = ScrapeConfig {
            url: "https://example.com/literal".to_string(),
            session_sticky_proxy: true,
            asp: v,
            ..Default::default()
        };
        let built = ScrapeConfig::builder("https://example.com/literal")
            .unblocker(v)
            .build()
            .expect("build");
        assert_eq!(literal.unblocker_enabled(), built.unblocker_enabled());
        assert_eq!(
            scrape_pairs_ordered(&literal),
            scrape_pairs_ordered(&built),
            "the struct-literal path must emit what .unblocker({v}) emits"
        );
        assert_eq!(format!("{:?}", literal), format!("{:?}", built));
    }
}

#[test]
fn crawler_struct_literal_asp_matches_the_unblocker_builder() {
    for v in [true, false] {
        let literal = CrawlerConfig {
            url: "https://example.com/literal".to_string(),
            asp: v,
            ..Default::default()
        };
        let built = CrawlerConfig::builder("https://example.com/literal")
            .unblocker(v)
            .build()
            .expect("build");
        assert_eq!(literal.unblocker_enabled(), built.unblocker_enabled());
        assert_eq!(
            String::from_utf8(literal.to_json_body().expect("body")).expect("utf8"),
            String::from_utf8(built.to_json_body().expect("body")).expect("utf8"),
            "the struct-literal path must serialize what .unblocker({v}) serializes"
        );
        assert_eq!(format!("{:?}", literal), format!("{:?}", built));
    }
}

#[test]
fn struct_literal_has_one_slot_so_supplied_false_and_unset_coincide() {
    // NOT a divergence, and deliberately not named like one: on the
    // struct-literal path there is a single `asp` slot, so "supplied false" and
    // "never supplied" are the same config and both resolve to off — exactly
    // what Python, TypeScript and Go do for those rows. Rust answers the whole
    // matrix the way the other SDKs do; the ONE cell where any SDK differs is
    // Go's `ASP: false, Unblocker: BoolPtr(true)`, whose test row is named
    // GO_LANGUAGE_FORCED_EXCEPTION_documented_divergence_not_a_bug. Nothing
    // here belongs in that category.
    //
    // The builders do NOT collapse the two — they track suppliedness, which is
    // why `.asp(false)` can veto `.unblocker(true)` — so no builder row is
    // lost; the coincidence is confined to the struct-literal and
    // post-build-field path, where there is one slot and nothing to disagree
    // with.
    let supplied_false = ScrapeConfig {
        url: "https://example.com/literal".to_string(),
        session_sticky_proxy: true,
        asp: false,
        ..Default::default()
    };
    let never_supplied = ScrapeConfig {
        url: "https://example.com/literal".to_string(),
        session_sticky_proxy: true,
        ..Default::default()
    };
    assert_eq!(
        scrape_pairs_ordered(&supplied_false),
        scrape_pairs_ordered(&never_supplied),
        "on the literal path the two are the same config, and both leave the feature off"
    );
    assert!(!supplied_false.unblocker_enabled());

    // The builder, by contrast, keeps them apart: a supplied false vetoes.
    let vetoed = ScrapeConfig::builder("https://example.com/literal")
        .asp(false)
        .unblocker(true)
        .build()
        .expect("build");
    let deferred = ScrapeConfig::builder("https://example.com/literal")
        .unblocker(true)
        .build()
        .expect("build");
    assert!(!vetoed.unblocker_enabled(), "supplied asp=false wins");
    assert!(deferred.unblocker_enabled(), "unsupplied asp defers");
}

// --- 8. CLIENT LAYER -------------------------------------------------------
//
// Every assertion above stops at the config serializer. Nothing pinned that the
// key survives the CLIENT: a param whitelist, a rename shim or a
// re-serialization between `to_query_pairs` and `build_url` would leave the
// whole matrix green while the wire lost the flag. These drive a real `Client`
// against a one-shot local server and read the request line it actually sent.

/// Runs one scrape against a local server and returns the raw request text.
async fn captured_scrape_request(cfg: &ScrapeConfig) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = tokio::spawn(capture_one_request(
        listener,
        r#"{"result":{"status":"DONE","success":true,"status_code":200,"content":"","format":"text"}}"#,
    ));

    let client = Client::builder()
        .api_key("scp-live-key")
        .host(format!("http://127.0.0.1:{}", port))
        .build()
        .expect("client");

    // The assertion is about the request the client SENT; how the stub envelope
    // decodes is beside the point, so the result is deliberately not unwrapped.
    let _ = client.scrape(cfg).await;

    server.await.expect("server")
}

/// The query string of a captured `GET /scrape?...` request line.
fn captured_query(raw: &str) -> String {
    let line = raw.lines().next().expect("request line");
    let target = line.split_whitespace().nth(1).expect("request target");
    target
        .split_once('?')
        .map(|(_, q)| q.to_string())
        .unwrap_or_default()
}

#[tokio::test]
async fn client_sends_the_asp_wire_key_under_either_input_name() {
    for input in [Input::Asp, Input::Unblocker] {
        let raw = captured_scrape_request(&loaded_scrape(input, true)).await;
        let query = captured_query(&raw);
        assert!(
            query.contains("asp=true"),
            "{input:?}: `asp` missing from the sent request line: {query}"
        );
        assert!(
            !query.contains("unblocker"),
            "{input:?}: the new name reached the wire: {query}"
        );
    }
}

#[tokio::test]
async fn client_omits_the_key_when_the_feature_is_off() {
    for input in [Input::Asp, Input::Unblocker] {
        let query = captured_query(&captured_scrape_request(&loaded_scrape(input, false)).await);
        assert!(
            !query.contains("asp="),
            "{input:?}: `asp` emitted while the feature is off: {query}"
        );
        assert!(!query.contains("unblocker"), "{input:?}: {query}");
    }
}

#[tokio::test]
async fn client_sends_an_identical_query_string_under_both_names() {
    // The WHOLE query the client put on the wire, not just the one key.
    let by_asp = captured_query(&captured_scrape_request(&loaded_scrape(Input::Asp, true)).await);
    let by_unblocker =
        captured_query(&captured_scrape_request(&loaded_scrape(Input::Unblocker, true)).await);

    assert!(
        by_asp.split('&').count() >= MIN_LOADED_PAIRS,
        "the client sent only {} params; the comparison would be near-vacuous: {by_asp}",
        by_asp.split('&').count()
    );
    assert_eq!(
        by_asp, by_unblocker,
        "the client must send an identical query string for the two names"
    );
    assert!(by_asp.contains("asp=true"));
}

// --- 9. ERROR SURFACE ------------------------------------------------------

#[test]
fn unblocker_named_predicate_matches_the_asp_bypass_variant() {
    // A variant cannot be aliased in Rust, so the rename reaches the error
    // surface as a predicate. Go exposes `ErrUnblockerBypassFailed` and the
    // TypeScript / Python SDKs `ScrapflyUnblockerError` for the same failure; a
    // customer who renamed the config parameter needs an unblocker-named way to
    // test for it here too.
    let asp_failure = scrapfly_sdk::error::from_response(
        422,
        br#"{"code":"ERR::ASP::SHIELD_PROTECTION_FAILED","message":"shield failed"}"#,
        0,
        false,
    );
    assert!(matches!(
        asp_failure,
        scrapfly_sdk::error::ScrapflyError::AspBypassFailed(_)
    ));
    assert!(
        asp_failure.is_unblocker_failure(),
        "is_unblocker_failure() must be true for exactly the AspBypassFailed variant"
    );

    // Negative control: a different failure must NOT answer true, so the
    // predicate is evidence and not a constant.
    let proxy_failure = scrapfly_sdk::error::from_response(
        422,
        br#"{"code":"ERR::PROXY::UNAVAILABLE","message":"no proxy"}"#,
        0,
        false,
    );
    assert!(!proxy_failure.is_unblocker_failure());
}
