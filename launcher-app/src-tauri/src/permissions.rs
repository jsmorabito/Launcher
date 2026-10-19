use crate::error::AppError;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// Rust-side extension permission registry. Populated by extensionManager.ts at extension
/// load time. Queried by sensitive commands for defense-in-depth enforcement.
///
/// `inner` stores the declared permission *strings* (flag permissions like `network`,
/// `clipboard:read`, `fs:watch`). `args` stores the sidecar value bag for
/// parameterized permissions — currently only `fs:watch` (value: `Array<String>`
/// of glob patterns), but the shape is intentionally generic so future
/// parameterized permissions reuse the same store.
pub struct ExtensionPermissionRegistry {
    pub inner: Mutex<HashMap<String, HashSet<String>>>,
    pub args: Mutex<HashMap<String, HashMap<String, serde_json::Value>>>,
    pub secret_runtime: std::sync::Arc<crate::network::secret_refs::SecretRuntime>,
}

impl Default for ExtensionPermissionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod secret_tests {
    use super::*;
    #[test]
    fn fetch_with_secrets_maps_to_network_secrets() {
        assert_eq!(
            get_required_permission("launcher:api:network:fetchWithSecrets"),
            Some("network:secrets")
        );
        assert_eq!(
            gate_decision("launcher:api:network:fetchWithSecrets"),
            GateDecision::Require("network:secrets")
        );
    }
    #[test]
    fn secret_refs_lookup_is_scoped_to_caller() {
        let r = ExtensionPermissionRegistry::new();
        let args = serde_json::json!([{"name":"cli-token","sources":[{"type":"jsonFile","path":"~/.cli/a.json","pointer":"/token"}],"hosts":["api.example.com"],"header":"Authorization","template":"Bearer {secret}"}]);
        r.register(
            "a",
            HashSet::from(["network".into(), "network:secrets".into()]),
            HashMap::from([("network:secrets".into(), args)]),
        );
        r.register("b", HashSet::from(["network".into()]), HashMap::new());
        assert_eq!(r.secret_refs("a").unwrap()[0].name, "cli-token");
        assert!(r.secret_refs("b").is_err());
        assert!(r.secret_refs("unknown").is_err());
    }
}

impl ExtensionPermissionRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            args: Mutex::new(HashMap::new()),
            secret_runtime: std::sync::Arc::new(
                crate::network::secret_refs::SecretRuntime::default(),
            ),
        }
    }

    /// Returns Ok(()) if allowed. None or "launcher" caller = system/core call, always allowed.
    pub fn check(
        &self,
        caller_extension_id: &Option<String>,
        required_permission: &str,
    ) -> Result<(), AppError> {
        let Some(id) = caller_extension_id else {
            return Ok(());
        };
        if id == "launcher" {
            return Ok(());
        }
        let registry = self.inner.lock().map_err(|_| AppError::Lock)?;
        let permissions = registry.get(id).ok_or_else(|| {
            AppError::Permission(format!(
                "Extension \"{}\" is not registered in the Rust permission registry.",
                id
            ))
        })?;
        if permissions.contains(required_permission) {
            Ok(())
        } else {
            Err(AppError::Permission(format!(
                "Extension \"{}\" requires the \"{}\" permission.",
                id, required_permission
            )))
        }
    }

    /// Register (or replace) an extension's permission set + its arg bag.
    /// Preferred over writing `inner` / `args` directly — keeps both in sync.
    pub fn register(
        &self,
        extension_id: &str,
        permissions: HashSet<String>,
        permission_args: HashMap<String, serde_json::Value>,
    ) {
        if let Ok(mut p) = self.inner.lock() {
            p.insert(extension_id.to_string(), permissions);
        }
        if let Ok(mut a) = self.args.lock() {
            a.insert(extension_id.to_string(), permission_args);
        }
    }

    /// Remove an extension from the registry. Used when the extension is
    /// uninstalled or disabled.
    pub fn unregister(&self, extension_id: &str) {
        if let Ok(mut p) = self.inner.lock() {
            p.remove(extension_id);
        }
        if let Ok(mut a) = self.args.lock() {
            a.remove(extension_id);
        }
    }

    /// Generic arg lookup. Returns a cloned JSON value for the (extension,
    /// permission) pair, or `None` if absent. Callers narrow to the expected
    /// shape (e.g. `fs_watch_patterns` for `Vec<String>`).
    pub fn args_for(&self, extension_id: &str, permission: &str) -> Option<serde_json::Value> {
        let guard = self.args.lock().ok()?;
        guard.get(extension_id)?.get(permission).cloned()
    }

    /// Narrowed typed view of a `string[]` arg bag (the shape shared by
    /// `fs:watch` and `files:read` patterns). Errors if the extension
    /// hasn't declared the permission.
    pub fn string_array_args(
        &self,
        extension_id: &str,
        permission: &str,
    ) -> Result<Vec<String>, AppError> {
        let value = self.args_for(extension_id, permission).ok_or_else(|| {
            AppError::Permission(format!(
                "Extension '{}' has no {} patterns declared in manifest",
                extension_id, permission
            ))
        })?;
        let arr = value.as_array().ok_or_else(|| {
            AppError::Validation(format!(
                "permissionArgs.{} must be an array of strings",
                permission
            ))
        })?;
        arr.iter()
            .map(|v| {
                v.as_str().map(|s| s.to_string()).ok_or_else(|| {
                    AppError::Validation(format!(
                        "permissionArgs.{} entries must be strings",
                        permission
                    ))
                })
            })
            .collect()
    }

    /// Narrowed typed view of `fs:watch` patterns. Errors if the extension
    /// hasn't declared the permission.
    pub fn fs_watch_patterns(&self, extension_id: &str) -> Result<Vec<String>, AppError> {
        self.string_array_args(extension_id, "fs:watch")
    }

    /// Narrowed typed view of `files:read` patterns. Errors if the extension
    /// hasn't declared the permission.
    pub fn files_read_patterns(&self, extension_id: &str) -> Result<Vec<String>, AppError> {
        self.string_array_args(extension_id, "files:read")
    }
    pub fn secret_refs(
        &self,
        extension_id: &str,
    ) -> Result<Vec<crate::network::secret_refs::SecretRefDecl>, AppError> {
        self.check(&Some(extension_id.into()), "network")?;
        self.check(&Some(extension_id.into()), "network:secrets")?;
        let value = self
            .args_for(extension_id, "network:secrets")
            .ok_or(crate::network::secret_refs::SecretError::InvalidDeclaration)?;
        crate::network::secret_refs::validate_network_secrets_args(&value)
    }
}

