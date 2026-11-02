//! Manifest `connectors`: accounts an extension needs, connected by the host.
//!
//! The extension only declares the provider (OAuth endpoints, scopes, and how
//! to identify the signed-in account). Launcher runs the whole connect flow in
//! its own UI and hands the extension an access token on request, so no
//! extension code runs during sign-in.

use crate::oauth::account_probe::{validate_probe_url, AccountProbe};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectorOAuthDecl {
    pub client_id: String,
    pub authorization_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectorAccountDecl {
    pub url: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    pub id_path: String,
    pub name_path: String,
}

impl ConnectorAccountDecl {
    pub fn to_probe(&self) -> AccountProbe {
        AccountProbe {
            url: self.url.clone(),
            method: self.method.clone(),
            body: self.body.clone(),
            id_path: self.id_path.clone(),
            name_path: self.name_path.clone(),
        }
    }
}

/// A connector that talks to a server the user supplies (self-hosted or one of
/// many deployments). Launcher discovers the server's OAuth endpoints and
/// registers itself as a client, so the manifest carries no client id.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectorServerDecl {
    /// Prefilled in the connect dialog, e.g. `http://localhost:4100`.
    #[serde(default)]
    pub default_url: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Name shown on the server's consent screen. Defaults to "Launcher".
    #[serde(default)]
    pub client_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectorDecl {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    /// Commands of this extension ask the user to connect before they run.
    #[serde(default)]
    pub required: Option<bool>,
    /// More than one account (e.g. several workspaces) may be connected.
    #[serde(default)]
    pub multi_account: Option<bool>,
    /// Fixed provider (e.g. Linear). Declare this with `account`, or `server`, not both.
    #[serde(default)]
    pub oauth: Option<ConnectorOAuthDecl>,
    #[serde(default)]
    pub account: Option<ConnectorAccountDecl>,
    /// User-supplied server. Declare this instead of `oauth` and `account`.
    #[serde(default)]
    pub server: Option<ConnectorServerDecl>,
}

fn require_https(label: &str, raw: &str) -> Result<(), String> {
    match reqwest::Url::parse(raw) {
        Ok(u) if u.scheme() == "https" => Ok(()),
        Ok(_) => Err(format!("{label} must use https")),
        Err(_) => Err(format!("{label} is not a valid URL")),
    }
}

fn validate_fixed(
    id: &str,
    oauth: &ConnectorOAuthDecl,
    account: &ConnectorAccountDecl,
) -> Result<(), String> {
    if oauth.client_id.trim().is_empty() {
        return Err(format!("Connector '{id}' must have an oauth.clientId"));
    }
    if oauth.scopes.iter().any(|s| s.trim().is_empty()) {
        return Err(format!("Connector '{id}' has an empty scope"));
    }
    require_https(
        &format!("Connector '{id}' oauth.authorizationUrl"),
        &oauth.authorization_url,
    )?;
    require_https(
        &format!("Connector '{id}' oauth.tokenUrl"),
        &oauth.token_url,
    )?;
    validate_probe_url(&account.url, &oauth.authorization_url, &oauth.token_url)
        .map_err(|e| format!("Connector '{id}' account.url: {e}"))?;
    if let Some(m) = &account.method {
        if !m.eq_ignore_ascii_case("GET") && !m.eq_ignore_ascii_case("POST") {
            return Err(format!(
                "Connector '{id}' account.method must be GET or POST"
            ));
        }
    }
    if account.id_path.trim().is_empty() || account.name_path.trim().is_empty() {
        return Err(format!(
            "Connector '{id}' account needs idPath and namePath"
        ));
    }
    Ok(())
}

fn validate_server(id: &str, server: &ConnectorServerDecl) -> Result<(), String> {
    if server.scopes.iter().any(|s| s.trim().is_empty()) {
        return Err(format!("Connector '{id}' has an empty server scope"));
    }
    if let Some(url) = &server.default_url {
        crate::oauth::dynamic::normalize_server_url(url)
            .map_err(|e| format!("Connector '{id}' server.defaultUrl: {e}"))?;
    }
    Ok(())
}

pub fn validate_connectors(connectors: &[ConnectorDecl]) -> Result<(), String> {
    let id_re = Regex::new(r"^[a-z][a-z0-9-]*$").unwrap();
    let mut seen = HashSet::new();

    for c in connectors {
        if !id_re.is_match(&c.id) {
            return Err(format!(
                "Connector id '{}' must match /^[a-z][a-z0-9-]*$/",
                c.id
            ));
        }
        if !seen.insert(c.id.as_str()) {
            return Err(format!("Duplicate connector id '{}'", c.id));
        }
        if c.name.trim().is_empty() {
            return Err(format!("Connector '{}' must have a name", c.id));
        }
        match (&c.oauth, &c.account, &c.server) {
            (Some(oauth), Some(account), None) => validate_fixed(&c.id, oauth, account)?,
            (None, None, Some(server)) => validate_server(&c.id, server)?,
            _ => {
                return Err(format!(
                    "Connector '{}' must declare either oauth with account, or server",
                    c.id
                ))
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn linear() -> ConnectorDecl {
        serde_json::from_value(json!({
            "id": "linear",
            "name": "Linear",
            "multiAccount": true,
            "oauth": {
                "clientId": "abc",
                "authorizationUrl": "https://linear.app/oauth/authorize",
                "tokenUrl": "https://api.linear.app/oauth/token",
                "scopes": ["read", "issues:create"]
            },
            "account": {
                "url": "https://api.linear.app/graphql",
                "method": "POST",
                "body": "{\"query\":\"{ organization { id name } }\"}",
                "idPath": "data.organization.id",
                "namePath": "data.organization.name"
            }
        }))
        .unwrap()
    }

    #[test]
    fn accepts_a_complete_connector() {
        assert!(validate_connectors(&[linear()]).is_ok());
    }

    #[test]
    fn rejects_unknown_fields() {
        let bad = serde_json::from_value::<ConnectorDecl>(json!({
            "id": "x", "name": "X", "surprise": true,
            "oauth": {"clientId":"a","authorizationUrl":"https://a/b","tokenUrl":"https://a/c","scopes":[]},
            "account": {"url":"https://a/d","idPath":"i","namePath":"n"}
        }));
        assert!(bad.is_err());
    }

    #[test]
    fn rejects_bad_ids_and_duplicates() {
        let mut c = linear();
        c.id = "Linear!".into();
        assert!(validate_connectors(&[c]).is_err());
        assert!(validate_connectors(&[linear(), linear()]).is_err());
    }

    #[test]
    fn rejects_non_https_oauth_urls() {
        let mut c = linear();
        c.oauth.as_mut().unwrap().token_url = "http://api.linear.app/oauth/token".into();
        assert!(validate_connectors(&[c]).is_err());
    }

    #[test]
    fn rejects_account_probe_on_a_foreign_host() {
        let mut c = linear();
        c.account.as_mut().unwrap().url = "https://evil.example/graphql".into();
        assert!(validate_connectors(&[c]).is_err());
    }

    #[test]
    fn rejects_unsupported_probe_method_and_empty_paths() {
        let mut c = linear();
        c.account.as_mut().unwrap().method = Some("DELETE".into());
        assert!(validate_connectors(&[c]).is_err());
        let mut c = linear();
        c.account.as_mut().unwrap().id_path = " ".into();
        assert!(validate_connectors(&[c]).is_err());
    }

    fn tasks() -> ConnectorDecl {
        serde_json::from_value(json!({
            "id": "tasks",
            "name": "Tasks",
            "server": {
                "defaultUrl": "http://localhost:4100",
                "scopes": ["tasks"],
                "clientName": "Launcher"
            }
        }))
        .unwrap()
    }

    #[test]
    fn accepts_a_server_connector() {
        assert!(validate_connectors(&[tasks()]).is_ok());
    }

    #[test]
    fn rejects_declaring_both_or_neither_form() {
        let mut both = linear();
        both.server = tasks().server;
        assert!(validate_connectors(&[both]).is_err());
        let mut neither = tasks();
        neither.server = None;
        assert!(validate_connectors(&[neither]).is_err());
        let mut half = linear();
        half.account = None;
        assert!(validate_connectors(&[half]).is_err());
    }

    #[test]
    fn server_default_url_must_be_a_safe_address() {
        let mut t = tasks();
        t.server.as_mut().unwrap().default_url = Some("http://tasks.example.com".into());
        assert!(validate_connectors(&[t]).is_err());
        let mut t = tasks();
        t.server.as_mut().unwrap().default_url = None;
        assert!(validate_connectors(&[t]).is_ok());
        let mut t = tasks();
        t.server.as_mut().unwrap().scopes = vec![" ".into()];
        assert!(validate_connectors(&[t]).is_err());
    }

    #[test]
    fn empty_client_id_is_rejected() {
        let mut c = linear();
        c.oauth.as_mut().unwrap().client_id = " ".into();
        assert!(validate_connectors(&[c]).is_err());
    }
}
