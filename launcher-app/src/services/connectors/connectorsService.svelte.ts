import type { ConnectorDeclaration } from 'launcher-sdk/contracts';
import * as commands from '../../lib/ipc/commands';
import type { ConnectorAccountRecord } from '../../lib/ipc/commands';
import { getExtensionFrameOrigin } from '../../lib/ipc/extensionOrigin';
import { logService } from '../log/logService';
import type { ExtensionOAuthService } from '../oauth/extensionOAuthService.svelte';

/** What an extension sees for a connected account. */
export interface ConnectedAccount {
  accountId: string;
  displayName: string;
  /** Origin of the server, for connectors declared with `server`. */
  serverUrl?: string;
}

export interface ConnectOptions {
  /** The address the user typed. Required for connectors declared with `server`. */
  serverUrl?: string;
}

export interface ConnectResult {
  account: ConnectedAccount;
  /** The signed-in account was already connected; its tokens were renewed. */
  alreadyConnected: boolean;
}

type ManifestConnectors = (extensionId: string) => ConnectorDeclaration[] | undefined;
type Listener = (extensionId: string, connectorId: string) => void;

const keyOf = (extensionId: string, connectorId: string) => `${extensionId}\u0000${connectorId}`;

const toAccount = (r: ConnectorAccountRecord): ConnectedAccount => ({
  accountId: r.accountId,
  displayName: r.displayName,
  ...(r.serverUrl ? { serverUrl: r.serverUrl } : {}),
});

const DEFAULT_CLIENT_NAME = 'Launcher';

/**
 * Host-owned connector accounts.
 *
 * Settings drives `connect` / `disconnect`; extensions only read accounts and
 * tokens through the IPC surface (`getAccounts` / `getToken`), both of which
 * are restricted to connectors the calling extension declared in its manifest.
 *
 * Takes its collaborators by constructor so it never imports the extension
 * manager: the manifest lookup is supplied by the composition root.
 */
export class ConnectorsService {
  /** Reactive account lists for the Settings UI, keyed by extension+connector. */
  accounts = $state<Record<string, ConnectedAccount[]>>({});
  /** Connect flows in progress, keyed like `accounts`. */
  connecting = $state<Record<string, boolean>>({});

  private listeners = new Set<Listener>();

  constructor(
    private readonly oauth: Pick<
      ExtensionOAuthService,
      'authorizeForHost' | 'trackHostFlow' | 'newHostFlowId'
    >,
    private readonly connectorsOf: ManifestConnectors,
  ) {}

  /** Connectors an extension declared. */
  list(extensionId: string): ConnectorDeclaration[] {
    return this.connectorsOf(extensionId) ?? [];
  }

  accountsFor(extensionId: string, connectorId: string): ConnectedAccount[] {
    return this.accounts[keyOf(extensionId, connectorId)] ?? [];
  }

  isConnecting(extensionId: string, connectorId: string): boolean {
    return this.connecting[keyOf(extensionId, connectorId)] === true;
  }

  /** True when every `required` connector of the extension has an account. */
  async hasRequiredConnections(extensionId: string): Promise<boolean> {
    for (const decl of this.list(extensionId)) {
      if (!decl.required) continue;
      if ((await this.refresh(extensionId, decl.id)).length === 0) return false;
    }
    return true;
  }

  /** Connectors that are `required` and still have no account. */
  async missingRequired(extensionId: string): Promise<ConnectorDeclaration[]> {
    const missing: ConnectorDeclaration[] = [];
    for (const decl of this.list(extensionId)) {
      if (decl.required && (await this.refresh(extensionId, decl.id)).length === 0) {
        missing.push(decl);
      }
    }
    return missing;
  }

  /** Reload one connector's accounts from the database. */
  async refresh(extensionId: string, connectorId: string): Promise<ConnectedAccount[]> {
    const records = await commands.connectorListAccounts(extensionId, connectorId);
    const accounts = records.map(toAccount);
    this.accounts[keyOf(extensionId, connectorId)] = accounts;
    return accounts;
  }

  /** Run the whole connect flow: browser sign-in, then identify the account. */
  async connect(
    extensionId: string,
    connectorId: string,
    options: ConnectOptions = {},
  ): Promise<ConnectResult> {
    const decl = this.declaration(extensionId, connectorId);
    if (decl.server && !options.serverUrl?.trim()) {
      throw new Error('Enter the address of your server');
    }
    const key = keyOf(extensionId, connectorId);
    if (this.connecting[key]) throw new Error('A connection is already in progress');

    this.connecting[key] = true;
    try {
      const before = await this.refresh(extensionId, connectorId);
      const pendingProviderId = `connector-pending:${crypto.randomUUID()}`;
      const { account, alreadyConnected } = decl.server
        ? await this.connectServer(extensionId, connectorId, pendingProviderId, decl, options)
        : await this.connectFixed(extensionId, connectorId, pendingProviderId, decl);

      // A single-account connector holds one account: signing in as someone
      // else replaces the previous one.
      if (!decl.multiAccount) {
        for (const old of before) {
          if (old.accountId !== account.accountId) {
            await commands.connectorDisconnect(extensionId, connectorId, old.accountId);
          }
        }
      }

      await this.refresh(extensionId, connectorId);
      this.notify(extensionId, connectorId);
      return { account: toAccount(account), alreadyConnected };
    } finally {
      this.connecting[key] = false;
    }
  }