/// Maps an IPC call type string to the required permission.
/// Returns None if the call type is a core call that doesn't require a permission.
fn get_required_permission(call_type: &str) -> Option<&'static str> {
    match call_type {
        // Clipboard: current clipboard read/write
        "launcher:api:clipboard:readCurrentClipboard" => Some("clipboard:read"),
        "launcher:api:clipboard:readCurrentText" => Some("clipboard:read"),
        "launcher:api:clipboard:writeToClipboard" => Some("clipboard:write"),
        "launcher:api:clipboard:pasteItem" => Some("clipboard:write"),
        "launcher:api:clipboard:simulatePaste" => Some("clipboard:write"),
        // Clipboard History: stored history query
        "launcher:api:clipboard:getRecentItems" => Some("clipboard-history:read"),
        "launcher:api:clipboard:searchHistory" => Some("clipboard-history:read"),
        // Clipboard History: background capture subscription
        "launcher:api:clipboard:subscribeCapture" => Some("clipboard-history:capture"),
        "launcher:api:clipboard:unsubscribeCapture" => Some("clipboard-history:capture"),
        // Clipboard History: deletion and management
        "launcher:api:clipboard:toggleItemFavorite" => Some("clipboard-history:manage"),
        "launcher:api:clipboard:deleteItem" => Some("clipboard-history:manage"),
        "launcher:api:clipboard:clearNonFavorites" => Some("clipboard-history:manage"),
        // Notifications
        "launcher:api:feedback:sendBackground" => Some("notifications:send"),
        "launcher:api:feedback:dismissBackground" => Some("notifications:send"),
        "launcher:api:feedback:announce" => Some("feedback:announce"),
        // Raw Tauri invoke
        "launcher:api:invoke" => Some("shell:spawn"),
        // Network
        "launcher:api:network:fetch" => Some("network"),
        "launcher:api:network:fetchWithSecrets" => Some("network:secrets"),
        "launcher:api:network:wsConnect" => Some("network"),
        "launcher:api:network:wsSend" => Some("network"),
        "launcher:api:network:wsClose" => Some("network"),
        // Opener
        "launcher:api:opener:open" => Some("shell:open-url"),
        "launcher:api:opener:openPath" => Some("shell:open-path"),
        "launcher:api:opener:reveal" => Some("fs:read"),
        "launcher:api:fs:showInFileManager" => Some("fs:read"),
        "launcher:api:fs:trash" => Some("fs:write"),
        "launcher:api:shell:spawn" => Some("shell:spawn"),
        "launcher:api:shell:list" => Some("shell:spawn"),
        "launcher:api:shell:attach" => Some("shell:spawn"),
        "launcher:api:shell:write-stdin" => Some("shell:spawn"),
        "launcher:api:shell:close-stdin" => Some("shell:spawn"),
        // Entitlement service — requires subscription read permission
        "launcher:api:entitlements:check" => Some("entitlements:read"),
        "launcher:api:entitlements:getAll" => Some("entitlements:read"),
        // Extension storage
        "launcher:api:storage:get" => Some("storage:read"),
        "launcher:api:storage:set" => Some("storage:write"),
        "launcher:api:storage:delete" => Some("storage:write"),
        "launcher:api:storage:getAll" => Some("storage:read"),
        "launcher:api:storage:clear" => Some("storage:write"),
        // Extension cache
        "launcher:api:cache:get" => Some("cache:read"),
        "launcher:api:cache:set" => Some("cache:write"),
        "launcher:api:cache:delete" => Some("cache:write"),
        "launcher:api:cache:clear" => Some("cache:write"),
        // Screen sampling (eyedropper) and OCR capture
        "launcher:api:screen:pickColor" => Some("screen:pick-color"),
        "launcher:api:screen:captureText" => Some("screen:capture"),
        // Calculator evaluation
        "launcher:api:calculator:evaluate" => Some("calculator:evaluate"),
        // Selection
        "launcher:api:selection:getSelectedText" => Some("selection:read"),
        "launcher:api:selection:getSelectedFinderItems" => Some("selection:read"),
        // OAuth PKCE for extensions
        "launcher:api:oauth:authorize" => Some("oauth:use"),
        "launcher:api:oauth:revokeToken" => Some("oauth:use"),
        // Inter-extension command invocation
        "launcher:api:interop:launchCommand" => Some("extension:invoke"),
        // Application Service
        "launcher:api:application:getFrontmostApplication" => Some("application:read"),
        "launcher:api:application:syncApplicationIndex" => Some("application:read"),
        "launcher:api:application:listApplications" => Some("application:read"),
        // Window Management
        "launcher:api:window:getWindowBounds" => Some("window:manage"),
        "launcher:api:window:setWindowBounds" => Some("window:manage"),
        "launcher:api:window:setFullscreen" => Some("window:manage"),
        "launcher:api:window:applyPreset" => Some("window:manage"),
        // Extension Preferences
        "launcher:api:preferences:getAll" => Some("preferences:read"),
        "launcher:api:preferences:set" => Some("preferences:write"),
        "launcher:api:preferences:reset" => Some("preferences:write"),
        // Power inhibitor
        "launcher:api:power:keepAwake" => Some("power:inhibit"),
        "launcher:api:power:release" => Some("power:inhibit"),
        "launcher:api:power:list" => Some("power:inhibit"),
        // Process service (list/kill)
        "launcher:api:process:list" => Some("process:read"),
        "launcher:api:process:kill" => Some("process:kill"),
        // System events (OS sleep/wake/lid/battery push)
        "launcher:api:systemEvents:subscribe" => Some("systemEvents:read"),
        "launcher:api:systemEvents:unsubscribe" => Some("systemEvents:read"),
        // App-presence push events (launched / terminated / frontmost-changed)
        "launcher:api:appEvents:subscribe" => Some("app:frontmost-watch"),
        "launcher:api:appEvents:unsubscribe" => Some("app:frontmost-watch"),
        // Application one-shot query — same permission as the rest of application:*
        "launcher:api:application:isRunning" => Some("application:read"),
        // Persistent one-shot timers (fire even after relaunch).
        "launcher:api:timers:schedule" => Some("timers:schedule"),
        "launcher:api:timers:cancel" => Some("timers:cancel"),
        "launcher:api:timers:list" => Some("timers:list"),
        // Filesystem watcher — patterns are sidecarred in permissionArgs.
        "launcher:api:fsWatcher:create" => Some("fs:watch"),
        "launcher:api:fsWatcher:dispose" => Some("fs:watch"),
        // Snippets — extension contributes shortcodes to the snippets service
        "launcher:api:snippets:registerShortcodes" => Some("snippets:contribute"),
        "launcher:api:snippets:unregisterShortcodes" => Some("snippets:contribute"),
        // Browser bridge — tabs (read = list/inspect, write = act/open/search).
        "launcher:api:browser:listTabs" => Some("browser:tabs.read"),
        "launcher:api:browser:getActiveTab" => Some("browser:tabs.read"),
        "launcher:api:browser:listPairedBrowsers" => Some("browser:tabs.read"),
        "launcher:api:browser:getMostRecentActiveBrowser" => Some("browser:tabs.read"),
        "launcher:api:browser:subscribeTabsChanged" => Some("browser:tabs.read"),
        "launcher:api:browser:unsubscribeTabsChanged" => Some("browser:tabs.read"),
        "launcher:api:browser:activateTab" => Some("browser:tabs.write"),
        "launcher:api:browser:closeTab" => Some("browser:tabs.write"),
        "launcher:api:browser:openUrl" => Some("browser:tabs.write"),
        "launcher:api:browser:searchWeb" => Some("browser:tabs.write"),
        // Browser bridge — bookmarks / history (read from disk).
        "launcher:api:browser:listBookmarks" => Some("browser:bookmarks.read"),
        "launcher:api:browser:searchHistory" => Some("browser:history.read"),
        // Browser bridge — page content (read = snapshot/query, write = act).
        "launcher:api:browser:getCurrentPage" => Some("browser:page.read"),
        "launcher:api:browser:queryPage" => Some("browser:page.read"),
        "launcher:api:browser:subscribePageChanged" => Some("browser:page.read"),
        "launcher:api:browser:unsubscribePageChanged" => Some("browser:page.read"),
        "launcher:api:browser:actOnPage" => Some("browser:page.write"),
        // File search — one permission covers both read methods; the local
        // index is read-only, no separate write surface.
        "launcher:api:files:search" => Some("files:search"),
        "launcher:api:files:status" => Some("files:search"),
        // File content read — separate permission from the index search;
        // scope is in permissionArgs.files:read (glob patterns).
        "launcher:api:files:read" => Some("files:read"),
        "launcher:api:files:readRange" => Some("files:read"),
        // Scoped enumeration and thumbnails ride the same permission and
        // the same declared globs: listing in-scope names and rendering an
        // in-scope file as an icon are both strictly less information than
        // the byte read files:read already grants.
        "launcher:api:files:glob" => Some("files:read"),
        "launcher:api:files:globEntries" => Some("files:read"),
        "launcher:api:files:thumbnail" => Some("files:read"),
        // Notes SDK service — third-party extensions can search/read/create/
        // append to the user's Notes (never update-in-place or delete an
        // arbitrary note by id, so a buggy or malicious extension cannot
        // silently corrupt or destroy the user's own notes; see
        // NotesServiceProxy for the full rationale).
        "launcher:api:notes:search" => Some("notes:read"),
        "launcher:api:notes:list" => Some("notes:read"),
        "launcher:api:notes:get" => Some("notes:read"),
        "launcher:api:notes:create" => Some("notes:write"),
        "launcher:api:notes:append" => Some("notes:write"),
        // Agent runs — extension-owned Tier 2 run tracking. Previously ungated
        // (fail-open hole): any extension could start/write/cancel runs without
        // declaring anything. Mirrors the JS PERMISSION_MAP's runs:track.
        "launcher:api:runs:start"
        | "launcher:api:runs:write"
        | "launcher:api:runs:done"
        | "launcher:api:runs:fail"
        | "launcher:api:runs:cancel" => Some("runs:track"),
        // Agent tools — registering/listing Tier 2 tools in the agent runtime.
        // Previously ungated (fail-open hole). Mirrors the JS PERMISSION_MAP's
        // tools:register.
        "launcher:api:tools:registerTool"
        | "launcher:api:tools:unregisterTool"
        | "launcher:api:tools:listTools" => Some("tools:register"),
        // Model Context Protocol (MCP) client: discover servers, inspect tools, invoke tools
        "launcher:api:mcp:listServers" | "launcher:api:mcp:listTools" | "launcher:api:mcp:invokeTool" => {
            Some("mcp")
        }
        // AI Model Prompting — Tier 2 extensions prompting configured AI providers / spending tokens.
        "launcher:api:ai:streamChat" | "launcher:api:ai:complete" => Some("ai"),
        // browser:listAvailableBrowsers / isCompanionInstalled are intentionally
        // permission-free (discovery, low blast radius).
        // search:rank is intentionally permission-free: the caller supplies its
        // own already-known items and gets back an ordering — no host data is
        // read, nothing is persisted, no cross-extension exposure.
        // clipboard:stripHtml / clipboard:stripRtf are intentionally
        // permission-free for the same reason: the caller supplies its own
        // markup string and gets back plain text — the host clipboard is
        // never read (that's clipboard:read, gated separately), nothing is
        // persisted, no cross-extension exposure.
        //
        // A `None` here does NOT mean "always allowed" — it only means "not
        // gated". The permission-free calls above are listed explicitly in
        // `is_public_call`; `gate_decision` requires a `None` call to be an
        // enumerated public call, else it is DENIED (fail closed).
        _ => None,
    }
}

