//! Live integration tests — run against a real Scrapfly API.
//!
//! The unit suite (`tests/unit.rs`) proves the SDK *serializes* `unblocker`
//! and `asp` onto the same wire key. It stops at the serializer. This file
//! closes the other half: it drives both names through the real client to a
//! real API and asserts the API's own echo of the parsed config agrees.
//!
//! # What the SDK legs prove, and what they do not
//!
//! Be precise, because the obvious reading is wrong. `ScrapeConfigBuilder`
//! collapses `unblocker` into the single stored `asp` slot BEFORE serializing,
//! and the wire key is frozen at `asp`. So legs 1 and 2 put a BYTE-IDENTICAL
//! request on the wire, as do legs 3 and 4. The SDK matrix proves the client
//! folds both names onto one key and that the API honours that key. It does
//! NOT prove the API still honours the `unblocker` SPELLING, because no SDK
//! leg ever sends it — every leg below logs `wire[unblocker]=None`.
//!
//! That spelling is a separate code path in the API itself: it reads the `asp`
//! query parameter first and falls back to `unblocker` when `asp` is absent.
//!
//! It is what a customer on a raw HTTP client depends on, and the API silently
//! ignores query params it does not recognise, so deleting it would make
//! `unblocker=true` return an UNPROTECTED, billed scrape. Leg 5 is the only leg
//! that covers it: it bypasses the SDK's fold and puts `unblocker=true` on the
//! wire with no `asp` key. Delete that server-side fallback and leg 5 — and
//! only leg 5 — goes red.
//!
//! # Gating
//!
//! Both `SCRAPFLY_API_KEY` and `SCRAPFLY_API_HOST` must be set. With either
//! missing the test prints a skip line and **returns early, passing**, so
//! `cargo test` on a machine with no credentials stays green.
//!
//! That default has a failure mode worth naming: libtest captures stdout, so
//! without `--nocapture` a skipped run prints `test ... ok` and is
//! indistinguishable from a run that exercised the whole live matrix. A CI job
//! whose key secret expires would go on reporting green forever. So CI must
//! set `SCRAPFLY_REQUIRE_INTEGRATION=1`, which turns missing credentials into
//! a hard failure. Laptops leave it unset and stay green offline.
//!
//! # TLS
//!
//! Verification is kept ON. An endpoint whose certificate the system store
//! cannot verify is served by pointing `SCRAPFLY_CA_BUNDLE` at its root, which
//! is handed to `reqwest` as an extra trust anchor. `SCRAPFLY_INSECURE_TLS=1`
//! exists as an explicit last-resort escape hatch and prints a loud warning
//! when used; it is not the default and is not needed on the dev cluster.
//! Nothing here adds a dependency or a crate feature — `reqwest` is already a
//! direct dependency of the SDK, and `Client::builder()` already exposes both
//! `http_client()` and `danger_accept_invalid_certs()`.
//!
//! # Cost, measured rather than assumed
//!
//! Three legs request the bypass. On this shieldless target that costs the
//! same as the disabled legs: `context.cost` came back
//! `{total: 1, details: [PROXY_DATACENTER_NETWORK]}` for an `unblocker=true`
//! scrape of httpbin.dev, with no anti-bot line item — the surcharge applies
//! when a shield is actually engaged, which httpbin.dev does not do. The fixed
//! leg count is therefore discipline about not hammering a live account, not a
//! credit constraint. The no-retry rule still matters for a different reason:
//! a retry would hide a genuine alias failure behind a second attempt.
//!
//! `SCRAPFLY_SKIP_BILLABLE=1` runs only the two cheap legs, for debugging the
//! harness. It aborts with an explicit message instead of passing, so a
//! harness-only run can never read as a green alias verdict.
//!
//! Run with:
//! ```text
//! SCRAPFLY_API_KEY=scp-live-... \
//!   cargo test --offline --test integration -- --nocapture
//! ```

