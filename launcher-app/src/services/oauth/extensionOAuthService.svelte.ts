import { openUrl } from '@tauri-apps/plugin-opener';
import { logService } from '../log/logService';
import { getExtensionFrameOrigin } from '../../lib/ipc/extensionOrigin';
import * as commands from '../../lib/ipc/commands';
import type { OAuthTokenPayload } from '../../lib/ipc/commands';
import { appListen } from '../../lib/ipc/bridgeEvents';

/** Flow ids of host-initiated flows (Settings → Connect) carry this prefix. */
const HOST_FLOW_PREFIX = 'host:';

/** Matches this instance's own scheme: `launcher://` or `launcher-dev://`. */
const OAUTH_CALLBACK_RE = /^launcher(-dev)?:\/\/oauth\/callback/;

/** How long a host-initiated flow waits for the browser round-trip. */
const HOST_FLOW_TIMEOUT_MS = 5 * 60 * 1000;

export interface HostOAuthConfig {
  clientId: string;
  authorizationUrl: string;
  tokenUrl: string;
  scopes: string[];
}

export class ExtensionOAuthService {
  /** Settles host-initiated flows by flowId; see [[authorizeForHost]]. */
  private _hostFlows = new Map<
    string,
    { resolve: () => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }
  >();

  /**
   * Maps the opaque state param → {extensionId, flowId} so that errors arriving
   * via deep link can be routed to the right iframe even though Rust has already
   * cleared the pending flow on exchange success.
   */
  private _pendingFlows = new Map<string, { extensionId: string; flowId: string }>();

  /** Set up the deep-link listener for launcher://oauth/callback. Call once at app init. */
  async init(): Promise<void> {
    await appListen<string>('launcher:deep-link', async (event) => {
      const url = event.payload;
      if (!OAUTH_CALLBACK_RE.test(url)) return;
      await this._handleCallback(url);
    });
  }

  /**
   * Begin an OAuth PKCE flow for an extension.
   *
   * Called by ExtensionIpcRouter with extensionId injected as the first arg.
   * Payload key order in OAuthServiceProxy must match the parameter order here
   * (after extensionId): providerId, clientId, authorizationUrl, tokenUrl, scopes, flowId.
   *
   * Returns:
   *  - The cached OAuthTokenPayload directly if a valid token already exists.
   *  - { pending: true } if the browser was opened and we are waiting for callback.
   */
  async authorize(
    extensionId: string,
    providerId: string,
    clientId: string,
    authorizationUrl: string,
    tokenUrl: string,
    scopes: string[],
    flowId: string,
  ): Promise<{ pending: true } | OAuthTokenPayload> {
    // 1. Check for a cached valid token — Rust filters expired tokens, so null = start fresh
    const cached = await commands.oauthGetStoredToken(extensionId, providerId);
    if (cached) {
      return cached;
    }

    // 2. Start PKCE flow in Rust (generates verifier/challenge + state, builds URL)
    const startResult = await commands.oauthStartFlow(
      extensionId,
      providerId,
      clientId,
      authorizationUrl,
      tokenUrl,
      scopes,
      flowId,
    );
    if (!startResult) {
      throw new Error('Failed to start OAuth flow');
    }
    const { state, authUrl } = startResult;

    // 3. Track state → {extensionId, flowId} for error routing in _handleCallback
    this._pendingFlows.set(state, { extensionId, flowId });

    // 4. Open the browser
    await openUrl(authUrl);

    return { pending: true };
  }

  /**
   * Run a full PKCE flow on behalf of the host UI (Settings → Connect), with no
   * extension iframe involved. Resolves once the token is stored under
   * `providerId`; rejects on denial, exchange failure or timeout.
   */
  async authorizeForHost(
    extensionId: string,
    providerId: string,
    config: HostOAuthConfig,
  ): Promise<void> {
    const flowId = this.newHostFlowId();
    const startResult = await commands.oauthStartFlow(
      extensionId,
      providerId,
      config.clientId,
      config.authorizationUrl,
      config.tokenUrl,
      config.scopes,
      flowId,
    );
    if (!startResult) throw new Error('Failed to start OAuth flow');
    return this.trackHostFlow(extensionId, flowId, startResult.state, startResult.authUrl);
  }

