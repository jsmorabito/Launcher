//! Accounts connected through manifest-declared connectors.
//!
//! The OAuth tokens live in `oauth_tokens` (see [`super::token_store`]) under
//! the provider id from [`token_provider_id`]; this table only records which
//! accounts exist and what to call them in the UI.

use crate::error::AppError;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorAccount {
    pub connector_id: String,
    pub account_id: String,
    pub display_name: String,
    pub created_at: i64,
    /// Set for accounts on a user-supplied server (manifest `server` connectors).
    pub server_url: Option<String>,
}

/// What a server-backed account needs to refresh its token without the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountEndpoints {
    pub server_url: String,
    pub client_id: String,
    pub token_url: String,
}

/// The `oauth_tokens` provider id that holds one account's tokens.
pub fn token_provider_id(connector_id: &str, account_id: &str) -> String {
    format!("connector:{connector_id}:{account_id}")
}

pub fn list_accounts(
    conn: &Connection,
    extension_id: &str,
    connector_id: &str,
) -> Result<Vec<ConnectorAccount>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT connector_id, account_id, display_name, created_at, server_url
             FROM connector_accounts
             WHERE extension_id = ?1 AND connector_id = ?2
             ORDER BY created_at, account_id",
        )
        .map_err(db("list connector accounts"))?;
    let rows = stmt
        .query_map(params![extension_id, connector_id], |row| {
            Ok(ConnectorAccount {
                connector_id: row.get(0)?,
                account_id: row.get(1)?,
                display_name: row.get(2)?,
                created_at: row.get(3)?,
                server_url: row.get(4)?,
            })
        })
        .map_err(db("list connector accounts"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(db("read connector accounts"))
}

/// Insert the account, or refresh its display name when it already exists.
/// Returns `true` when the account was already connected.
pub fn upsert_account(
    conn: &Connection,
    extension_id: &str,
    connector_id: &str,
    account_id: &str,
    display_name: &str,
    endpoints: Option<&AccountEndpoints>,
    now: i64,
) -> Result<bool, AppError> {
    let existed = account_exists(conn, extension_id, connector_id, account_id)?;
    conn.execute(
        "INSERT INTO connector_accounts
            (extension_id, connector_id, account_id, display_name, created_at,
             server_url, client_id, token_url)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(extension_id, connector_id, account_id)
         DO UPDATE SET display_name = excluded.display_name,
                       server_url = excluded.server_url,
                       client_id = excluded.client_id,
                       token_url = excluded.token_url",
        params![
            extension_id,
            connector_id,
            account_id,
            display_name,
            now,
            endpoints.map(|e| e.server_url.as_str()),
            endpoints.map(|e| e.client_id.as_str()),
            endpoints.map(|e| e.token_url.as_str()),
        ],
    )
    .map_err(db("save connector account"))?;
    Ok(existed)
}

/// The stored refresh endpoints of a server-backed account. `None` when the
/// account does not exist or belongs to a fixed-provider connector.
pub fn account_endpoints(
    conn: &Connection,
    extension_id: &str,
    connector_id: &str,
    account_id: &str,
) -> Result<Option<AccountEndpoints>, AppError> {
    let row = conn.query_row(
        "SELECT server_url, client_id, token_url FROM connector_accounts
         WHERE extension_id = ?1 AND connector_id = ?2 AND account_id = ?3",
        params![extension_id, connector_id, account_id],
        |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    );
    match row {
        Ok((Some(server_url), Some(client_id), Some(token_url))) => Ok(Some(AccountEndpoints {
            server_url,
            client_id,
            token_url,
        })),
        Ok(_) | Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(db("read connector account")(e)),
    }
}

pub fn account_exists(
    conn: &Connection,
    extension_id: &str,
    connector_id: &str,
    account_id: &str,
) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM connector_accounts
         WHERE extension_id = ?1 AND connector_id = ?2 AND account_id = ?3",
        params![extension_id, connector_id, account_id],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .map_err(db("check connector account"))
}

/// Remove one account row. Returns `true` when a row was deleted.
pub fn delete_account(
    conn: &Connection,
    extension_id: &str,
    connector_id: &str,
    account_id: &str,
) -> Result<bool, AppError> {
    let n = conn
        .execute(
            "DELETE FROM connector_accounts
             WHERE extension_id = ?1 AND connector_id = ?2 AND account_id = ?3",
            params![extension_id, connector_id, account_id],
        )
        .map_err(db("delete connector account"))?;
    Ok(n > 0)
}

/// Drop every account an extension connected (called during uninstall).
pub fn delete_all_for_extension(conn: &Connection, extension_id: &str) -> Result<u64, AppError> {
    let n = conn
        .execute(
            "DELETE FROM connector_accounts WHERE extension_id = ?1",
            params![extension_id],
        )
        .map_err(db("delete connector accounts"))?;
    Ok(n as u64)
}