use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::HeaderMap;
use reqwest::{Method, Url};
use scrapfly_sdk::{Client, OnRequest, ScrapeConfig, ScrapflyError};

/// Small, stable, cheap. Not anti-bot protected, so a leg that fails fails
/// because of the alias plumbing and not because a shield won.
const TARGET_URL: &str = "https://httpbin.dev/html";

/// Spacing between legs. The legs are already strictly sequential — each
/// `scrape()` is awaited to completion before the next starts — but the dev
/// project carries a user throttle rule on the target host (SLIDING_WINDOW,
/// `max_rate` 5, `max_concurrency` 5, reported under `context.throttler`), and
/// the slot is released slightly after the response is handed back. Five
/// back-to-back scrapes of the same host do not fit; at 5s they did not fit
/// reliably either. This is a gap, not a retry: no assertion is softened.
const LEG_SPACING: Duration = Duration::from_secs(12);

/// The single wait granted to a throttle-refused leg — long enough to drain
/// the host throttler's sliding window rather than land in the same one again.
const THROTTLE_BACKOFF: Duration = Duration::from_secs(60);

struct Creds {
    key: String,
    host: String,
}

/// Read the two gating env vars. `None` means "skip", never "fail" — unless
/// `SCRAPFLY_REQUIRE_INTEGRATION=1` says this environment was supposed to have
/// them, in which case a silent skip is the bug.
fn creds(test_name: &str) -> Option<Creds> {
    let key = std::env::var("SCRAPFLY_API_KEY")
        .ok()
        .filter(|v| !v.is_empty());
    let host = std::env::var("SCRAPFLY_API_HOST")
        .ok()
        .filter(|v| !v.is_empty());
    match (key, host) {
        (Some(key), Some(host)) => Some(Creds {
            key,
            // `Client::build_url` concatenates host + path, so a trailing
            // slash would produce `//scrape`.
            host: host.trim_end_matches('/').to_string(),
        }),
        _ => {
            if std::env::var("SCRAPFLY_REQUIRE_INTEGRATION").as_deref() == Ok("1") {
                panic!(
                    "{test_name}: SCRAPFLY_REQUIRE_INTEGRATION=1 but SCRAPFLY_API_KEY and/or \
                     SCRAPFLY_API_HOST are unset. This run would have reported `ok` while \
                     testing nothing — failing loudly instead."
                );
            }
            println!(
                "SKIP {test_name}: set SCRAPFLY_API_KEY and SCRAPFLY_API_HOST to enable \
                 (libtest captures this line without --nocapture; set \
                 SCRAPFLY_REQUIRE_INTEGRATION=1 in CI to make a credential-less run fail)"
            );
            None
        }
    }
}

/// Path to a PEM root to trust, if one is available.
fn ca_bundle_path() -> Option<String> {
    if let Ok(p) = std::env::var("SCRAPFLY_CA_BUNDLE") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    None
}

/// Never print the API key, even into a local test log.
///
/// Two passes, because there are two shapes to catch. The structured pass
/// handles a URL we built. The literal pass handles everything else — notably
/// `ScrapflyError::Transport`, whose `Display` embeds the whole request URL in
/// parentheses (`error sending request for url (https://.../scrape?key=...)`),
/// which no query-string splitting would reach.
fn redact_key(url: &str) -> String {
    let structured = redact_key_structured(url);
    match std::env::var("SCRAPFLY_API_KEY") {
        Ok(key) if !key.is_empty() => structured.replace(&key, "scp-live-***REDACTED***"),
        _ => structured,
    }
}

fn redact_key_structured(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for (i, part) in url.split('&').enumerate() {
        if i > 0 {
            out.push('&');
        }
        if let Some(rest) = part.strip_prefix("key=") {
            out.push_str("key=");
            out.push_str(&"*".repeat(rest.len().min(8)));
        } else if let Some(idx) = part.find("?key=") {
            out.push_str(&part[..idx]);
            out.push_str("?key=********");
        } else {
            out.push_str(part);
        }
    }
    out
}