  /** Fixed provider: the manifest names the OAuth app and how to identify the account. */
  private async connectFixed(
    extensionId: string,
    connectorId: string,
    pendingProviderId: string,
    decl: ConnectorDeclaration,
  ) {
    if (!decl.oauth || !decl.account) throw new Error(`connector_misconfigured: ${connectorId}`);
    await this.oauth.authorizeForHost(extensionId, pendingProviderId, decl.oauth);
    return commands.connectorFinalizeConnect(
      extensionId,
      connectorId,
      pendingProviderId,
      decl.oauth.authorizationUrl,
      decl.oauth.tokenUrl,
      decl.account,
    );
  }

  /** User-supplied server: Rust discovers its endpoints and registers the client. */
  private async connectServer(
    extensionId: string,
    connectorId: string,
    pendingProviderId: string,
    decl: ConnectorDeclaration,
    options: ConnectOptions,
  ) {
    const flowId = this.oauth.newHostFlowId();
    const start = await commands.connectorBeginDynamic(
      extensionId,
      pendingProviderId,
      flowId,
      options.serverUrl ?? '',
      decl.server?.scopes ?? [],
      decl.server?.clientName ?? DEFAULT_CLIENT_NAME,
    );
    await this.oauth.trackHostFlow(extensionId, flowId, start.state, start.authUrl);
    return commands.connectorFinalizeDynamic(
      extensionId,
      connectorId,
      pendingProviderId,
      start.serverUrl,
      start.clientId,
      start.tokenUrl,
    );
  }

  async disconnect(extensionId: string, connectorId: string, accountId: string): Promise<void> {
    this.declaration(extensionId, connectorId);
    await commands.connectorDisconnect(extensionId, connectorId, accountId);
    await this.refresh(extensionId, connectorId);
    this.notify(extensionId, connectorId);
  }

  // ── Extension IPC surface (extensionId injected by the router) ──────────────

  async getAccounts(extensionId: string, connectorId: string): Promise<ConnectedAccount[]> {
    this.declaration(extensionId, connectorId);
    return this.refresh(extensionId, connectorId);
  }

  async getToken(extensionId: string, connectorId: string, accountId: string): Promise<string> {
    const decl = this.declaration(extensionId, connectorId);
    return commands.connectorGetToken(
      extensionId,
      connectorId,
      accountId,
      // Server-backed accounts refresh with the endpoints they stored at connect time.
      decl.oauth?.tokenUrl ?? '',
      decl.oauth?.clientId ?? '',
    );
  }

  async fetch(
    extensionId: string,
    connectorId: string,
    accountId: string,
    path: string,
    options: Parameters<typeof commands.connectorFetch>[4] = {},
  ): Promise<commands.ConnectorFetchResult> {
    const decl = this.declaration(extensionId, connectorId);
    if (!decl.server) throw new Error(`connector_not_server: ${connectorId}`);
    return commands.connectorFetch(extensionId, connectorId, accountId, path, options);
  }

  // ── Change notifications ────────────────────────────────────────────────────

  /** Subscribe to connect/disconnect events. Returns an unsubscribe function. */
  onChange(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private notify(extensionId: string, connectorId: string): void {
    for (const listener of this.listeners) listener(extensionId, connectorId);
    this.postToIframes(extensionId, connectorId);
  }

  private postToIframes(extensionId: string, connectorId: string): void {
    if (typeof document === 'undefined') return;
    const origin = getExtensionFrameOrigin(extensionId);
    const iframes = document.querySelectorAll<HTMLIFrameElement>(
      `iframe[data-extension-id="${extensionId}"]`,
    );
    for (const iframe of Array.from(iframes)) {
      try {
        iframe.contentWindow?.postMessage(
          { type: 'launcher:connectors:changed', connectorId },
          origin,
        );
      } catch (err) {
        logService.warn(`connectors: failed to notify ${extensionId}: ${err}`);
      }
    }
  }

  /** The declaration, or an error when this extension did not declare it. */
  private declaration(extensionId: string, connectorId: string): ConnectorDeclaration {
    const decl = this.list(extensionId).find((c) => c.id === connectorId);
    if (!decl) throw new Error(`connector_not_declared: ${connectorId}`);
    return decl;
  }
}