  /**
   * Open the browser for a flow Rust already started (the pending PKCE state is
   * stored there) and wait for the redirect to complete it. Used for servers the
   * user supplies, where Rust discovers endpoints and registers the client first.
   */
  trackHostFlow(
    extensionId: string,
    flowId: string,
    state: string,
    authUrl: string,
  ): Promise<void> {
    this._pendingFlows.set(state, { extensionId, flowId });

    return new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        this._hostFlows.delete(flowId);
        this._pendingFlows.delete(state);
        reject(new Error('OAuth error [timeout]: authorization timed out'));
      }, HOST_FLOW_TIMEOUT_MS);
      this._hostFlows.set(flowId, { resolve, reject, timer });
      openUrl(authUrl).catch((err) =>
        this._settleHostFlow(flowId, { error: { code: 'open_failed', message: String(err) } }),
      );
    });
  }

  /** A fresh id for a host-initiated flow. */
  newHostFlowId(): string {
    return `${HOST_FLOW_PREFIX}${crypto.randomUUID()}`;
  }

  /** Revoke the stored token for an extension+provider. */
  async revokeToken(extensionId: string, providerId: string): Promise<void> {
    await commands.oauthRevokeExtensionToken(extensionId, providerId);
  }

  /** Handle the launcher://oauth/callback deep link. Called by the init() listener. */
  async _handleCallback(url: string): Promise<void> {
    const parsed = new URL(url);
    const code = parsed.searchParams.get('code');
    const state = parsed.searchParams.get('state');
    const error = parsed.searchParams.get('error');

    if (!state) return;

    if (error || !code) {
      const pending = this._pendingFlows.get(state);
      if (pending) {
        this._pendingFlows.delete(state);
        this._postToIframe(pending.extensionId, pending.flowId, {
          error: { code: error ?? 'access_denied', message: 'OAuth authorization was denied' },
        });
      }
      return;
    }

    try {
      const result = await commands.oauthExchangeCode(state, code);
      // Clean up local tracking now that exchange succeeded
      this._pendingFlows.delete(state);
      this._postToIframe(result.extensionId, result.flowId, { token: result.token });
    } catch (err) {
      logService.error(`OAuth token exchange failed: ${err}`);
      const pending = this._pendingFlows.get(state);
      if (pending) {
        this._pendingFlows.delete(state);
        this._postToIframe(pending.extensionId, pending.flowId, {
          error: { code: 'exchange_failed', message: String(err) },
        });
      }
    }
  }

  /** Settle a host-initiated flow. Returns false when `flowId` is not one. */
  private _settleHostFlow(
    flowId: string,
    payload: { token?: unknown; error?: { code: string; message: string } },
  ): boolean {
    const flow = this._hostFlows.get(flowId);
    if (!flow) return false;
    this._hostFlows.delete(flowId);
    clearTimeout(flow.timer);
    if (payload.error) {
      flow.reject(new Error(`OAuth error [${payload.error.code}]: ${payload.error.message}`));
    } else {
      flow.resolve();
    }
    return true;
  }

  private _postToIframe(extensionId: string, flowId: string, payload: object): void {
    if (this._settleHostFlow(flowId, payload)) return;
    // OAuth can be initiated from either role (worker or view); broadcast to
    // all iframes of this extension and rely on the SDK OAuthServiceProxy's
    // flowId filter to drop the message in the non-initiator iframe. Worker
    // and view share origin (launcher-extension://<id>), so the token payload
    // crossing between them is within the extension's trust boundary.
    const iframes = Array.from(
      document.querySelectorAll<HTMLIFrameElement>(`iframe[data-extension-id="${extensionId}"]`),
    );
    const origin = getExtensionFrameOrigin(extensionId);
    for (const iframe of iframes) {
      if (!iframe.contentWindow) continue;
      iframe.contentWindow.postMessage(
        { type: 'launcher:oauth:result', flowId, ...payload },
        origin,
      );
    }
  }
}

export const extensionOAuthService = new ExtensionOAuthService();