fn db(action: &'static str) -> impl Fn(rusqlite::Error) -> AppError {
    move |e| AppError::Database(format!("Failed to {action}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::create_test_store;

    #[test]
    fn upsert_lists_accounts_per_connector_in_connect_order() {
        let store = create_test_store();
        let conn = store.conn().unwrap();
        upsert_account(&conn, "ext.a", "linear", "org2", "Beta", None, 20).unwrap();
        upsert_account(&conn, "ext.a", "linear", "org1", "Acme", None, 10).unwrap();
        upsert_account(&conn, "ext.a", "github", "u1", "me", None, 5).unwrap();

        let names: Vec<_> = list_accounts(&conn, "ext.a", "linear")
            .unwrap()
            .into_iter()
            .map(|a| a.display_name)
            .collect();
        assert_eq!(names, ["Acme", "Beta"]);
    }

    #[test]
    fn upsert_reports_existing_and_refreshes_name_keeping_created_at() {
        let store = create_test_store();
        let conn = store.conn().unwrap();
        assert!(!upsert_account(&conn, "ext.a", "linear", "org1", "Old", None, 10).unwrap());
        assert!(upsert_account(&conn, "ext.a", "linear", "org1", "New", None, 99).unwrap());

        let accounts = list_accounts(&conn, "ext.a", "linear").unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].display_name, "New");
        assert_eq!(accounts[0].created_at, 10);
    }

    #[test]
    fn accounts_are_isolated_between_extensions() {
        let store = create_test_store();
        let conn = store.conn().unwrap();
        upsert_account(&conn, "ext.a", "linear", "org1", "A", None, 1).unwrap();
        assert!(list_accounts(&conn, "ext.b", "linear").unwrap().is_empty());
    }

    #[test]
    fn delete_account_reports_whether_a_row_existed() {
        let store = create_test_store();
        let conn = store.conn().unwrap();
        upsert_account(&conn, "ext.a", "linear", "org1", "A", None, 1).unwrap();
        assert!(delete_account(&conn, "ext.a", "linear", "org1").unwrap());
        assert!(!delete_account(&conn, "ext.a", "linear", "org1").unwrap());
    }

    #[test]
    fn delete_all_for_extension_removes_only_that_extension() {
        let store = create_test_store();
        let conn = store.conn().unwrap();
        upsert_account(&conn, "ext.a", "linear", "org1", "A", None, 1).unwrap();
        upsert_account(&conn, "ext.b", "linear", "org1", "B", None, 1).unwrap();
        assert_eq!(delete_all_for_extension(&conn, "ext.a").unwrap(), 1);
        assert_eq!(list_accounts(&conn, "ext.b", "linear").unwrap().len(), 1);
    }

    #[test]
    fn server_backed_accounts_keep_their_endpoints_and_fixed_ones_do_not() {
        let store = create_test_store();
        let conn = store.conn().unwrap();
        let ep = AccountEndpoints {
            server_url: "https://tasks.example.com".into(),
            client_id: "cid".into(),
            token_url: "https://tasks.example.com/oauth/token".into(),
        };
        upsert_account(
            &conn,
            "ext.a",
            "tasks",
            "https://tasks.example.com",
            "tasks.example.com",
            Some(&ep),
            1,
        )
        .unwrap();
        upsert_account(&conn, "ext.a", "linear", "org1", "Acme", None, 1).unwrap();

        assert_eq!(
            account_endpoints(&conn, "ext.a", "tasks", "https://tasks.example.com").unwrap(),
            Some(ep)
        );
        assert_eq!(
            account_endpoints(&conn, "ext.a", "linear", "org1").unwrap(),
            None
        );
        assert_eq!(
            account_endpoints(&conn, "ext.a", "tasks", "missing").unwrap(),
            None
        );

        let listed = list_accounts(&conn, "ext.a", "tasks").unwrap();
        assert_eq!(
            listed[0].server_url.as_deref(),
            Some("https://tasks.example.com")
        );
        assert_eq!(
            list_accounts(&conn, "ext.a", "linear").unwrap()[0].server_url,
            None
        );
    }

    #[test]
    fn reconnecting_updates_the_stored_endpoints() {
        let store = create_test_store();
        let conn = store.conn().unwrap();
        let first = AccountEndpoints {
            server_url: "https://s".into(),
            client_id: "old".into(),
            token_url: "https://s/t".into(),
        };
        let second = AccountEndpoints {
            client_id: "new".into(),
            ..first.clone()
        };
        upsert_account(&conn, "e", "c", "https://s", "s", Some(&first), 1).unwrap();
        assert!(upsert_account(&conn, "e", "c", "https://s", "s", Some(&second), 2).unwrap());
        assert_eq!(
            account_endpoints(&conn, "e", "c", "https://s")
                .unwrap()
                .unwrap()
                .client_id,
            "new"
        );
    }

    #[test]
    fn token_provider_id_is_namespaced_per_connector_and_account() {
        assert_eq!(token_provider_id("linear", "org1"), "connector:linear:org1");
    }
}
