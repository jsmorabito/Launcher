//! Connect to a user-supplied OAuth server (manifest `server` connectors).
//!
//! The extension declares no client id or endpoints. The user enters the
//! server's address once; Launcher reads its RFC 8414 metadata, registers
//! itself as a public client (RFC 7591), and runs the usual PKCE flow.

use crate::error::AppError;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerMetadata {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: String,
}

#[derive(Deserialize)]
struct RawMetadata {
    authorization_endpoint: Option<String>,
    token_endpoint: Option<String>,
    registration_endpoint: Option<String>,
    code_challenge_methods_supported: Option<Vec<String>>,
}

const LOOPBACK_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];

/// Reduce what the user typed to a canonical origin (`scheme://host[:port]`).
/// Only `https://` is allowed, plus `http://` on loopback for self-hosting.
pub fn normalize_server_url(input: &str) -> Result<String, AppError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation("Enter your server's address".into()));
    }
    // Accept a bare host such as `tasks.example.com` or `localhost:4100`.
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        let host = trimmed.split(['/', ':']).next().unwrap_or_default();
        let scheme = if LOOPBACK_HOSTS.contains(&host) {
            "http"
        } else {
            "https"
        };
        format!("{scheme}://{trimmed}")
    };
    let url = url::Url::parse(&candidate)
        .map_err(|_| AppError::Validation("That doesn't look like a valid address".into()))?;
    let host = url
        .host_str()
        .ok_or_else(|| AppError::Validation("That address has no host".into()))?;
    let loopback = LOOPBACK_HOSTS.contains(&host) || host == "::1";
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        _ => {
            return Err(AppError::Validation(
                "Use an https:// address (http:// is only allowed for localhost)".into(),
            ))
        }
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::Validation(
            "Remove the username and password from the address".into(),
        ));
    }
    Ok(url.origin().ascii_serialization())
}

/// Human-friendly account name for a server origin.
pub fn display_name(origin: &str) -> String {
    url::Url::parse(origin)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| u.port().map_or(h.to_string(), |p| format!("{h}:{p}")))
        })
        .unwrap_or_else(|| origin.to_string())
}

pub fn parse_metadata(origin: &str, body: &str) -> Result<ServerMetadata, AppError> {
    let raw: RawMetadata = serde_json::from_str(body)
        .map_err(|_| AppError::OAuth("The server's sign-in details could not be read".into()))?;
    let need = |value: Option<String>, name: &str| {
        value.ok_or_else(|| AppError::OAuth(format!("The server does not advertise {name}")))
    };
    let meta = ServerMetadata {
        authorization_endpoint: need(raw.authorization_endpoint, "an authorization endpoint")?,
        token_endpoint: need(raw.token_endpoint, "a token endpoint")?,
        registration_endpoint: need(
            raw.registration_endpoint,
            "app registration (RFC 7591), which Launcher needs to connect",
        )?,
    };
    if let Some(methods) = raw.code_challenge_methods_supported {
        if !methods.iter().any(|m| m == "S256") {
            return Err(AppError::OAuth(
                "The server does not support PKCE (S256)".into(),
            ));
        }
    }
    // Tokens and the client registration go to these URLs, so they must stay on
    // the address the user typed, not wherever the metadata points.
    for (label, endpoint) in [
        ("authorization", &meta.authorization_endpoint),
        ("token", &meta.token_endpoint),
        ("registration", &meta.registration_endpoint),
    ] {
        let same_origin = url::Url::parse(endpoint)
            .map(|u| u.origin().ascii_serialization() == origin)
            .unwrap_or(false);
        if !same_origin {
            return Err(AppError::OAuth(format!(
                "The server's {label} endpoint is on a different address than {origin}"
            )));
        }
    }
    Ok(meta)
}

pub async fn discover(origin: &str) -> Result<ServerMetadata, AppError> {
    let url = format!("{origin}/.well-known/oauth-authorization-server");
    let response = reqwest::Client::new()
        .get(&url)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|_| AppError::OAuth(format!("Could not reach {origin}")))?;
    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "{origin} does not offer sign-in (HTTP {}). Is it set up for remote access?",
            response.status().as_u16()
        )));
    }
    let body = response
        .text()
        .await
        .map_err(|_| AppError::OAuth("The server's sign-in details could not be read".into()))?;
    parse_metadata(origin, &body)
}

