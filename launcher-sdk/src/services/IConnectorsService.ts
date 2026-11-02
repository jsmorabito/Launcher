/** An account the user connected to one of the extension's declared connectors. */
export interface ConnectorAccount {
  /** Stable provider-side id (e.g. a Linear workspace id). */
  accountId: string;
  /** Human-readable name shown in Settings and account pickers. */
  displayName: string;
  /** Origin of the server, for connectors declared with `server`. */
  serverUrl?: string;
}

export interface ConnectorFetchOptions {
  /** Defaults to `GET`. */
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
  headers?: Record<string, string>;
  body?: string;
  /** Milliseconds, default 30000, at most 60000. */
  timeout?: number;
}

export interface ConnectorFetchResponse {
  status: number;
  ok: boolean;
  body: string;
}

/**
 * Rejection messages from `getToken()`.
 * - `connector_not_connected` — no such account (never connected, or disconnected).
 * - `reconnect_required` — the stored token expired and could not be refreshed;
 *   ask the user to reconnect in Settings → Extensions.
 */
export type ConnectorErrorCode = 'connector_not_connected' | 'reconnect_required';

/**
 * Accounts connected through manifest-declared `connectors`.
 *
 * Launcher owns the whole sign-in flow in its own Settings UI; the extension
 * never opens a browser, handles a client secret, or stores a token. Requires
 * the `connectors:use` permission.
 */
export interface IConnectorsService {
  /** Accounts currently connected for `connectorId`, oldest first. */
  getAccounts(connectorId: string): Promise<ConnectorAccount[]>;

  /**
   * A valid access token for one account. Refreshed by the host when expired.
   * Never opens UI; rejects with a {@link ConnectorErrorCode} message instead.
   */
  getToken(connectorId: string, accountId: string): Promise<string>;

  /**
   * Make a request to the server of a connected account. Only for connectors
   * declared with `server`. Launcher sends it to the address the user connected,
   * adds the account's token (the extension never sees it), and never follows
   * redirects. Because the user entered that address, it may be `localhost` or a
   * LAN address, which `network.fetch` refuses. `path` starts with `/` and cannot
   * leave that server. `Authorization` and other host-owned headers are ignored.
   */
  fetch(
    connectorId: string,
    accountId: string,
    path: string,
    options?: ConnectorFetchOptions,
  ): Promise<ConnectorFetchResponse>;

  /**
   * Subscribe to connect/disconnect events for any of this extension's
   * connectors. Returns an unsubscribe function.
   */
  onChange(callback: (connectorId: string) => void): () => void;
}