// ── Fail-closed gate classification ──────────────────────────────────────────
//
// Every call type an extension can invoke is exactly one of three things:
//   1. gated  — needs a declared manifest permission (`get_required_permission`)
//   2. public — deliberately permission-free (`is_public_call`)
//   3. neither — a bug or a not-yet-classified new API → DENIED by default.
//
// This turns "someone added a privileged API and forgot to gate it" into a
// loud denial in development instead of a silent grant in production.

#[derive(Debug, PartialEq, Eq)]
enum GateDecision {
    /// Permission-free surface — allow without consulting the registry.
    AllowPublic,
    /// Requires the given manifest permission.
    Require(&'static str),
    /// Unrecognized call type — deny by default (fail closed).
    Deny,
}

/// Call types deliberately exposed to extensions WITHOUT any permission:
/// local logging, pure compute, discovery, and UI plumbing that read no host
/// data, persist nothing, and cannot reach another extension's state.
///
/// This is the ONLY permission-free surface. Anything absent from both this
/// list and `get_required_permission` is denied by `gate_decision` (fail
/// closed), so keep it exhaustive: a new permission-free call must be added
/// here explicitly, on purpose.
fn is_public_call(call_type: &str) -> bool {
    matches!(
        call_type,
        // Local logging — never leaves the host, reads no data.
        "launcher:api:log:debug"
            | "launcher:api:log:info"
            | "launcher:api:log:warn"
            | "launcher:api:log:error"
            | "launcher:api:log:custom"
        // UI plumbing for the extension's own surface: result actions, its own
        // view navigation/labels, the search-bar accessory, the status-bar
        // item. All scoped to the calling extension — no host data read,
        // nothing persisted, no cross-extension reach.
            | "launcher:api:actions:registerActionHandler"
            | "launcher:api:actions:registerAction"
            | "launcher:api:actions:unregisterAction"
            | "launcher:api:actions:setContext"
            | "launcher:api:actions:executeAction"
            | "launcher:api:extensions:navigateToView"
            | "launcher:api:extensions:goBack"
            | "launcher:api:extensions:forwardKeyToActiveView"
            | "launcher:api:extensions:setActiveViewActionLabel"
            | "launcher:api:extensions:setActiveViewSubtitle"
            | "launcher:api:searchBar:set"
            | "launcher:api:searchBar:clear"
            | "launcher:api:statusBar:registerItem"
            | "launcher:api:statusBar:unregisterItem"
            | "launcher:api:statusBar:updateItem"
        // Dynamic commands — the extension manages its OWN command surface
        // (registration, metadata, dynamic replacement). Scoped to the caller.
            | "launcher:api:commands:registerCommand"
            | "launcher:api:commands:unregisterCommand"
            | "launcher:api:commands:replaceDynamicCommands"
            | "launcher:api:commands:updateCommandMetadata"
            | "launcher:api:commands:clearCommandsForExtension"
        // Worker<->view shared state + RPC, scoped to the calling extension
        // (state is in INJECTS_EXTENSION_ID) — internal plumbing, no host data
        // read, nothing persisted host-side, no cross-extension reach.
            | "launcher:api:state:get"
            | "launcher:api:state:set"
            | "launcher:api:state:subscribe"
            | "launcher:api:state:unsubscribe"
            | "launcher:api:state:rpcRequest"
            | "launcher:api:state:rpcReply"
            | "launcher:api:state:rpcAbort"
        // Ephemeral, extension-scoped feedback UI — toast / progress / HUD /
        // confirm dialog. Transient, scoped to the caller; nothing persisted.
        // (Notification-like feedback — announce / sendBackground — stays gated.)
            | "launcher:api:feedback:report"
            | "launcher:api:feedback:showProgress"
            | "launcher:api:feedback:updateProgress"
            | "launcher:api:feedback:finishProgress"
            | "launcher:api:feedback:showHUD"
            | "launcher:api:feedback:dismiss"
            | "launcher:api:feedback:confirmAlert"
        // Pure compute — caller supplies its own input, gets a transform back.
            | "launcher:api:search:rank"
            | "launcher:api:clipboard:stripHtml"
            | "launcher:api:clipboard:stripRtf"
        // Discovery — low blast radius, no host data.
            | "launcher:api:browser:listAvailableBrowsers"
            | "launcher:api:browser:isCompanionInstalled"
        // Read-only monitor geometry.
            | "launcher:api:window:getMonitors"
        // Push-subscription plumbing; the pushed payloads are themselves gated
        // at their read sites. TODO(policy): revisit whether these warrant a
        // scoped permission of their own.
            | "launcher:api:applicationIndex:subscribe"
            | "launcher:api:applicationIndex:unsubscribe"
            | "launcher:api:browser:subscribeEvents"
            | "launcher:api:browser:unsubscribeEvents"
        // An extension marking its OWN per-extension onboarding complete
        // (extension_id is host-injected from the trusted iframe attribute,
        // never the payload — see INJECTS_EXTENSION_ID) — scoped to the
        // caller, like state:*/feedback:*.
            | "launcher:api:onboarding:complete"
        // Read-only host environment and locale metadata.
            | "launcher:api:environment:getEnvironment"
    )
}

/// Decides how `call_type` is gated. Fail closed: a call in neither
/// `get_required_permission` nor `is_public_call` is denied.
fn gate_decision(call_type: &str) -> GateDecision {
    match get_required_permission(call_type) {
        Some(perm) => GateDecision::Require(perm),
        None if is_public_call(call_type) => GateDecision::AllowPublic,
        None => GateDecision::Deny,
    }
}

/// Result of a permission check, returned to the frontend.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCheckResult {
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_permission: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Called by the frontend IPC router to check if an extension is allowed to make a specific call.
/// This is the single enforcement point for extension permissions.
#[tauri::command]
pub fn check_extension_permission(
    extension_id: String,
    call_type: String,
    registry: tauri::State<'_, ExtensionPermissionRegistry>,
) -> PermissionCheckResult {
    let required = match gate_decision(&call_type) {
        // Deliberately permission-free call — allow without the registry.
        GateDecision::AllowPublic => {
            return PermissionCheckResult {
                allowed: true,
                required_permission: None,
                reason: None,
            };
        }
        // Unrecognized call type — fail closed. A call that is neither gated
        // nor on the public allowlist is refused by default, so a new
        // privileged API that nobody classified can never be silently reached.
        GateDecision::Deny => {
            return PermissionCheckResult {
                allowed: false,
                required_permission: None,
                reason: Some(format!(
                    "Call \"{}\" is not a recognized extension API — refusing by default (fail closed).",
                    call_type
                )),
            };
        }
        GateDecision::Require(perm) => perm,
    };

    let reg = match registry.inner.lock() {
        Ok(r) => r,
        Err(_) => {
            return PermissionCheckResult {
                allowed: false,
                required_permission: Some(required.to_string()),
                reason: Some("Internal error: permission registry lock failed".to_string()),
            };
        }
    };

    let permissions = match reg.get(&extension_id) {
        Some(p) => p,
        None => {
            return PermissionCheckResult {
                allowed: false,
                required_permission: Some(required.to_string()),
                reason: Some(format!(
                    "Extension \"{}\" is not registered in the permission registry.",
                    extension_id
                )),
            };
        }
    };

    if permissions.contains(required) {
        PermissionCheckResult {
            allowed: true,
            required_permission: None,
            reason: None,
        }
    } else {
        PermissionCheckResult {
            allowed: false,
            required_permission: Some(required.to_string()),
            reason: Some(format!(
                "Extension \"{}\" called \"{}\" but did not declare permission \"{}\" in its manifest.json",
                extension_id, call_type, required
            )),
        }
    }
}

/// Outcome of a registration attempt, returned to the frontend loader.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRegistrationResult {
    pub registered: bool,
    pub needs_consent: bool,
}