/// A throttle refusal is the API declining to produce a data point: the scrape
/// never executed, nothing was billed, and no outcome was observed. It is
/// categorically different from a wrong answer, and reporting it as
/// "leg `asp=true` FAILED at the API" sends an engineer off to debug an alias
/// that is fine.
fn is_throttle_refusal(err: &ScrapflyError) -> bool {
    match err {
        ScrapflyError::TooManyRequests(_) => true,
        ScrapflyError::Api(e)
        | ScrapflyError::ApiClient(e)
        | ScrapflyError::ApiServer(e)
        | ScrapflyError::QuotaLimitReached(e) => {
            e.http_status == 429 || e.code.starts_with("ERR::THROTTLE::")
        }
        _ => false,
    }
}

/// Build a client that talks to `creds.host`, recording every outbound
/// request URL into `seen` so the test can assert on the real wire and count
/// how many requests were actually sent (the SDK retries 5xx up to 3 times —
/// each retry is another billable call, so it must be visible).
fn build_client(creds: &Creds, seen: Arc<Mutex<Vec<String>>>) -> (Client, String) {
    let cb: OnRequest = Arc::new(move |_m: &Method, url: &Url, _h: &HeaderMap| {
        seen.lock().unwrap().push(url.to_string());
    });

    let mut builder = Client::builder()
        .api_key(creds.key.clone())
        .host(creds.host.clone())
        .on_request(cb);

    let tls_note;
    if let Some(path) = ca_bundle_path() {
        let pem = std::fs::read(&path).unwrap_or_else(|e| panic!("read CA bundle {}: {}", path, e));
        let cert = reqwest::Certificate::from_pem(&pem)
            .unwrap_or_else(|e| panic!("parse CA bundle {}: {}", path, e));
        // `http_client()` bypasses the SDK's own timeout wiring, so restate
        // it here rather than silently dropping to reqwest's no-timeout
        // default.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .user_agent("Scrapfly-Rust-SDK")
            .add_root_certificate(cert)
            .build()
            .expect("build reqwest client with extra root CA");
        builder = builder.http_client(http);
        tls_note = format!("verification ON, extra root CA trusted from {}", path);
    } else if std::env::var("SCRAPFLY_INSECURE_TLS").as_deref() == Ok("1") {
        eprintln!(
            "WARNING: SCRAPFLY_INSECURE_TLS=1 — TLS certificate verification is DISABLED \
             for this run. The result proves nothing about the server's identity."
        );
        builder = builder.danger_accept_invalid_certs(true);
        tls_note = "verification DISABLED via SCRAPFLY_INSECURE_TLS=1".to_string();
    } else {
        tls_note = "verification ON, system trust store only".to_string();
    }

    (builder.build().expect("build scrapfly client"), tls_note)
}

/// A plain reqwest client sharing the same trust decision, for the raw leg.
fn build_raw_http() -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .user_agent("Scrapfly-Rust-SDK");
    if let Some(path) = ca_bundle_path() {
        let pem = std::fs::read(&path).unwrap_or_else(|e| panic!("read CA bundle {}: {}", path, e));
        let cert = reqwest::Certificate::from_pem(&pem)
            .unwrap_or_else(|e| panic!("parse CA bundle {}: {}", path, e));
        builder = builder.add_root_certificate(cert);
    } else if std::env::var("SCRAPFLY_INSECURE_TLS").as_deref() == Ok("1") {
        builder = builder.danger_accept_invalid_certs(true);
    }
    builder.build().expect("build raw reqwest client")
}

/// One matrix leg: run a scrape and report what the API echoed back.
struct Leg {
    name: &'static str,
    /// `config.asp` from the response envelope, as raw JSON.
    echoed_asp: serde_json::Value,
    status_code: u16,
    success: bool,
    /// Query pairs on the URL the SDK actually put on the wire.
    wire_asp: Option<String>,
    wire_unblocker: Option<String>,
    uuid: String,
    requests_sent: usize,
}

