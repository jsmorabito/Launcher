//! Requests an extension makes to its connected server through the host.
//!
//! The host adds the account's token, so the extension never holds it, and
//! sends the request only to the server address the user connected. That is
//! also why loopback and LAN servers work here even though `network.fetch`
//! refuses private addresses: the user entered that address themselves.

use crate::error::AppError;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_TIMEOUT: Duration = Duration::from_secs(60);
/// Responses larger than this are rejected rather than buffered.
const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;

/// Headers the extension may not set: the host owns auth and framing.
const BLOCKED_HEADERS: [&str; 8] = [
    "authorization",
    "host",
    "cookie",
    "content-length",
    "transfer-encoding",
    "connection",
    "proxy-authorization",
    "upgrade",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerResponse {
    pub status: u16,
    pub ok: bool,
    pub body: String,
}

/// Join `path` onto the server origin. The path must stay on that origin.
pub fn build_url(server_url: &str, path: &str) -> Result<String, AppError> {
    let invalid = || AppError::Validation("Invalid request path".into());
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['\\', '\r', '\n', '\0'])
        || path
            .split(['/', '?', '#'])
            .any(|seg| seg == ".." || seg == ".")
    {
        return Err(invalid());
    }
    let base = url::Url::parse(server_url).map_err(|_| invalid())?;
    let joined = url::Url::parse(&format!("{}{}", base.origin().ascii_serialization(), path))
        .map_err(|_| invalid())?;
    if joined.origin() != base.origin() {
        return Err(invalid());
    }
    Ok(joined.to_string())
}

pub fn validate_method(method: &str) -> Result<reqwest::Method, AppError> {
    match method.to_ascii_uppercase().as_str() {
        "GET" => Ok(reqwest::Method::GET),
        "POST" => Ok(reqwest::Method::POST),
        "PUT" => Ok(reqwest::Method::PUT),
        "PATCH" => Ok(reqwest::Method::PATCH),
        "DELETE" => Ok(reqwest::Method::DELETE),
        _ => Err(AppError::Validation("Unsupported request method".into())),
    }
}

/// Drop headers the host controls (and anything malformed).
pub fn allowed_headers(headers: HashMap<String, String>) -> Vec<(String, String)> {
    headers
        .into_iter()
        .filter(|(name, value)| {
            let lower = name.to_ascii_lowercase();
            !BLOCKED_HEADERS.contains(&lower.as_str())
                && !lower.starts_with("proxy-")
                && !name.is_empty()
                && !name.contains(['\r', '\n', ':'])
                && !value.contains(['\r', '\n'])
        })
        .collect()
}

pub fn clamp_timeout(requested_ms: Option<u64>) -> Duration {
    requested_ms
        .map(Duration::from_millis)
        .map_or(DEFAULT_TIMEOUT, |d| d.min(MAX_TIMEOUT))
}

pub async fn send(
    url: &str,
    method: reqwest::Method,
    access_token: &str,
    headers: Vec<(String, String)>,
    body: Option<String>,
    timeout: Duration,
) -> Result<ServerResponse, AppError> {
    // Never follow redirects: one could carry the bearer token to another host.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|e| AppError::Other(format!("Could not start the request: {e}")))?;
    let mut request = client.request(method, url).bearer_auth(access_token);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    if let Some(body) = body {
        request = request.body(body);
    }
    let response = request
        .send()
        .await
        .map_err(|_| AppError::Other("Could not reach the server".into()))?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|n| n as usize > MAX_RESPONSE_BYTES)
    {
        return Err(AppError::Other(
            "The server's response was too large".into(),
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| AppError::Other("The server's response could not be read".into()))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(AppError::Other(
            "The server's response was too large".into(),
        ));
    }
    Ok(ServerResponse {
        status: status.as_u16(),
        ok: status.is_success(),
        body: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_paths_onto_the_server_origin() {
        assert_eq!(
            build_url("https://tasks.example.com", "/mcp").unwrap(),
            "https://tasks.example.com/mcp"
        );
        assert_eq!(
            build_url("http://localhost:4100", "/trpc/x?a=1").unwrap(),
            "http://localhost:4100/trpc/x?a=1"
        );
    }

    #[test]
    fn rejects_paths_that_could_leave_the_origin() {
        for bad in [
            "mcp",
            "//evil.example/x",
            "/a/../b",
            "/./a",
            "/a?x=/../y",
            "/a\\b",
            "/a\r\nHost: evil",
            "https://evil.example/x",
            "",
        ] {
            assert!(
                build_url("https://tasks.example.com", bad).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn only_standard_methods_are_allowed() {
        assert!(validate_method("post").is_ok());
        assert!(validate_method("DELETE").is_ok());
        assert!(validate_method("CONNECT").is_err());
        assert!(validate_method("TRACE").is_err());
    }

    #[test]
    fn host_owned_and_malformed_headers_are_dropped() {
        let kept = allowed_headers(HashMap::from([
            ("Authorization".to_string(), "Bearer stolen".to_string()),
            ("COOKIE".to_string(), "a=b".to_string()),
            ("Host".to_string(), "evil".to_string()),
            ("Proxy-Foo".to_string(), "x".to_string()),
            ("X-Bad\r\nInjected".to_string(), "x".to_string()),
            ("X-Ok".to_string(), "line1\r\nline2".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ]));
        let mut names: Vec<_> = kept.iter().map(|(n, _)| n.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["Accept", "Content-Type"]);
    }

    #[test]
    fn timeout_defaults_and_is_capped() {
        assert_eq!(clamp_timeout(None), Duration::from_secs(30));
        assert_eq!(clamp_timeout(Some(5_000)), Duration::from_secs(5));
        assert_eq!(clamp_timeout(Some(9_999_999)), Duration::from_secs(60));
    }

    #[tokio::test]
    async fn sends_the_bearer_token_and_returns_status_and_body() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/mcp")
            .match_header("authorization", "Bearer tok123")
            .match_header("content-type", "application/json")
            .match_body("{\"a\":1}")
            .with_status(200)
            .with_body("{\"ok\":true}")
            .create_async()
            .await;
        let res = send(
            &format!("{}/mcp", server.url()),
            reqwest::Method::POST,
            "tok123",
            vec![("Content-Type".into(), "application/json".into())],
            Some("{\"a\":1}".into()),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        mock.assert_async().await;
        assert!(res.ok);
        assert_eq!(res.status, 200);
        assert_eq!(res.body, "{\"ok\":true}");
    }

    #[tokio::test]
    async fn reports_error_statuses_without_failing() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/x")
            .with_status(401)
            .with_body("nope")
            .create_async()
            .await;
        let res = send(
            &format!("{}/x", server.url()),
            reqwest::Method::GET,
            "t",
            vec![],
            None,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert!(!res.ok);
        assert_eq!(res.status, 401);
    }

    #[tokio::test]
    async fn does_not_follow_redirects_with_the_token() {
        let mut server = mockito::Server::new_async().await;
        let elsewhere = server
            .mock("GET", "/elsewhere")
            .match_header("authorization", mockito::Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        server
            .mock("GET", "/go")
            .with_status(302)
            .with_header("location", "/elsewhere")
            .create_async()
            .await;
        let res = send(
            &format!("{}/go", server.url()),
            reqwest::Method::GET,
            "secret",
            vec![],
            None,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(res.status, 302);
        elsewhere.assert_async().await;
    }
}
