//! A screenshot with a webhook: the API answers 201 and its job, the image goes to the webhook.

use scrapfly_sdk::result::screenshot::ScreenshotResult;
use scrapfly_sdk::{Client, ScrapflyError, ScreenshotConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const ENQUEUED: &[u8] = br#"{"success":true,"webhook_enqueued":true,"job_uuid":"job-1","log_uuid":"log-1","webhook_name":"my-webhook"}"#;

async fn answer_one_request(
    listener: tokio::net::TcpListener,
    status: &str,
    content_type: &str,
    body: &[u8],
) {
    let (mut socket, _) = listener.accept().await.expect("accept");
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = socket.read(&mut buf).await.expect("read");
        raw.extend_from_slice(&buf[..n]);
    }
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(head.as_bytes()).await.expect("write");
    socket.write_all(body).await.expect("write");
}

async fn screenshot_against(
    status: &'static str,
    content_type: &'static str,
    body: &'static [u8],
) -> Result<ScreenshotResult, Box<ScrapflyError>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = tokio::spawn(answer_one_request(listener, status, content_type, body));
    let client = Client::builder()
        .api_key("scp-test-key")
        .host(format!("http://127.0.0.1:{port}"))
        .build()
        .expect("client");
    let config = ScreenshotConfig::builder("https://web-scraping.dev/product/1")
        .webhook("my-webhook")
        .build()
        .expect("config");
    let answer = client.screenshot(&config).await.map_err(Box::new);
    server.await.expect("server");
    answer
}

#[tokio::test]
async fn enqueued_for_a_webhook_returns_its_job() {
    let result = screenshot_against("201 Created", "application/json; charset=utf-8", ENQUEUED)
        .await
        .expect("an enqueued screenshot is no error");

    assert_eq!(result.job_uuid.as_deref(), Some("job-1"));
    assert!(result.image.is_empty());
    assert_eq!(result.metadata.extension_name, "");
    assert_eq!(result.metadata.upstream_status_code, 0);
    assert_eq!(result.metadata.upstream_url, "");
}

#[tokio::test]
async fn saving_a_screenshot_enqueued_for_a_webhook_is_refused() {
    let result = screenshot_against("201 Created", "application/json", ENQUEUED)
        .await
        .expect("an enqueued screenshot is no error");
    let dir = std::env::temp_dir().join(format!("scrapfly-sdk-test-{}", std::process::id()));

    let saved = result.save("shot", Some(&dir));

    assert!(
        saved.is_err(),
        "saved a screenshot with no image: {saved:?}"
    );
    assert!(!dir.join("shot.bin").exists());
}

#[tokio::test]
async fn any_other_201_is_an_error() {
    let bodies: [&'static [u8]; 5] = [
        br#"{"success":true}"#,
        br#"{"webhook_enqueued":false,"job_uuid":"job-1"}"#,
        br#"{"webhook_enqueued":true}"#,
        br#"{"webhook_enqueued":true,"job_uuid":""}"#,
        b"not json",
    ];
    for body in bodies {
        let answer = screenshot_against("201 Created", "application/json", body).await;

        assert!(
            answer.is_err(),
            "{:?}: {answer:?}",
            String::from_utf8_lossy(body)
        );
    }
}

#[tokio::test]
async fn a_json_answer_is_never_an_image() {
    let answer = screenshot_against(
        "200 OK",
        "application/json; charset=utf-8",
        br#"{"success":true}"#,
    )
    .await;

    assert!(answer.is_err(), "{answer:?}");
}

#[tokio::test]
async fn an_image_has_no_job() {
    let result = screenshot_against("200 OK", "image/png", b"\x89PNG")
        .await
        .expect("an image");

    assert_eq!(result.job_uuid, None);
    assert_eq!(result.image.as_ref(), b"\x89PNG");
    assert_eq!(result.metadata.extension_name, "png");
}

#[test]
fn saving_an_image_still_writes_it() {
    let dir = std::env::temp_dir().join(format!("scrapfly-sdk-test-image-{}", std::process::id()));
    let result = ScreenshotResult {
        image: bytes::Bytes::from_static(b"\x89PNG"),
        metadata: scrapfly_sdk::result::screenshot::ScreenshotMetadata {
            extension_name: "png".to_string(),
            ..Default::default()
        },
        job_uuid: None,
    };

    let path = result.save("shot", Some(&dir)).expect("save");

    assert_eq!(std::fs::read(&path).expect("read"), b"\x89PNG");
    std::fs::remove_dir_all(&dir).expect("clean");
}