/// Drive one leg.
///
/// An assertion-relevant error is a real finding and panics with the full
/// context. A throttle refusal is not: the scrape never ran, so it is given
/// exactly one re-dispatch after a wait long enough to drain the window, and
/// if it is refused again the panic says plainly that this is an environment
/// limit rather than an alias divergence.
async fn run_leg(
    client: &Client,
    seen: &Arc<Mutex<Vec<String>>>,
    name: &'static str,
    cfg: ScrapeConfig,
) -> Leg {
    seen.lock().unwrap().clear();

    let mut outcome = client.scrape(&cfg).await;
    if let Err(e) = &outcome {
        if is_throttle_refusal(e) {
            println!(
                "  leg {:<18} throttled before the scrape ran ({}) — waiting {:?} and dispatching once more",
                name,
                redact_key(&e.to_string()),
                THROTTLE_BACKOFF
            );
            tokio::time::sleep(THROTTLE_BACKOFF).await;
            seen.lock().unwrap().clear();
            outcome = client.scrape(&cfg).await;
        }
    }

    let result = match outcome {
        Ok(r) => r,
        Err(e) => {
            let urls: Vec<String> = seen.lock().unwrap().iter().map(|u| redact_key(u)).collect();
            if is_throttle_refusal(&e) {
                panic!(
                    "leg `{}` was THROTTLED before it ran, twice. The scrape never executed, so \
                     this is an ENVIRONMENT LIMIT and NOT an `asp`/`unblocker` divergence — the \
                     dev project throttles httpbin.dev at max_concurrency 5.\n  request(s): {:?}\n  error: {}",
                    name,
                    urls,
                    redact_key(&e.to_string())
                );
            }
            panic!(
                "leg `{}` FAILED at the API.\n  request(s): {:?}\n  error: {}",
                name,
                urls,
                redact_key(&e.to_string())
            );
        }
    };

    let urls: Vec<String> = seen.lock().unwrap().clone();
    let sent = urls.len();
    let last = urls.last().cloned().unwrap_or_default();
    let parsed = Url::parse(&last).expect("parse captured request url");
    let mut wire_asp = None;
    let mut wire_unblocker = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "asp" => wire_asp = Some(v.to_string()),
            "unblocker" => wire_unblocker = Some(v.to_string()),
            _ => {}
        }
    }

    let echoed_asp = result
        .config
        .get("asp")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let cost = result
        .context
        .get("cost")
        .and_then(|c| c.get("total"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    println!(
        "  leg {:<18} -> HTTP {} success={} config.asp={} wire[asp]={:?} wire[unblocker]={:?} cost={} uuid={} requests={}",
        name,
        result.result.status_code,
        result.result.success,
        echoed_asp,
        wire_asp,
        wire_unblocker,
        cost,
        result.uuid,
        sent,
    );
    println!("      request: {}", redact_key(&last));

    Leg {
        name,
        echoed_asp,
        status_code: result.result.status_code,
        success: result.result.success,
        wire_asp,
        wire_unblocker,
        uuid: result.uuid,
        requests_sent: sent,
    }
}

/// The API-side alias leg: `unblocker` on the wire, no SDK folding.
struct RawLeg {
    url: String,
    http_status: u16,
    echoed_asp: serde_json::Value,
    upstream_status: u64,
    success: bool,
    uuid: String,
}

/// Send `unblocker=true` with no `asp` key and read back what the API parsed.
///
/// `ScrapeConfig` cannot express this request — the builder resolves
/// `unblocker` into `asp` — so the URL is assembled by hand and sent through a
/// reqwest client that makes the same trust decision as the SDK client.
async fn run_raw_alias_leg(creds: &Creds) -> RawLeg {
    let http = build_raw_http();
    let mut url = Url::parse(&format!("{}/scrape", creds.host)).expect("parse scrape endpoint");
    url.query_pairs_mut()
        .append_pair("key", &creds.key)
        .append_pair("url", TARGET_URL)
        // An `asp` key of any value wins the server-side precedence rule, and
        // the alias fallback would never be reached.
        .append_pair("unblocker", "true");

    let response = http
        .get(url.clone())
        .header("accept", "application/json")
        .send()
        .await
        .unwrap_or_else(|e| {
            panic!(
                "leg `raw unblocker=true` could not reach the API: {}\n  request: {}",
                redact_key(&e.to_string()),
                redact_key(url.as_str())
            )
        });

    let http_status = response.status().as_u16();
    let text = response.text().await.expect("read raw alias leg body");
    let body: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "leg `raw unblocker=true`: API answered HTTP {} with non-JSON ({}): {}",
            http_status,
            e,
            &text[..text.len().min(400)]
        )
    });

    let leg = RawLeg {
        url: url.to_string(),
        http_status,
        echoed_asp: body
            .pointer("/config/asp")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        upstream_status: body
            .pointer("/result/status_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        success: body
            .pointer("/result/success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        uuid: body
            .get("uuid")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    };

    println!(
        "  leg {:<18} -> API HTTP {} upstream={} success={} config.asp={} uuid={}",
        "raw unblocker=true",
        leg.http_status,
        leg.upstream_status,
        leg.success,
        leg.echoed_asp,
        leg.uuid
    );
    println!("      request: {}", redact_key(&leg.url));

    leg
}

/// Assert a leg actually succeeded. Without this, two legs could "agree" by
/// failing identically and the equivalence claim would be worthless.
fn assert_succeeded(leg: &Leg) {
    assert_eq!(
        leg.status_code, 200,
        "leg `{}` did not return HTTP 200 from the target (uuid={})",
        leg.name, leg.uuid
    );
    assert!(
        leg.success,
        "leg `{}` reported result.success=false (uuid={})",
        leg.name, leg.uuid
    );
}

/// The wire key is frozen at `asp` under both input names — a build that
/// emitted `unblocker` against an API deployment whose parser has not learned
/// it would silently drop a paid feature.
///
/// Note what this pins: the CLIENT's fold. The server's half is leg 5's job.
fn assert_wire_key(leg: &Leg, expect_asp: Option<&str>) {
    assert_eq!(
        leg.wire_asp.as_deref(),
        expect_asp,
        "leg `{}` outbound query carried an unexpected `asp` value",
        leg.name
    );
    assert_eq!(
        leg.wire_unblocker, None,
        "leg `{}` put `unblocker` on the wire; the SDK folds it to `asp`",
        leg.name
    );
}

/// Read the echoed anti-bot value as a bool, failing loudly on any other
/// shape rather than coercing it.
fn echoed_bool(leg: &Leg) -> bool {
    leg.echoed_asp.as_bool().unwrap_or_else(|| {
        panic!(
            "leg `{}`: response envelope config.asp was {} (expected a JSON bool)",
            leg.name, leg.echoed_asp
        )
    })
}

/// Harness self-check: credentials, host wiring and TLS trust, on the free
/// `/account` endpoint. It spends nothing, and it is what separates "the
/// alias is broken" from "this machine cannot reach the dev cluster" when the
/// matrix test below fails.
#[tokio::test]
async fn harness_reaches_the_api() {
    let Some(creds) = creds("harness_reaches_the_api") else {
        return;
    };
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let (client, tls_note) = build_client(&creds, seen);
    println!("harness: host={} tls={}", creds.host, tls_note);
    match client.account().await {
        Ok(acc) => println!(
            "harness: /account OK — project={} suspended={}",
            acc.project.get("name").unwrap_or(&serde_json::Value::Null),
            acc.account
                .get("suspended")
                .unwrap_or(&serde_json::Value::Null),
        ),
        Err(e) => panic!(
            "harness could not reach {}: {}",
            creds.host,
            redact_key(&e.to_string())
        ),
    }
}

/// The whole matrix in one test.
///
/// It is one test on purpose: the assertion under examination is *cross-leg*
/// equivalence, and the legs must run in a known order against one client so
/// the billable-call count is exact and provable.
#[tokio::test]
async fn unblocker_and_asp_are_equivalent_at_the_api() {
    let Some(creds) = creds("unblocker_and_asp_are_equivalent_at_the_api") else {
        return;
    };

    let skip_billable = std::env::var("SCRAPFLY_SKIP_BILLABLE").as_deref() == Ok("1");

    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let (client, tls_note) = build_client(&creds, Arc::clone(&seen));

    println!("host: {}", creds.host);
    println!("tls: {}", tls_note);
    println!("target: {}", TARGET_URL);

    if skip_billable {
        // Deliberately not a silent early return: the cheap legs alone say
        // nothing about the alias, and a green result here would be read as if
        // they did.
        let leg3 = run_leg(
            &client,
            &seen,
            "unblocker=false",
            ScrapeConfig::builder(TARGET_URL)
                .unblocker(false)
                .build()
                .expect("build unblocker=false"),
        )
        .await;
        tokio::time::sleep(LEG_SPACING).await;
        let leg4 = run_leg(
            &client,
            &seen,
            "asp=false",
            ScrapeConfig::builder(TARGET_URL)
                .asp(false)
                .build()
                .expect("build asp=false"),
        )
        .await;
        assert_succeeded(&leg3);
        assert_succeeded(&leg4);
        assert_wire_key(&leg3, None);
        assert_wire_key(&leg4, None);
        assert_eq!(echoed_bool(&leg3), echoed_bool(&leg4));
        panic!(
            "SCRAPFLY_SKIP_BILLABLE=1: the harness works (both cheap legs passed) but the \
             anti-bot legs and the raw `unblocker` alias leg did NOT run, so the equivalence \
             claim is UNTESTED. Unset SCRAPFLY_SKIP_BILLABLE to get a verdict."
        );
    }

    println!("matrix (3 anti-bot legs + 2 cheap):");

    // Leg 1 — current name, enabled. Anti-bot.
    let leg1 = run_leg(
        &client,
        &seen,
        "unblocker=true",
        ScrapeConfig::builder(TARGET_URL)
            .unblocker(true)
            .build()
            .expect("build unblocker=true"),
    )
    .await;

    tokio::time::sleep(LEG_SPACING).await;

    // Leg 2 — deprecated name, enabled. Anti-bot.
    let leg2 = run_leg(
        &client,
        &seen,
        "asp=true",
        ScrapeConfig::builder(TARGET_URL)
            .asp(true)
            .build()
            .expect("build asp=true"),
    )
    .await;

    tokio::time::sleep(LEG_SPACING).await;

    // Leg 3 — current name, disabled. Cheap.
    let leg3 = run_leg(
        &client,
        &seen,
        "unblocker=false",
        ScrapeConfig::builder(TARGET_URL)
            .unblocker(false)
            .build()
            .expect("build unblocker=false"),
    )
    .await;

    tokio::time::sleep(LEG_SPACING).await;

    // Leg 4 — deprecated name, disabled. Cheap.
    let leg4 = run_leg(
        &client,
        &seen,
        "asp=false",
        ScrapeConfig::builder(TARGET_URL)
            .asp(false)
            .build()
            .expect("build asp=false"),
    )
    .await;

    tokio::time::sleep(LEG_SPACING).await;

    // Leg 5 — the API-side alias. The only leg that shows the server the new
    // spelling. Anti-bot.
    let raw = run_raw_alias_leg(&creds).await;

    // A leg that errored out cannot stand in for a leg that agreed.
    for leg in [&leg1, &leg2, &leg3, &leg4] {
        assert_succeeded(leg);
    }

    // Wire: `asp=true` when on, the key absent entirely when off, and
    // `unblocker` never present.
    assert_wire_key(&leg1, Some("true"));
    assert_wire_key(&leg2, Some("true"));
    assert_wire_key(&leg3, None);
    assert_wire_key(&leg4, None);

    // The point of the whole file: the API's own parse of the two names.
    let on1 = echoed_bool(&leg1);
    let on2 = echoed_bool(&leg2);
    let off3 = echoed_bool(&leg3);
    let off4 = echoed_bool(&leg4);

    assert!(
        on1,
        "leg `unblocker=true`: API echoed config.asp={} — the Unblocker was NOT enabled",
        leg1.echoed_asp
    );
    assert!(
        on2,
        "leg `asp=true`: API echoed config.asp={} — the Unblocker was NOT enabled",
        leg2.echoed_asp
    );
    assert_eq!(
        on1, on2,
        "ALIAS BREAK: unblocker=true echoed {} but asp=true echoed {}",
        leg1.echoed_asp, leg2.echoed_asp
    );

    assert!(
        !off3,
        "leg `unblocker=false`: API echoed config.asp={} — the Unblocker was enabled anyway",
        leg3.echoed_asp
    );
    assert!(
        !off4,
        "leg `asp=false`: API echoed config.asp={} — the Unblocker was enabled anyway",
        leg4.echoed_asp
    );
    assert_eq!(
        off3, off4,
        "ALIAS BREAK: unblocker=false echoed {} but asp=false echoed {}",
        leg3.echoed_asp, leg4.echoed_asp
    );

    // Enabled and disabled must actually differ, or all legs agreeing would
    // prove nothing about the flag being read at all.
    assert_ne!(
        on1, off3,
        "enabled and disabled legs echoed the same config.asp — the flag is not being read"
    );

    // --- Leg 5: the server-side alias, the one claim the SDK legs cannot make.
    let raw_query: Vec<(String, String)> = Url::parse(&raw.url)
        .expect("parse raw leg url")
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    assert!(
        raw_query
            .iter()
            .any(|(k, v)| k == "unblocker" && v == "true"),
        "harness error: leg 5 must put `unblocker=true` on the wire, sent {:?}",
        raw_query.iter().map(|(k, _)| k).collect::<Vec<_>>()
    );
    assert!(
        !raw_query.iter().any(|(k, _)| k == "asp"),
        "harness error: leg 5 also carried an `asp` key, which wins server-side precedence \
         so the alias fallback would never be reached"
    );

    assert_eq!(
        raw.http_status, 200,
        "leg `raw unblocker=true`: Scrapfly API answered HTTP {} (uuid={})",
        raw.http_status, raw.uuid
    );
    assert_eq!(
        raw.upstream_status, 200,
        "leg `raw unblocker=true`: upstream answered {} (uuid={})",
        raw.upstream_status, raw.uuid
    );
    assert!(
        raw.success,
        "leg `raw unblocker=true`: scrape reported unsuccessful (uuid={})",
        raw.uuid
    );
    assert_eq!(
        raw.echoed_asp,
        serde_json::Value::Bool(true),
        "THE API-SIDE ALIAS IS BROKEN: a request carrying only `unblocker=true` (no `asp` key) \
         was parsed as config.asp={}. Every customer who migrated to the new name on a raw HTTP \
         client is being billed for an UNPROTECTED scrape. uuid={}",
        raw.echoed_asp,
        raw.uuid
    );

    // The two independent routes to "bypass on" must land on the same state.
    assert_eq!(
        serde_json::Value::Bool(on2),
        raw.echoed_asp,
        "the SDK `asp=true` route and the raw `unblocker=true` route echoed different states"
    );

    let anti_bot: usize = leg1.requests_sent + leg2.requests_sent + 1;
    println!(
        "anti-bot requests sent: {} (leg1={}, leg2={}, raw=1); cheap requests: {}",
        anti_bot,
        leg1.requests_sent,
        leg2.requests_sent,
        leg3.requests_sent + leg4.requests_sent
    );
    assert_eq!(
        anti_bot, 3,
        "expected exactly 3 anti-bot requests, the SDK retry loop sent {}",
        anti_bot
    );
}