/// Register Launcher as a public client for exactly one redirect URI.
pub async fn register_client(
    registration_endpoint: &str,
    client_name: &str,
    redirect_uri: &str,
    scopes: &[String],
) -> Result<String, AppError> {
    let response = reqwest::Client::new()
        .post(registration_endpoint)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .json(&json!({
            "client_name": client_name,
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
            "scope": scopes.join(" "),
        }))
        .send()
        .await
        .map_err(|_| AppError::OAuth("Could not register Launcher with the server".into()))?;
    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "The server refused to register Launcher (HTTP {})",
            response.status().as_u16()
        )));
    }
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|_| AppError::OAuth("The server's registration reply could not be read".into()))?;
    value
        .get("client_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| AppError::OAuth("The server's registration reply had no client id".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://tasks.example.com";

    fn metadata_json(origin: &str) -> String {
        json!({
            "issuer": origin,
            "authorization_endpoint": format!("{origin}/oauth/authorize"),
            "token_endpoint": format!("{origin}/oauth/token"),
            "registration_endpoint": format!("{origin}/oauth/register"),
            "code_challenge_methods_supported": ["S256"],
        })
        .to_string()
    }

    #[test]
    fn normalizes_to_an_origin() {
        for (input, want) in [
            ("https://tasks.example.com", ORIGIN),
            ("https://tasks.example.com/", ORIGIN),
            ("  https://tasks.example.com/some/path?x=1#y ", ORIGIN),
            ("tasks.example.com", ORIGIN),
            ("HTTPS://Tasks.Example.com", ORIGIN),
            ("http://localhost:4100", "http://localhost:4100"),
            ("localhost:4100", "http://localhost:4100"),
            ("127.0.0.1:4100/x", "http://127.0.0.1:4100"),
        ] {
            assert_eq!(normalize_server_url(input).unwrap(), want, "{input}");
        }
    }

    #[test]
    fn rejects_unsafe_or_malformed_addresses() {
        for bad in [
            "",
            "   ",
            "http://tasks.example.com",
            "ftp://tasks.example.com",
            "https://user:pw@tasks.example.com",
            "https://",
            "javascript:alert(1)",
        ] {
            assert!(normalize_server_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn display_name_is_host_and_optional_port() {
        assert_eq!(display_name(ORIGIN), "tasks.example.com");
        assert_eq!(display_name("http://localhost:4100"), "localhost:4100");
    }

    #[test]
    fn parses_complete_metadata() {
        let meta = parse_metadata(ORIGIN, &metadata_json(ORIGIN)).unwrap();
        assert_eq!(meta.token_endpoint, "https://tasks.example.com/oauth/token");
        assert_eq!(
            meta.registration_endpoint,
            "https://tasks.example.com/oauth/register"
        );
    }

    #[test]
    fn requires_registration_and_pkce() {
        let no_reg = json!({
            "authorization_endpoint": format!("{ORIGIN}/a"),
            "token_endpoint": format!("{ORIGIN}/t"),
        })
        .to_string();
        assert!(parse_metadata(ORIGIN, &no_reg)
            .unwrap_err()
            .to_string()
            .contains("registration"));

        let plain_only = json!({
            "authorization_endpoint": format!("{ORIGIN}/a"),
            "token_endpoint": format!("{ORIGIN}/t"),
            "registration_endpoint": format!("{ORIGIN}/r"),
            "code_challenge_methods_supported": ["plain"],
        })
        .to_string();
        assert!(parse_metadata(ORIGIN, &plain_only)
            .unwrap_err()
            .to_string()
            .contains("PKCE"));
        assert!(parse_metadata(ORIGIN, "not json").is_err());
    }

    #[test]
    fn endpoints_must_stay_on_the_typed_origin() {
        let hijack = json!({
            "authorization_endpoint": format!("{ORIGIN}/a"),
            "token_endpoint": "https://evil.example/token",
            "registration_endpoint": format!("{ORIGIN}/r"),
        })
        .to_string();
        let err = parse_metadata(ORIGIN, &hijack).unwrap_err().to_string();
        assert!(
            err.contains("token") && err.contains("different address"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn discovers_metadata_from_the_well_known_path() {
        let mut server = mockito::Server::new_async().await;
        let origin = server.url();
        let mock = server
            .mock("GET", "/.well-known/oauth-authorization-server")
            .with_status(200)
            .with_body(metadata_json(&origin))
            .create_async()
            .await;
        // mockito listens on http://127.0.0.1:<port>, which is a valid loopback origin.
        let meta = discover(&normalize_server_url(&origin).unwrap())
            .await
            .unwrap();
        mock.assert_async().await;
        assert!(meta.authorization_endpoint.ends_with("/oauth/authorize"));
    }

    #[tokio::test]
    async fn discovery_explains_a_server_without_oauth() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/.well-known/oauth-authorization-server")
            .with_status(404)
            .create_async()
            .await;
        let err = discover(&normalize_server_url(&server.url()).unwrap())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("does not offer sign-in"), "{err}");
    }

    #[tokio::test]
    async fn registers_a_public_client_for_one_redirect() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/oauth/register")
            .match_body(mockito::Matcher::PartialJson(json!({
                "client_name": "Launcher",
                "redirect_uris": ["http://127.0.0.1:5555/oauth/callback"],
                "token_endpoint_auth_method": "none",
                "scope": "tasks",
            })))
            .with_status(201)
            .with_body(r#"{"client_id":"abc123"}"#)
            .create_async()
            .await;
        let id = register_client(
            &format!("{}/oauth/register", server.url()),
            "Launcher",
            "http://127.0.0.1:5555/oauth/callback",
            &["tasks".to_string()],
        )
        .await
        .unwrap();
        mock.assert_async().await;
        assert_eq!(id, "abc123");
    }

    #[tokio::test]
    async fn registration_failures_are_reported() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/r")
            .with_status(400)
            .create_async()
            .await;
        server
            .mock("POST", "/empty")
            .with_status(201)
            .with_body("{}")
            .create_async()
            .await;
        let r = |p: &str| format!("{}{p}", server.url());
        assert!(
            register_client(&r("/r"), "L", "http://127.0.0.1:1/oauth/callback", &[])
                .await
                .is_err()
        );
        assert!(
            register_client(&r("/empty"), "L", "http://127.0.0.1:1/oauth/callback", &[])
                .await
                .is_err()
        );
    }
}