/// Resolves the declared permission set trusted by `register_extension_permissions`.
/// When the extension is already discovered, its manifest on disk is
/// authoritative and the caller-supplied `permissions`/`permission_args` are
/// ignored entirely — this is what stops a compromised caller from forging a
/// larger permission set than the extension's own manifest declares. The
/// caller-supplied values are only trusted when no discovery record exists.
fn resolve_declared_permissions(
    extensions: &HashMap<String, crate::extensions::ExtensionRecord>,
    extension_id: &str,
    caller_permissions: Vec<String>,
    caller_permission_args: Option<serde_json::Map<String, serde_json::Value>>,
) -> (
    bool,
    Vec<String>,
    serde_json::Map<String, serde_json::Value>,
) {
    match extensions.get(extension_id) {
        Some(record) => (
            record.is_built_in,
            record.manifest.permissions.clone().unwrap_or_default(),
            record.manifest.permission_args.clone().unwrap_or_default(),
        ),
        None => (
            false,
            caller_permissions,
            caller_permission_args.unwrap_or_default(),
        ),
    }
}

/// Called by extensionManager.ts when an extension loads. Stores the extension's
/// declared permission strings + their sidecar arguments so sensitive Rust
/// commands can check them.
///
/// Consent backstop: the declared set only enters the registry when a covering
/// consent record exists (see `extensions::consent`). Otherwise any stale
/// entry is cleared and `needs_consent` is reported, so every gated call fails
/// closed until the user accepts. The declared set is resolved from the
/// discovered manifest when the extension is known — the caller-supplied
/// values are only trusted when no discovery record exists.
#[tauri::command]
pub fn register_extension_permissions(
    app_handle: tauri::AppHandle,
    extension_id: String,
    permissions: Vec<String>,
    permission_args: Option<serde_json::Map<String, serde_json::Value>>,
    registry: tauri::State<'_, ExtensionPermissionRegistry>,
    extensions: tauri::State<'_, crate::extensions::ExtensionRegistryState>,
) -> Result<PermissionRegistrationResult, AppError> {
    if extension_id.trim().is_empty() {
        return Err(AppError::Validation(
            "extension_id cannot be empty".to_string(),
        ));
    }

    let (is_built_in, declared_perms, declared_args) = {
        let reg = extensions.extensions.lock().map_err(|_| AppError::Lock)?;
        resolve_declared_permissions(&reg, &extension_id, permissions, permission_args)
    };

    let consent = crate::extensions::consent::get_consent(&app_handle, &extension_id)?;
    match crate::extensions::consent::registration_decision(
        is_built_in,
        &declared_perms,
        &declared_args,
        consent.as_ref(),
    ) {
        crate::extensions::consent::RegistrationDecision::Register => {
            let perms: HashSet<String> = declared_perms.into_iter().collect();
            let args: HashMap<String, serde_json::Value> = declared_args.into_iter().collect();
            registry.register(&extension_id, perms, args);
            Ok(PermissionRegistrationResult {
                registered: true,
                needs_consent: false,
            })
        }
        crate::extensions::consent::RegistrationDecision::WithholdNeedsConsent => {
            registry.unregister(&extension_id);
            log::warn!(
                "Withholding permission registration for '{}': declared permissions exceed recorded consent",
                extension_id
            );
            Ok(PermissionRegistrationResult {
                registered: false,
                needs_consent: true,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry() -> ExtensionPermissionRegistry {
        let reg = ExtensionPermissionRegistry::new();
        let mut inner = reg.inner.lock().unwrap();
        inner.insert("org.launcher.test".to_string(), {
            let mut s = HashSet::new();
            s.insert("network".to_string());
            s.insert("clipboard:read".to_string());
            s
        });
        drop(inner);
        reg
    }

    #[test]
    fn test_get_required_permission_known_call() {
        assert_eq!(
            get_required_permission("launcher:api:network:fetch"),
            Some("network")
        );
    }

    #[test]
    fn test_get_required_permission_opener() {
        assert_eq!(
            get_required_permission("launcher:api:opener:open"),
            Some("shell:open-url")
        );
        assert_eq!(
            get_required_permission("launcher:api:opener:openPath"),
            Some("shell:open-path")
        );
        assert_eq!(
            get_required_permission("launcher:api:opener:reveal"),
            Some("fs:read")
        );
    }

    #[test]
    fn test_get_required_permission_unknown_call() {
        assert_eq!(get_required_permission("launcher:api:log:info"), None);
    }

    #[test]
    fn test_get_required_permission_files() {
        assert_eq!(
            get_required_permission("launcher:api:files:search"),
            Some("files:search")
        );
        assert_eq!(
            get_required_permission("launcher:api:files:status"),
            Some("files:search")
        );
        assert_eq!(
            get_required_permission("launcher:api:files:read"),
            Some("files:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:files:readRange"),
            Some("files:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:files:glob"),
            Some("files:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:files:globEntries"),
            Some("files:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:files:thumbnail"),
            Some("files:read")
        );
    }

    #[test]
    fn test_get_required_permission_notes() {
        assert_eq!(
            get_required_permission("launcher:api:notes:search"),
            Some("notes:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:notes:list"),
            Some("notes:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:notes:get"),
            Some("notes:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:notes:create"),
            Some("notes:write")
        );
        assert_eq!(
            get_required_permission("launcher:api:notes:append"),
            Some("notes:write")
        );
    }

    #[test]
    fn test_get_required_permission_process() {
        assert_eq!(
            get_required_permission("launcher:api:process:list"),
            Some("process:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:process:kill"),
            Some("process:kill")
        );
    }

    #[test]
    fn test_browser_calls_require_their_permission() {
        // Regression guard: browser IPC commands MUST be gated. If any returns
        // None it defaults to "core call, always allowed" — letting any extension
        // read tabs/bookmarks/history/page content without declaring permission.
        assert_eq!(
            get_required_permission("launcher:api:browser:listTabs"),
            Some("browser:tabs.read")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:getActiveTab"),
            Some("browser:tabs.read")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:listPairedBrowsers"),
            Some("browser:tabs.read")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:getMostRecentActiveBrowser"),
            Some("browser:tabs.read")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:activateTab"),
            Some("browser:tabs.write")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:closeTab"),
            Some("browser:tabs.write")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:openUrl"),
            Some("browser:tabs.write")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:searchWeb"),
            Some("browser:tabs.write")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:listBookmarks"),
            Some("browser:bookmarks.read")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:searchHistory"),
            Some("browser:history.read")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:getCurrentPage"),
            Some("browser:page.read")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:queryPage"),
            Some("browser:page.read")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:actOnPage"),
            Some("browser:page.write")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:subscribeTabsChanged"),
            Some("browser:tabs.read")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:unsubscribeTabsChanged"),
            Some("browser:tabs.read")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:subscribePageChanged"),
            Some("browser:page.read")
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:unsubscribePageChanged"),
            Some("browser:page.read")
        );
        // Discovery calls are intentionally permission-free (low blast radius).
        assert_eq!(
            get_required_permission("launcher:api:browser:listAvailableBrowsers"),
            None
        );
        assert_eq!(
            get_required_permission("launcher:api:browser:isCompanionInstalled"),
            None
        );
    }

    #[test]
    fn search_rank_is_intentionally_permission_free() {
        // The caller supplies its own already-known items and gets back an
        // ordering — no host data is read, nothing persisted, no
        // cross-extension exposure. See the comment above get_required_permission's
        // fallthrough arm.
        assert_eq!(get_required_permission("launcher:api:search:rank"), None);
    }

    #[test]
    fn clipboard_strip_html_and_strip_rtf_are_intentionally_permission_free() {
        // The caller supplies its own markup string and gets back plain text
        // — the host clipboard is never read, nothing is persisted. See the
        // comment above get_required_permission's fallthrough arm.
        assert_eq!(
            get_required_permission("launcher:api:clipboard:stripHtml"),
            None
        );
        assert_eq!(
            get_required_permission("launcher:api:clipboard:stripRtf"),
            None
        );
    }

    // ── Fail-closed gate regression tests ────────────────────────────────────

    #[test]
    fn read_current_text_requires_clipboard_read() {
        // Reading the clipboard's text is exactly clipboard:read, same as its
        // sibling readCurrentClipboard. Was ungated (fail-open hole).
        assert_eq!(
            get_required_permission("launcher:api:clipboard:readCurrentText"),
            Some("clipboard:read")
        );
    }

    #[test]
    fn window_apply_preset_requires_window_manage() {
        // applyPreset moves/resizes the focused window, same as its sibling
        // setWindowBounds. Was ungated (fail-open hole).
        assert_eq!(
            get_required_permission("launcher:api:window:applyPreset"),
            Some("window:manage")
        );
    }

    #[test]
    fn runs_calls_require_runs_track() {
        for call in [
            "launcher:api:runs:start",
            "launcher:api:runs:write",
            "launcher:api:runs:done",
            "launcher:api:runs:fail",
            "launcher:api:runs:cancel",
        ] {
            assert_eq!(
                get_required_permission(call),
                Some("runs:track"),
                "{call} must be gated"
            );
        }
    }

    #[test]
    fn tools_calls_require_tools_register() {
        for call in [
            "launcher:api:tools:registerTool",
            "launcher:api:tools:unregisterTool",
            "launcher:api:tools:listTools",
        ] {
            assert_eq!(
                get_required_permission(call),
                Some("tools:register"),
                "{call} must be gated"
            );
        }
    }

    #[test]
    fn gate_decision_denies_unrecognized_call() {
        // The core of the fix: a call type in neither the gated map nor the
        // public allowlist must be DENIED, not silently allowed.
        assert_eq!(
            gate_decision("launcher:api:totally:madeup"),
            GateDecision::Deny
        );
        assert_eq!(
            gate_decision("launcher:api:secrets:exfiltrate"),
            GateDecision::Deny
        );
    }

    #[test]
    fn gate_decision_allows_enumerated_public_calls() {
        for call in [
            "launcher:api:log:info",
            "launcher:api:log:error",
            "launcher:api:search:rank",
            "launcher:api:clipboard:stripHtml",
            "launcher:api:clipboard:stripRtf",
            "launcher:api:browser:listAvailableBrowsers",
            "launcher:api:browser:isCompanionInstalled",
            "launcher:api:actions:registerActionHandler",
            "launcher:api:actions:registerAction",
            "launcher:api:extensions:navigateToView",
            "launcher:api:extensions:goBack",
            "launcher:api:searchBar:set",
            "launcher:api:statusBar:registerItem",
            "launcher:api:commands:replaceDynamicCommands",
            "launcher:api:log:custom",
            "launcher:api:window:getMonitors",
        ] {
            assert_eq!(
                gate_decision(call),
                GateDecision::AllowPublic,
                "{call} should be permission-free"
            );
        }
    }

    #[test]
    fn gate_decision_allows_worker_view_state_rpc() {
        // The state:* family is worker<->view shared-state plumbing, scoped to
        // the calling extension (state is in INJECTS_EXTENSION_ID) — so it is
        // permission-free like actions:* and extensions:navigateToView. Omitting
        // these made the fail-closed gate deny any extension using shared state.
        for call in [
            "launcher:api:state:get",
            "launcher:api:state:set",
            "launcher:api:state:subscribe",
            "launcher:api:state:unsubscribe",
            "launcher:api:state:rpcRequest",
            "launcher:api:state:rpcReply",
            "launcher:api:state:rpcAbort",
        ] {
            assert_eq!(
                gate_decision(call),
                GateDecision::AllowPublic,
                "{call} should be permission-free (worker<->view state, scoped per extension)"
            );
        }
    }

    #[test]
    fn gate_decision_allows_scoped_feedback_and_action_exec() {
        // Ephemeral, extension-scoped feedback UI (toast/progress/HUD/confirm)
        // and the extension triggering its own action are transient and scoped
        // to the caller — permission-free, like statusBar:* and actions:register*.
        // (Notification-like feedback — announce/sendBackground — stays gated.)
        for call in [
            "launcher:api:feedback:report",
            "launcher:api:feedback:showProgress",
            "launcher:api:feedback:updateProgress",
            "launcher:api:feedback:finishProgress",
            "launcher:api:feedback:showHUD",
            "launcher:api:feedback:dismiss",
            "launcher:api:feedback:confirmAlert",
            "launcher:api:actions:executeAction",
        ] {
            assert_eq!(
                gate_decision(call),
                GateDecision::AllowPublic,
                "{call} should be permission-free (ephemeral, extension-scoped UI)"
            );
        }
    }

    #[test]
    fn gate_decision_allows_onboarding_complete() {
        // Tier 2 extensions with an `onboarding.command` in their manifest call
        // `context.proxies.onboarding.complete()` on themselves once their
        // onboarding view finishes. `extension_id` is host-injected from the
        // trusted iframe attribute (INJECTS_EXTENSION_ID), never the payload,
        // so this only ever marks the *calling* extension onboarded — no
        // cross-extension reach, nothing else persisted. Permission-free like
        // state:*/feedback:*. Was left unclassified when the gate went
        // fail-closed (#567), silently denying every extension onboarding flow.
        assert_eq!(
            gate_decision("launcher:api:onboarding:complete"),
            GateDecision::AllowPublic
        );
    }

    #[test]
    fn gate_decision_requires_permission_for_gated_call() {
        for call in [
            "launcher:api:files:read",
            "launcher:api:files:readRange",
            "launcher:api:files:glob",
        ] {
            assert_eq!(gate_decision(call), GateDecision::Require("files:read"));
        }
        assert_eq!(
            gate_decision("launcher:api:network:fetch"),
            GateDecision::Require("network")
        );
        assert_eq!(
            gate_decision("launcher:api:clipboard:readCurrentText"),
            GateDecision::Require("clipboard:read")
        );
    }

    #[test]
    fn test_check_allows_registered_permission() {
        let reg = make_registry();
        assert!(reg
            .check(&Some("org.launcher.test".to_string()), "network")
            .is_ok());
    }

    #[test]
    fn test_check_denies_missing_permission() {
        let reg = make_registry();
        assert!(reg
            .check(&Some("org.launcher.test".to_string()), "notifications:send")
            .is_err());
    }

    #[test]
    fn test_check_allows_core_call() {
        let reg = make_registry();
        assert!(reg.check(&None, "network").is_ok());
    }

    #[test]
    fn test_check_denies_unregistered_extension() {
        let reg = make_registry();
        assert!(reg
            .check(&Some("org.launcher.unknown".to_string()), "network")
            .is_err());
    }

    #[test]
    fn test_get_required_permission_clipboard_write() {
        assert_eq!(
            get_required_permission("launcher:api:clipboard:writeToClipboard"),
            Some("clipboard:write")
        );
        assert_eq!(
            get_required_permission("launcher:api:clipboard:pasteItem"),
            Some("clipboard:write")
        );
        assert_eq!(
            get_required_permission("launcher:api:clipboard:simulatePaste"),
            Some("clipboard:write")
        );
    }

    #[test]
    fn test_clipboard_permissions_granularity() {
        // Explicit current clipboard read
        assert_eq!(
            get_required_permission("launcher:api:clipboard:readCurrentClipboard"),
            Some("clipboard:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:clipboard:readCurrentText"),
            Some("clipboard:read")
        );

        // Stored history queries require clipboard-history:read (not clipboard:read)
        assert_eq!(
            get_required_permission("launcher:api:clipboard:getRecentItems"),
            Some("clipboard-history:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:clipboard:searchHistory"),
            Some("clipboard-history:read")
        );

        // Background capture subscription requires clipboard-history:capture
        assert_eq!(
            get_required_permission("launcher:api:clipboard:subscribeCapture"),
            Some("clipboard-history:capture")
        );
        assert_eq!(
            get_required_permission("launcher:api:clipboard:unsubscribeCapture"),
            Some("clipboard-history:capture")
        );

        // History management requires clipboard-history:manage
        assert_eq!(
            get_required_permission("launcher:api:clipboard:toggleItemFavorite"),
            Some("clipboard-history:manage")
        );
        assert_eq!(
            get_required_permission("launcher:api:clipboard:deleteItem"),
            Some("clipboard-history:manage")
        );
        assert_eq!(
            get_required_permission("launcher:api:clipboard:clearNonFavorites"),
            Some("clipboard-history:manage")
        );
    }

    #[test]
    fn test_old_clipboard_monitoring_calls_are_denied() {
        // TDD requirement 24: IPC permission tests prove the old global public lifecycle calls are unavailable.
        assert_eq!(
            gate_decision("launcher:api:clipboard:startMonitoring"),
            GateDecision::Deny
        );
        assert_eq!(
            gate_decision("launcher:api:clipboard:stopMonitoring"),
            GateDecision::Deny
        );
        assert!(!is_public_call("launcher:api:clipboard:startMonitoring"));
        assert!(!is_public_call("launcher:api:clipboard:stopMonitoring"));
        assert_eq!(
            get_required_permission("launcher:api:clipboard:startMonitoring"),
            None
        );
        assert_eq!(
            get_required_permission("launcher:api:clipboard:stopMonitoring"),
            None
        );
    }

    #[test]
    fn only_rare_feedback_announcements_require_permission() {
        assert_eq!(
            get_required_permission("launcher:api:feedback:announce"),
            Some("feedback:announce")
        );
        assert_eq!(get_required_permission("launcher:api:feedback:report"), None);
        assert_eq!(
            get_required_permission("launcher:api:feedback:showProgress"),
            None
        );
        assert_eq!(
            get_required_permission("launcher:api:feedback:sendBackground"),
            Some("notifications:send")
        );
        assert_eq!(
            get_required_permission("launcher:api:feedback:dismissBackground"),
            Some("notifications:send")
        );
    }

    #[test]
    fn fs_show_maps_to_fs_read() {
        assert_eq!(
            get_required_permission("launcher:api:fs:showInFileManager"),
            Some("fs:read")
        );
    }

    #[test]
    fn fs_trash_maps_to_fs_write() {
        assert_eq!(
            get_required_permission("launcher:api:fs:trash"),
            Some("fs:write")
        );
    }

    #[test]
    fn test_interop_service_permission() {
        assert_eq!(
            get_required_permission("launcher:api:interop:launchCommand"),
            Some("extension:invoke")
        );
    }

    #[test]
    fn application_canonical_namespace_permissions_mapped() {
        assert_eq!(
            get_required_permission("launcher:api:application:getFrontmostApplication"),
            Some("application:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:application:syncApplicationIndex"),
            Some("application:read")
        );
        assert_eq!(
            get_required_permission("launcher:api:application:listApplications"),
            Some("application:read")
        );
    }

    #[test]
    fn window_manage_permission_mapped() {
        assert_eq!(
            get_required_permission("launcher:api:window:getWindowBounds"),
            Some("window:manage")
        );
        assert_eq!(
            get_required_permission("launcher:api:window:setWindowBounds"),
            Some("window:manage")
        );
        assert_eq!(
            get_required_permission("launcher:api:window:setFullscreen"),
            Some("window:manage")
        );
    }

    #[test]
    fn preferences_get_all_maps_to_preferences_read() {
        assert_eq!(
            get_required_permission("launcher:api:preferences:getAll"),
            Some("preferences:read")
        );
    }

    #[test]
    fn preferences_set_maps_to_preferences_write() {
        assert_eq!(
            get_required_permission("launcher:api:preferences:set"),
            Some("preferences:write")
        );
    }

    #[test]
    fn preferences_reset_maps_to_preferences_write() {
        assert_eq!(
            get_required_permission("launcher:api:preferences:reset"),
            Some("preferences:write")
        );
    }

    // --- Canonical wire type entries (broker.invoke('shell:spawn')
    //     → broker prepends launcher:api: → wire = launcher:api:shell:spawn)

    #[test]
    fn shell_spawn_wire_type_maps_to_shell_spawn() {
        assert_eq!(
            get_required_permission("launcher:api:shell:spawn"),
            Some("shell:spawn")
        );
    }

    #[test]
    fn shell_list_wire_type_maps_to_shell_spawn() {
        assert_eq!(
            get_required_permission("launcher:api:shell:list"),
            Some("shell:spawn")
        );
    }

    #[test]
    fn shell_attach_wire_type_maps_to_shell_spawn() {
        assert_eq!(
            get_required_permission("launcher:api:shell:attach"),
            Some("shell:spawn")
        );
    }

    #[test]
    fn oauth_authorize_maps_to_oauth_use() {
        assert_eq!(
            get_required_permission("launcher:api:oauth:authorize"),
            Some("oauth:use")
        );
    }

    #[test]
    fn oauth_revoke_token_maps_to_oauth_use() {
        assert_eq!(
            get_required_permission("launcher:api:oauth:revokeToken"),
            Some("oauth:use")
        );
    }

    #[test]
    fn ai_prompting_calls_map_to_ai_permission() {
        assert_eq!(
            get_required_permission("launcher:api:ai:streamChat"),
            Some("ai")
        );
        assert_eq!(get_required_permission("launcher:api:ai:complete"), Some("ai"));
    }

    #[test]
    fn mcp_calls_map_to_mcp_permission() {
        assert_eq!(
            get_required_permission("launcher:api:mcp:listServers"),
            Some("mcp")
        );
        assert_eq!(
            get_required_permission("launcher:api:mcp:listTools"),
            Some("mcp")
        );
        assert_eq!(
            get_required_permission("launcher:api:mcp:invokeTool"),
            Some("mcp")
        );
    }

    #[test]
    fn entitlements_check_maps_to_entitlements_read() {
        assert_eq!(
            get_required_permission("launcher:api:entitlements:check"),
            Some("entitlements:read")
        );
    }
    #[test]
    fn entitlements_get_all_maps_to_entitlements_read() {
        assert_eq!(
            get_required_permission("launcher:api:entitlements:getAll"),
            Some("entitlements:read")
        );
    }

    /// Proves that the defense-in-depth check in shell_spawn must pass the manifest
    /// permission name ("shell:spawn"), not the IPC wire type, to registry.check().
    #[test]
    fn registry_check_uses_manifest_permission_name_not_ipc_wire_type() {
        let reg = ExtensionPermissionRegistry::new();
        {
            let mut inner = reg.inner.lock().unwrap();
            inner.insert("org.launcher.sdk-playground".to_string(), {
                let mut s = HashSet::new();
                s.insert("shell:spawn".to_string()); // what manifest.json declares
                s
            });
        }
        // Passing the manifest permission name → allowed
        assert!(reg
            .check(&Some("org.launcher.sdk-playground".to_string()), "shell:spawn")
            .is_ok());
        // Passing the IPC wire type → denied (proves shell.rs must NOT use this string)
        assert!(reg
            .check(
                &Some("org.launcher.sdk-playground".to_string()),
                "launcher:api:shell:spawn"
            )
            .is_err());
    }

    #[test]
    fn selection_get_selected_text_maps_to_selection_read() {
        assert_eq!(
            get_required_permission("launcher:api:selection:getSelectedText"),
            Some("selection:read")
        );
    }
    #[test]
    fn selection_get_selected_finder_items_maps_to_selection_read() {
        assert_eq!(
            get_required_permission("launcher:api:selection:getSelectedFinderItems"),
            Some("selection:read")
        );
    }

    #[test]
    fn system_events_subscribe_maps_to_system_events_read() {
        assert_eq!(
            get_required_permission("launcher:api:systemEvents:subscribe"),
            Some("systemEvents:read")
        );
    }
    #[test]
    fn system_events_unsubscribe_maps_to_system_events_read() {
        assert_eq!(
            get_required_permission("launcher:api:systemEvents:unsubscribe"),
            Some("systemEvents:read")
        );
    }

    #[test]
    fn app_events_subscribe_maps_to_frontmost_watch() {
        assert_eq!(
            get_required_permission("launcher:api:appEvents:subscribe"),
            Some("app:frontmost-watch")
        );
    }
    #[test]
    fn app_events_unsubscribe_maps_to_frontmost_watch() {
        assert_eq!(
            get_required_permission("launcher:api:appEvents:unsubscribe"),
            Some("app:frontmost-watch")
        );
    }
    #[test]
    fn application_is_running_maps_to_application_read() {
        assert_eq!(
            get_required_permission("launcher:api:application:isRunning"),
            Some("application:read")
        );
    }

    #[test]
    fn timers_schedule_wire_maps_to_timers_schedule() {
        assert_eq!(
            get_required_permission("launcher:api:timers:schedule"),
            Some("timers:schedule")
        );
    }
    #[test]
    fn timers_cancel_wire_maps_to_timers_cancel() {
        assert_eq!(
            get_required_permission("launcher:api:timers:cancel"),
            Some("timers:cancel")
        );
    }
    #[test]
    fn timers_list_wire_maps_to_timers_list() {
        assert_eq!(
            get_required_permission("launcher:api:timers:list"),
            Some("timers:list")
        );
    }

    // ---- fs:watch wire mappings ----

    #[test]
    fn fs_watcher_create_wire_maps_to_fs_watch() {
        assert_eq!(
            get_required_permission("launcher:api:fsWatcher:create"),
            Some("fs:watch")
        );
    }

    #[test]
    fn fs_watcher_dispose_wire_maps_to_fs_watch() {
        assert_eq!(
            get_required_permission("launcher:api:fsWatcher:dispose"),
            Some("fs:watch")
        );
    }

    // ---- args store ----

    #[test]
    fn args_for_returns_none_when_not_registered() {
        let reg = ExtensionPermissionRegistry::default();
        assert!(reg.args_for("ext.a", "fs:watch").is_none());
    }

    #[test]
    fn args_for_returns_registered_value() {
        let reg = ExtensionPermissionRegistry::default();
        let mut inner_args = HashMap::new();
        inner_args.insert("fs:watch".to_string(), serde_json::json!(["~/foo/**"]));
        reg.register("ext.a", HashSet::from(["fs:watch".to_string()]), inner_args);
        let v = reg.args_for("ext.a", "fs:watch").unwrap();
        assert!(v.is_array());
    }

    #[test]
    fn fs_watch_patterns_returns_registered_strings() {
        let reg = ExtensionPermissionRegistry::default();
        let mut inner_args = HashMap::new();
        inner_args.insert(
            "fs:watch".to_string(),
            serde_json::json!(["~/foo/**", "~/bar/config"]),
        );
        reg.register("ext.a", HashSet::from(["fs:watch".to_string()]), inner_args);
        let patterns = reg.fs_watch_patterns("ext.a").unwrap();
        assert_eq!(
            patterns,
            vec!["~/foo/**".to_string(), "~/bar/config".to_string()]
        );
    }

    #[test]
    fn fs_watch_patterns_errors_when_extension_has_no_args() {
        let reg = ExtensionPermissionRegistry::default();
        reg.register("ext.a", HashSet::new(), HashMap::new());
        assert!(reg.fs_watch_patterns("ext.a").is_err());
    }

    #[test]
    fn fs_watch_patterns_errors_when_extension_not_registered() {
        let reg = ExtensionPermissionRegistry::default();
        assert!(reg.fs_watch_patterns("ext.missing").is_err());
    }

    #[test]
    fn files_read_patterns_returns_registered_strings() {
        let reg = ExtensionPermissionRegistry::default();
        let mut inner_args = HashMap::new();
        inner_args.insert(
            "files:read".to_string(),
            serde_json::json!(["**/steamapps/appmanifest_*.acf"]),
        );
        reg.register(
            "ext.a",
            HashSet::from(["files:read".to_string()]),
            inner_args,
        );
        let patterns = reg.files_read_patterns("ext.a").unwrap();
        assert_eq!(patterns, vec!["**/steamapps/appmanifest_*.acf".to_string()]);
    }

    #[test]
    fn files_read_patterns_errors_when_extension_has_no_args() {
        let reg = ExtensionPermissionRegistry::default();
        reg.register("ext.a", HashSet::new(), HashMap::new());
        assert!(reg.files_read_patterns("ext.a").is_err());
    }

    #[test]
    fn screen_pick_color_maps_to_screen_pick_color() {
        assert_eq!(
            get_required_permission("launcher:api:screen:pickColor"),
            Some("screen:pick-color")
        );
    }

    #[test]
    fn screen_capture_text_maps_to_screen_capture() {
        assert_eq!(
            get_required_permission("launcher:api:screen:captureText"),
            Some("screen:capture")
        );
    }

    #[test]
    fn calculator_evaluate_maps_to_calculator_evaluate() {
        assert_eq!(
            get_required_permission("launcher:api:calculator:evaluate"),
            Some("calculator:evaluate")
        );
    }

    #[test]
    fn snippets_register_requires_contribute_permission() {
        assert_eq!(
            get_required_permission("launcher:api:snippets:registerShortcodes"),
            Some("snippets:contribute"),
        );
        assert_eq!(
            get_required_permission("launcher:api:snippets:unregisterShortcodes"),
            Some("snippets:contribute"),
        );
    }

    #[test]
    fn unregister_removes_both_perms_and_args() {
        let reg = ExtensionPermissionRegistry::default();
        let mut inner_args = HashMap::new();
        inner_args.insert("fs:watch".to_string(), serde_json::json!(["~/foo"]));
        reg.register("ext.a", HashSet::from(["fs:watch".to_string()]), inner_args);
        reg.unregister("ext.a");
        assert!(reg.check(&Some("ext.a".to_string()), "fs:watch").is_err());
        assert!(reg.args_for("ext.a", "fs:watch").is_none());
    }

    #[test]
    fn check_allows_none_and_launcher_host() {
        let reg = ExtensionPermissionRegistry::default();
        assert!(reg.check(&None, "notifications:send").is_ok());
        assert!(reg
            .check(&Some("launcher".to_string()), "notifications:send")
            .is_ok());
    }

    // ---- resolve_declared_permissions (register_extension_permissions'
    // forged-input resistance) ----

    fn make_record(
        id: &str,
        is_built_in: bool,
        permissions: Option<Vec<String>>,
    ) -> crate::extensions::ExtensionRecord {
        crate::extensions::ExtensionRecord {
            manifest: crate::extensions::ExtensionManifest {
                id: id.to_string(),
                name: id.to_string(),
                version: "1.0.0".to_string(),
                description: String::new(),
                author: None,
                extension_type: None,
                lifecycle: None,
                background: None,
                searchable: None,
                icon: None,
                commands: vec![],
                permissions,
                permission_args: None,
                min_app_version: None,
                launcher_sdk: None,
                platforms: None,
                preferences: None,
                onboarding: None,
                tools: None,
                runtimes: None,
                walkthrough: None,
            },
            enabled: true,
            is_built_in,
            disableable: !is_built_in,
            path: format!("/tmp/{id}"),
            compatibility: crate::extensions::CompatibilityStatus::Unknown,
            first_view_component: None,
        }
    }

    #[test]
    fn register_extension_permissions_ignores_forged_permissions_for_known_extension() {
        let mut extensions = HashMap::new();
        extensions.insert(
            "org.launcher.known".to_string(),
            make_record(
                "org.launcher.known",
                false,
                Some(vec!["clipboard:read".to_string()]),
            ),
        );

        // A compromised caller tries to smuggle a permission its manifest
        // never declared.
        let forged = vec!["shell:spawn".to_string(), "clipboard:read".to_string()];
        let (is_built_in, declared_perms, _declared_args) =
            resolve_declared_permissions(&extensions, "org.launcher.known", forged, None);

        assert!(!is_built_in);
        assert_eq!(declared_perms, vec!["clipboard:read".to_string()]);
    }

    #[test]
    fn resolve_declared_permissions_trusts_caller_when_extension_is_not_discovered() {
        let extensions: HashMap<String, crate::extensions::ExtensionRecord> = HashMap::new();
        let supplied = vec!["network".to_string()];

        let (is_built_in, declared_perms, _declared_args) =
            resolve_declared_permissions(&extensions, "org.launcher.unknown", supplied.clone(), None);

        assert!(!is_built_in);
        assert_eq!(declared_perms, supplied);
    }
}
