//! Host-driven connector accounts: connect, list, fetch a fresh token, disconnect.
//!
//! The OAuth browser round-trip reuses `oauth_start_flow` / `oauth_exchange_code`
//! with a temporary provider id. [`connector_finalize_connect`] then names the
//! account and moves the tokens to their permanent key.

use crate::deeplink::deep_link_scheme;
use crate::error::AppError;
use crate::oauth::account_probe::{self, AccountProbe};
use crate::oauth::connector_store::{self, AccountEndpoints, ConnectorAccount};
use crate::oauth::{
    dynamic, loopback, pkce, server_request, service, token_store, OAuthPendingFlowState,
    OAuthToken, PendingOAuthFlow,
};
use crate::storage::DataStore;
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, State};

/// How long the localhost listener waits for the browser to come back.
const LOOPBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// What the frontend needs to open the browser and later finish a server connect.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicConnectStart {
    pub state: String,
    pub auth_url: String,
    /// Canonical origin of the server the user typed.
    pub server_url: String,
    pub client_id: String,
    pub token_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorFinalizeResponse {
    pub account: ConnectorAccount,
    /// `true` when this account was already connected; its tokens were refreshed.
    pub already_connected: bool,
}

#[tauri::command]
pub fn connector_list_accounts(
    extension_id: String,
    connector_id: String,
    data_store: State<'_, DataStore>,
) -> Result<Vec<ConnectorAccount>, AppError> {
    let conn = data_store.conn()?;
    connector_store::list_accounts(&conn, &extension_id, &connector_id)
}

/// Complete a connect: identify the account with the declared probe, store the
/// tokens under the account's permanent key, and drop the temporary ones.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn connector_finalize_connect(
    app: AppHandle,
    extension_id: String,
    connector_id: String,
    pending_provider_id: String,
    authorization_url: String,
    token_url: String,
    probe: AccountProbe,
    data_store: State<'_, DataStore>,
) -> Result<ConnectorFinalizeResponse, AppError> {
    account_probe::validate_probe_url(&probe.url, &authorization_url, &token_url)?;

    let token = {
        let conn = data_store.conn()?;
        token_store::get_token(&app, &conn, &extension_id, &pending_provider_id)?
    }
    .ok_or_else(|| AppError::NotFound("No pending connection token".into()))?;

    let identity = account_probe::fetch_identity(&probe, &token.access_token).await;

    let conn = data_store.conn()?;
    // The temporary token is always discarded, success or not.
    let finish = |result: Result<ConnectorFinalizeResponse, AppError>| {
        token_store::delete_token(&conn, &extension_id, &pending_provider_id)?;
        result
    };

    let identity = match identity {
        Ok(identity) => identity,
        Err(e) => return finish(Err(e)),
    };

    let provider_id = connector_store::token_provider_id(&connector_id, &identity.account_id);
    let stored = token_store::store_token(&app, &conn, &extension_id, &provider_id, &token);
    if let Err(e) = stored {
        return finish(Err(e));
    }
    let already_connected = connector_store::upsert_account(
        &conn,
        &extension_id,
        &connector_id,
        &identity.account_id,
        &identity.display_name,
        None,
        service::now_secs(),
    );
    let already_connected = match already_connected {
        Ok(v) => v,
        Err(e) => return finish(Err(e)),
    };

    let account = connector_store::list_accounts(&conn, &extension_id, &connector_id)?
        .into_iter()
        .find(|a| a.account_id == identity.account_id)
        .ok_or_else(|| AppError::Database("Connected account was not saved".into()))?;
    finish(Ok(ConnectorFinalizeResponse {
        account,
        already_connected,
    }))
}

