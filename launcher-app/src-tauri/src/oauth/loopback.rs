//! One-shot localhost listener for the OAuth redirect (RFC 8252 §7.3).
//!
//! Providers that cannot redirect to a custom `launcher://` scheme (anything
//! that only allows `https://` or loopback redirects) send the browser to
//! `http://127.0.0.1:<port>/oauth/callback` instead. The listener accepts that
//! single request, answers with a static "you can close this tab" page, and
//! hands the query string back so the normal exchange path can finish the flow.

use crate::error::AppError;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const CALLBACK_PATH: &str = "/oauth/callback";

/// Longest request head we are willing to read from the browser.
const MAX_REQUEST_BYTES: usize = 8 * 1024;
/// A connection that sends nothing (a browser preconnect) must not stall the listener.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

const SUCCESS_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Launcher</title>\
<body style=\"font:16px system-ui;margin:4rem auto;max-width:28rem;text-align:center\">\
<h1>You're connected</h1><p>You can close this tab and return to Launcher.</p></body>";

/// Bind an ephemeral port on the loopback interface only.
pub async fn bind() -> Result<(TcpListener, u16), AppError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| AppError::OAuth(format!("Could not open a local callback port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| AppError::OAuth(format!("Could not read the callback port: {e}")))?
        .port();
    Ok((listener, port))
}

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

/// Wait for the provider's redirect and return its query string (no leading `?`).
/// Requests for any other path get a 404 and do not end the wait.
pub async fn wait_for_callback(
    listener: TcpListener,
    timeout: Duration,
) -> Result<String, AppError> {
    tokio::time::timeout(timeout, async {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            if let Some(query) = handle_connection(stream).await {
                return query;
            }
        }
    })
    .await
    .map_err(|_| AppError::OAuth("Timed out waiting for the browser to return to Launcher".into()))
}

async fn handle_connection(mut stream: TcpStream) -> Option<String> {
    let head = tokio::time::timeout(READ_TIMEOUT, read_request_head(&mut stream))
        .await
        .ok()??;
    let query = callback_query(&head);
    let response = match &query {
        Some(_) => http_response("200 OK", SUCCESS_PAGE),
        None => http_response("404 Not Found", "Not found"),
    };
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
    query
}

async fn read_request_head(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < MAX_REQUEST_BYTES {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.contains(&b'\n') {
            break;
        }
    }
    String::from_utf8(buf).ok()
}

/// The query string when the request line is `GET /oauth/callback?... HTTP/1.x`.
pub fn callback_query(head: &str) -> Option<String> {
    let line = head.lines().next()?;
    let mut parts = line.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    (path == CALLBACK_PATH && !query.is_empty()).then(|| query.to_string())
}

fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn get(port: u16, target: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[test]
    fn callback_query_accepts_only_get_on_the_callback_path_with_a_query() {
        let ok = "GET /oauth/callback?code=c&state=s HTTP/1.1\r\nHost: x\r\n\r\n";
        assert_eq!(callback_query(ok).as_deref(), Some("code=c&state=s"));
        assert_eq!(callback_query("GET /oauth/callback HTTP/1.1"), None);
        assert_eq!(callback_query("GET /favicon.ico HTTP/1.1"), None);
        assert_eq!(callback_query("POST /oauth/callback?code=c HTTP/1.1"), None);
        assert_eq!(
            callback_query("GET /oauth/callback/../x?code=c HTTP/1.1"),
            None
        );
        assert_eq!(callback_query(""), None);
    }

    #[test]
    fn redirect_uri_is_loopback_ip_with_the_port() {
        assert_eq!(redirect_uri(4321), "http://127.0.0.1:4321/oauth/callback");
    }

    #[tokio::test]
    async fn returns_the_query_and_serves_a_success_page() {
        let (listener, port) = bind().await.unwrap();
        let waiter = tokio::spawn(wait_for_callback(listener, Duration::from_secs(5)));
        let reply = get(port, "/oauth/callback?code=abc&state=xyz").await;
        assert!(reply.starts_with("HTTP/1.1 200"));
        assert!(reply.contains("You're connected"));
        assert!(!reply.contains("abc"), "the page must not echo the code");
        assert_eq!(waiter.await.unwrap().unwrap(), "code=abc&state=xyz");
    }

    #[tokio::test]
    async fn stray_requests_get_404_and_do_not_end_the_wait() {
        let (listener, port) = bind().await.unwrap();
        let waiter = tokio::spawn(wait_for_callback(listener, Duration::from_secs(5)));
        assert!(get(port, "/favicon.ico").await.starts_with("HTTP/1.1 404"));
        get(port, "/oauth/callback?error=access_denied&state=s").await;
        assert_eq!(
            waiter.await.unwrap().unwrap(),
            "error=access_denied&state=s"
        );
    }

    #[tokio::test]
    async fn times_out_when_no_callback_arrives() {
        let (listener, _port) = bind().await.unwrap();
        let err = wait_for_callback(listener, Duration::from_millis(50))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Timed out"));
    }

    #[tokio::test]
    async fn binds_to_loopback_only() {
        let (listener, _) = bind().await.unwrap();
        assert!(listener.local_addr().unwrap().ip().is_loopback());
    }
}
