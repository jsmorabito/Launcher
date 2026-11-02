//! Identifies which account a freshly issued token belongs to.
//!
//! A connector's manifest declares one request ([`AccountProbe`]) and where to
//! find the account id and display name in the JSON response, so the host can
//! name accounts without any extension code running during the connect flow.

use crate::error::AppError;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProbe {
    pub url: String,
    /// `GET` (default) or `POST`.
    #[serde(default)]
    pub method: Option<String>,
    /// Sent as a JSON body.
    #[serde(default)]
    pub body: Option<String>,
    /// Dot path to the stable account id in the JSON response.
    pub id_path: String,
    /// Dot path to the human-readable account name.
    pub name_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountIdentity {
    pub account_id: String,
    pub display_name: String,
}

/// Reads `a.b.c` out of `value`. Strings and numbers are returned as text.
pub fn json_path(value: &Value, path: &str) -> Option<String> {
    let mut cur = value;
    for key in path.split('.') {
        cur = match cur {
            Value::Object(map) => map.get(key)?,
            Value::Array(items) => items.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    match cur {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn identity_from_response(
    probe: &AccountProbe,
    response: &Value,
) -> Result<AccountIdentity, AppError> {
    let account_id = json_path(response, &probe.id_path).ok_or_else(|| {
        AppError::OAuth(format!(
            "Account lookup returned no id at '{}'",
            probe.id_path
        ))
    })?;
    let display_name = json_path(response, &probe.name_path).unwrap_or_else(|| account_id.clone());
    Ok(AccountIdentity {
        account_id,
        display_name,
    })
}

/// The probe carries the user's access token, so it may only go to a host the
/// connector already sends OAuth traffic to, and only over HTTPS.
pub fn validate_probe_url(
    probe_url: &str,
    authorization_url: &str,
    token_url: &str,
) -> Result<(), AppError> {
    let parse = |label: &str, raw: &str| {
        reqwest::Url::parse(raw).map_err(|e| AppError::OAuth(format!("Invalid {label} URL: {e}")))
    };
    let probe = parse("account", probe_url)?;
    if probe.scheme() != "https" {
        return Err(AppError::OAuth("Account lookup must use https".into()));
    }
    let allowed = [authorization_url, token_url]
        .iter()
        .filter_map(|u| reqwest::Url::parse(u).ok())
        .any(|u| u.host_str() == probe.host_str());
    if !allowed {
        return Err(AppError::OAuth(
            "Account lookup host must match the connector's OAuth hosts".into(),
        ));
    }
    Ok(())
}

pub async fn fetch_identity(
    probe: &AccountProbe,
    access_token: &str,
) -> Result<AccountIdentity, AppError> {
    let client = reqwest::Client::new();
    let is_post = probe
        .method
        .as_deref()
        .is_some_and(|m| m.eq_ignore_ascii_case("POST"));
    let mut request = if is_post {
        client.post(&probe.url)
    } else {
        client.get(&probe.url)
    };
    request = request
        .bearer_auth(access_token)
        .header("Accept", "application/json");
    if let Some(body) = &probe.body {
        request = request
            .header("Content-Type", "application/json")
            .body(body.clone());
    }

    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Account lookup failed ({})",
            response.status()
        )));
    }
    let json: Value = response
        .json()
        .await
        .map_err(|e| AppError::OAuth(format!("Failed to parse account lookup: {e}")))?;
    identity_from_response(probe, &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn probe() -> AccountProbe {
        AccountProbe {
            url: "https://api.linear.app/graphql".into(),
            method: Some("POST".into()),
            body: None,
            id_path: "data.organization.id".into(),
            name_path: "data.organization.name".into(),
        }
    }

    #[test]
    fn json_path_walks_objects_arrays_and_numbers() {
        let v = json!({"a": {"b": [{"c": 42}]}, "s": "x"});
        assert_eq!(json_path(&v, "a.b.0.c").as_deref(), Some("42"));
        assert_eq!(json_path(&v, "s").as_deref(), Some("x"));
        assert_eq!(json_path(&v, "a.missing"), None);
        assert_eq!(json_path(&v, "a.b.9"), None);
    }

    #[test]
    fn json_path_rejects_empty_strings_and_non_scalars() {
        let v = json!({"e": "", "o": {"k": 1}});
        assert_eq!(json_path(&v, "e"), None);
        assert_eq!(json_path(&v, "o"), None);
    }

    #[test]
    fn identity_reads_id_and_name() {
        let v = json!({"data": {"organization": {"id": "o1", "name": "Acme"}}});
        let id = identity_from_response(&probe(), &v).unwrap();
        assert_eq!(id.account_id, "o1");
        assert_eq!(id.display_name, "Acme");
    }

    #[test]
    fn identity_falls_back_to_id_when_name_missing() {
        let v = json!({"data": {"organization": {"id": "o1"}}});
        assert_eq!(
            identity_from_response(&probe(), &v).unwrap().display_name,
            "o1"
        );
    }

    #[test]
    fn identity_errors_without_an_id() {
        assert!(identity_from_response(&probe(), &json!({})).is_err());
    }

    #[test]
    fn probe_url_must_be_https_on_an_oauth_host() {
        let auth = "https://linear.app/oauth/authorize";
        let token = "https://api.linear.app/oauth/token";
        assert!(validate_probe_url("https://api.linear.app/graphql", auth, token).is_ok());
        assert!(validate_probe_url("https://linear.app/api", auth, token).is_ok());
        assert!(validate_probe_url("http://api.linear.app/graphql", auth, token).is_err());
        assert!(validate_probe_url("https://evil.example/graphql", auth, token).is_err());
        assert!(validate_probe_url("not a url", auth, token).is_err());
    }
}