/// A usable access token for one account, refreshing it first when it has
/// expired. Never opens UI: when the account cannot be refreshed the error
/// message is `reconnect_required`.
#[tauri::command]
pub async fn connector_get_token(
    app: AppHandle,
    extension_id: String,
    connector_id: String,
    account_id: String,
    token_url: String,
    client_id: String,
    data_store: State<'_, DataStore>,
) -> Result<String, AppError> {
    fresh_access_token(
        &app,
        &data_store,
        &extension_id,
        &connector_id,
        &account_id,
        (&token_url, &client_id),
    )
    .await
    .map(|(token, _)| token)
}

/// The account's access token, refreshed first when it has expired, plus the
/// stored endpoints of a server-backed account. `fallback` is the manifest's
/// `(token_url, client_id)`, used only by fixed-provider accounts.
async fn fresh_access_token(
    app: &AppHandle,
    data_store: &DataStore,
    extension_id: &str,
    connector_id: &str,
    account_id: &str,
    fallback: (&str, &str),
) -> Result<(String, Option<AccountEndpoints>), AppError> {
    let provider_id = connector_store::token_provider_id(connector_id, account_id);

    let (stored, endpoints): (OAuthToken, Option<AccountEndpoints>) = {
        let conn = data_store.conn()?;
        if !connector_store::account_exists(&conn, extension_id, connector_id, account_id)? {
            return Err(AppError::NotFound("connector_not_connected".into()));
        }
        let token = token_store::get_token(app, &conn, extension_id, &provider_id)?
            .ok_or_else(|| AppError::OAuth("reconnect_required".into()))?;
        (
            token,
            connector_store::account_endpoints(&conn, extension_id, connector_id, account_id)?,
        )
    };
    // A server-backed account registered its own client, so its refresh
    // endpoints come from the account, not from the manifest.
    let (token_url, client_id) = match &endpoints {
        Some(e) => (e.token_url.as_str(), e.client_id.as_str()),
        None => fallback,
    };

    if !service::is_token_expired(&stored) {
        return Ok((stored.access_token, endpoints));
    }

    let refresh_token = stored
        .refresh_token
        .as_deref()
        .ok_or_else(|| AppError::OAuth("reconnect_required".into()))?;
    let refreshed = service::refresh_access_token(token_url, client_id, refresh_token)
        .await
        .map_err(|_| AppError::OAuth("reconnect_required".into()))?;

    let conn = data_store.conn()?;
    token_store::store_token(app, &conn, extension_id, &provider_id, &refreshed)?;
    Ok((refreshed.access_token, endpoints))
}

/// A request to the account's own server, made by the host with the account's
/// token. Only for server-backed accounts; the path must stay on that server.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn connector_fetch(
    app: AppHandle,
    extension_id: String,
    connector_id: String,
    account_id: String,
    path: String,
    method: Option<String>,
    headers: Option<std::collections::HashMap<String, String>>,
    body: Option<String>,
    timeout_ms: Option<u64>,
    data_store: State<'_, DataStore>,
) -> Result<server_request::ServerResponse, AppError> {
    let method = server_request::validate_method(method.as_deref().unwrap_or("GET"))?;
    let (token, endpoints) = fresh_access_token(
        &app,
        &data_store,
        &extension_id,
        &connector_id,
        &account_id,
        ("", ""),
    )
    .await?;
    let endpoints = endpoints.ok_or_else(|| {
        AppError::Validation("connector_not_server: this connector has no server address".into())
    })?;
    let url = server_request::build_url(&endpoints.server_url, &path)?;
    server_request::send(
        &url,
        method,
        &token,
        server_request::allowed_headers(headers.unwrap_or_default()),
        body,
        server_request::clamp_timeout(timeout_ms),
    )
    .await
}

#[tauri::command]
pub fn connector_disconnect(
    extension_id: String,
    connector_id: String,
    account_id: String,
    data_store: State<'_, DataStore>,
) -> Result<(), AppError> {
    let conn = data_store.conn()?;
    let provider_id = connector_store::token_provider_id(&connector_id, &account_id);
    token_store::delete_token(&conn, &extension_id, &provider_id)?;
    connector_store::delete_account(&conn, &extension_id, &connector_id, &account_id)?;
    Ok(())
}

/// Start connecting to a user-supplied server: discover its sign-in endpoints,
/// register Launcher as a client for a fresh localhost redirect, and begin the
/// PKCE flow. The localhost listener delivers the browser's redirect through the
/// same `launcher:deep-link` event the custom-scheme flow uses.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn connector_begin_dynamic(
    app: AppHandle,
    extension_id: String,
    pending_provider_id: String,
    flow_id: String,
    server_url: String,
    scopes: Vec<String>,
    client_name: String,
    pending_flows: State<'_, OAuthPendingFlowState>,
) -> Result<DynamicConnectStart, AppError> {
    let origin = dynamic::normalize_server_url(&server_url)?;
    let metadata = dynamic::discover(&origin).await?;

    let (listener, port) = loopback::bind().await?;
    let redirect_uri = loopback::redirect_uri(port);
    let client_id = dynamic::register_client(
        &metadata.registration_endpoint,
        &client_name,
        &redirect_uri,
        &scopes,
    )
    .await?;

    let (code_verifier, code_challenge) = pkce::generate_pkce_pair();
    let state = pkce::generate_state();
    let auth_url = pkce::build_auth_url(
        &metadata.authorization_endpoint,
        &client_id,
        &code_challenge,
        &state,
        &scopes,
        &redirect_uri,
    )?;

    pending_flows.insert(
        state.clone(),
        PendingOAuthFlow {
            extension_id,
            flow_id,
            provider_id: pending_provider_id,
            code_verifier,
            token_url: metadata.token_endpoint.clone(),
            client_id: client_id.clone(),
            redirect_uri,
        },
    );

    let scheme = deep_link_scheme(&app);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        match loopback::wait_for_callback(listener, LOOPBACK_TIMEOUT).await {
            Ok(query) => {
                let url = format!("{scheme}://oauth/callback?{query}");
                crate::event_bridge::bridge_emit(&handle, "launcher:deep-link", url);
            }
            Err(e) => log::warn!("[connectors] localhost callback ended: {e}"),
        }
    });

    Ok(DynamicConnectStart {
        state,
        auth_url,
        server_url: origin,
        client_id,
        token_url: metadata.token_endpoint,
    })
}

/// Complete a server connect after the tokens were stored under the pending id:
/// the account is the server itself.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn connector_finalize_dynamic(
    app: AppHandle,
    extension_id: String,
    connector_id: String,
    pending_provider_id: String,
    server_url: String,
    client_id: String,
    token_url: String,
    data_store: State<'_, DataStore>,
) -> Result<ConnectorFinalizeResponse, AppError> {
    let conn = data_store.conn()?;
    let token = token_store::get_token(&app, &conn, &extension_id, &pending_provider_id)?
        .ok_or_else(|| AppError::NotFound("No pending connection token".into()))?;
    let result = (|| {
        let account_id = dynamic::normalize_server_url(&server_url)?;
        let provider_id = connector_store::token_provider_id(&connector_id, &account_id);
        token_store::store_token(&app, &conn, &extension_id, &provider_id, &token)?;
        let endpoints = AccountEndpoints {
            server_url: account_id.clone(),
            client_id,
            token_url,
        };
        let already_connected = connector_store::upsert_account(
            &conn,
            &extension_id,
            &connector_id,
            &account_id,
            &dynamic::display_name(&account_id),
            Some(&endpoints),
            service::now_secs(),
        )?;
        let account = connector_store::list_accounts(&conn, &extension_id, &connector_id)?
            .into_iter()
            .find(|a| a.account_id == account_id)
            .ok_or_else(|| AppError::Database("Connected account was not saved".into()))?;
        Ok(ConnectorFinalizeResponse {
            account,
            already_connected,
        })
    })();
    // The temporary token is always discarded, success or not.
    token_store::delete_token(&conn, &extension_id, &pending_provider_id)?;
    result
}
