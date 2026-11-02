import { describe, it, expect, vi, beforeEach } from 'vitest';

// ── DOM shim (Node test environment has no document) ─────────────────────────
let mockPostMessage: ReturnType<typeof vi.fn>;
let mockIframes: Array<{ contentWindow: { postMessage: ReturnType<typeof vi.fn> } }>;

function makeMockIframe() {
  return {
    contentWindow: { postMessage: vi.fn() },
    setAttribute: vi.fn(),
    getAttribute: vi.fn(),
  };
}

if (typeof document === 'undefined') {
  const defaultIframe = makeMockIframe();
  mockPostMessage = defaultIframe.contentWindow.postMessage;
  mockIframes = [defaultIframe];
  (global as any).document = {
    querySelector: vi.fn(() => mockIframes[0]),
    querySelectorAll: vi.fn(() => mockIframes),
    createElement: vi.fn(() => defaultIframe),
    body: { appendChild: vi.fn(), removeChild: vi.fn() },
  };
}

function setMockIframes(n: number): typeof mockIframes {
  mockIframes = Array.from({ length: n }, () => makeMockIframe());
  mockPostMessage = mockIframes[0].contentWindow.postMessage;
  return mockIframes;
}

// ── Mocks (all before imports) ────────────────────────────────────────────────

vi.mock('../../lib/ipc/commands', () => ({
  oauthStartFlow: vi.fn(),
  oauthExchangeCode: vi.fn(),
  oauthGetStoredToken: vi.fn(),
  oauthRevokeExtensionToken: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('../../lib/ipc/extensionOrigin', () => ({
  getExtensionFrameOrigin: vi.fn().mockReturnValue('null'),
}));

vi.mock('../log/logService', () => ({
  logService: { error: vi.fn(), warn: vi.fn(), info: vi.fn() },
}));

// ── Imports ───────────────────────────────────────────────────────────────────

import { ExtensionOAuthService } from './extensionOAuthService.svelte';
import * as commands from '../../lib/ipc/commands';
import { openUrl } from '@tauri-apps/plugin-opener';

// ── Helpers ───────────────────────────────────────────────────────────────────

function makeService() {
  return new ExtensionOAuthService();
}

function validToken(): commands.OAuthTokenPayload {
  return {
    accessToken: 'tok-abc',
    tokenType: 'Bearer',
    scopes: ['repo'],
    expiresAt: Math.floor(Date.now() / 1000) + 3600, // 1h from now
  };
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('ExtensionOAuthService', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('authorize()', () => {
    it('returns cached token immediately when valid and not expired', async () => {
      const service = makeService();
      const token = validToken();
      vi.mocked(commands.oauthGetStoredToken).mockResolvedValue(token);

      const result = await service.authorize(
        'ext.foo',
        'github',
        'clientid',
        'https://github.com/login/oauth/authorize',
        'https://github.com/login/oauth/access_token',
        ['repo'],
        'flow-1',
      );

      expect(result).toEqual(token);
      expect(openUrl).not.toHaveBeenCalled();
    });

    it('starts PKCE flow and returns pending:true when no cached token', async () => {
      const service = makeService();
      vi.mocked(commands.oauthGetStoredToken).mockResolvedValue(null);
      vi.mocked(commands.oauthStartFlow).mockResolvedValue({
        state: 'state-xyz',
        authUrl: 'https://github.com/login/oauth/authorize?...',
      });

      const result = await service.authorize(
        'ext.foo',
        'github',
        'clientid',
        'https://github.com/login/oauth/authorize',
        'https://github.com/login/oauth/access_token',
        ['repo'],
        'flow-2',
      );

      expect(result).toEqual({ pending: true });
      expect(openUrl).toHaveBeenCalledWith('https://github.com/login/oauth/authorize?...');
    });

    // Note: expired token filtering is handled in Rust (oauth_get_stored_token returns null
    // for expired tokens). No TS test needed — coverage lives in service::tests in Rust.
  });

  describe('revokeToken()', () => {
    it('calls oauthRevokeExtensionToken with correct args', async () => {
      const service = makeService();
      vi.mocked(commands.oauthRevokeExtensionToken).mockResolvedValue(undefined);

      await service.revokeToken('ext.foo', 'github');

      expect(commands.oauthRevokeExtensionToken).toHaveBeenCalledWith('ext.foo', 'github');
    });
  });

  describe('_handleCallback()', () => {
    it('calls oauthExchangeCode and posts token to iframe on success', async () => {
      const service = makeService();
      const token = validToken();

      // Pre-register a pending flow (simulate what authorize() does)
      vi.mocked(commands.oauthGetStoredToken).mockResolvedValue(null);
      vi.mocked(commands.oauthStartFlow).mockResolvedValue({
        state: 'state-abc',
        authUrl: 'https://example.com/auth',
      });
      await service.authorize(
        'ext.foo',
        'github',
        'cid',
        'https://example.com/auth',
        'https://example.com/token',
        [],
        'flow-42',
      );

      vi.mocked(commands.oauthExchangeCode).mockResolvedValue({
        extensionId: 'ext.foo',
        flowId: 'flow-42',
        token,
      });

      // Simulate callback
      await (service as any)._handleCallback(
        'launcher://oauth/callback?code=mycode&state=state-abc',
      );

      expect(commands.oauthExchangeCode).toHaveBeenCalledWith('state-abc', 'mycode');
      // document.querySelector is mocked — the postMessage on the returned iframe fires
      expect(mockPostMessage).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'launcher:oauth:result', flowId: 'flow-42', token }),
        expect.any(String),
      );
    });

    it('posts error to iframe on access_denied', async () => {
      const service = makeService();

      vi.mocked(commands.oauthGetStoredToken).mockResolvedValue(null);
      vi.mocked(commands.oauthStartFlow).mockResolvedValue({
        state: 'state-err',
        authUrl: 'https://example.com/auth',
      });
      await service.authorize(
        'ext.foo',
        'github',
        'cid',
        'https://example.com/auth',
        'https://example.com/token',
        [],
        'flow-err',
      );

      vi.clearAllMocks(); // reset mockPostMessage call count

      await (service as any)._handleCallback(
        'launcher://oauth/callback?error=access_denied&state=state-err',
      );

      expect(mockPostMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'launcher:oauth:result',
          flowId: 'flow-err',
          error: expect.objectContaining({ code: 'access_denied' }),
        }),
        expect.any(String),
      );
    });

    it('does nothing when state is missing from callback URL', async () => {
      const service = makeService();

      await (service as any)._handleCallback('launcher://oauth/callback?code=abc');

      expect(commands.oauthExchangeCode).not.toHaveBeenCalled();
    });

    it('broadcasts result to every iframe of the extension so either role (worker or view) can initiate', async () => {
      const service = makeService();
      const token = validToken();

      // Two iframes present (worker + view). Both have data-extension-id="ext.foo".
      const iframes = setMockIframes(2);

      vi.mocked(commands.oauthGetStoredToken).mockResolvedValue(null);
      vi.mocked(commands.oauthStartFlow).mockResolvedValue({
        state: 'state-broadcast',
        authUrl: 'https://example.com/auth',
      });
      await service.authorize(
        'ext.foo',
        'github',
        'cid',
        'https://example.com/auth',
        'https://example.com/token',
        [],
        'flow-broadcast',
      );

      vi.mocked(commands.oauthExchangeCode).mockResolvedValue({
        extensionId: 'ext.foo',
        flowId: 'flow-broadcast',
        token,
      });

      await (service as any)._handleCallback(
        'launcher://oauth/callback?code=c&state=state-broadcast',
      );

      // Both iframes received the broadcast. The non-initiator's SDK proxy
      // drops the message because its flowId does not match the registered
      // listener — contract documented in OAuthServiceProxy.authorize.
      for (const iframe of iframes) {
        expect(iframe.contentWindow.postMessage).toHaveBeenCalledWith(
          expect.objectContaining({
            type: 'launcher:oauth:result',
            flowId: 'flow-broadcast',
            token,
          }),
          expect.any(String),
        );
      }
    });
  });

  describe('authorizeForHost()', () => {
    const cfg = {
      clientId: 'cid',
      authorizationUrl: 'https://linear.app/oauth/authorize',
      tokenUrl: 'https://api.linear.app/oauth/token',
      scopes: ['read'],
    };

    async function begin(service: ExtensionOAuthService) {
      vi.mocked(commands.oauthStartFlow).mockResolvedValue({
        state: 'state-h',
        authUrl: 'https://linear.app/oauth/authorize?x=1',
      });
      const done = service.authorizeForHost('ext.foo', 'prov', cfg);
      await vi.waitFor(() => expect(openUrl).toHaveBeenCalled());
      const flowId = vi.mocked(commands.oauthStartFlow).mock.calls.at(-1)![6];
      return { done, flowId };
    }

    it('opens the browser with a host-prefixed flow id and resolves after the exchange', async () => {
      const service = makeService();
      const { done, flowId } = await begin(service);
      expect(flowId).toMatch(/^host:/);

      vi.mocked(commands.oauthExchangeCode).mockResolvedValue({
        extensionId: 'ext.foo',
        flowId,
        token: validToken(),
      });
      await service._handleCallback('launcher://oauth/callback?code=c&state=state-h');

      await expect(done).resolves.toBeUndefined();
    });

    it('does not broadcast host flow results to extension iframes', async () => {
      const service = makeService();
      const iframes = setMockIframes(1);
      const { done, flowId } = await begin(service);
      vi.mocked(commands.oauthExchangeCode).mockResolvedValue({
        extensionId: 'ext.foo',
        flowId,
        token: validToken(),
      });
      await service._handleCallback('launcher://oauth/callback?code=c&state=state-h');
      await done;
      expect(iframes[0].contentWindow.postMessage).not.toHaveBeenCalled();
    });

    it('rejects when the user denies access', async () => {
      const service = makeService();
      const { done } = await begin(service);
      await service._handleCallback('launcher://oauth/callback?error=access_denied&state=state-h');
      await expect(done).rejects.toThrow('access_denied');
    });

    it('rejects when the token exchange fails', async () => {
      const service = makeService();
      const { done } = await begin(service);
      vi.mocked(commands.oauthExchangeCode).mockRejectedValue(new Error('boom'));
      await service._handleCallback('launcher://oauth/callback?code=c&state=state-h');
      await expect(done).rejects.toThrow('exchange_failed');
    });

    it('rejects with a timeout when the browser flow never completes', async () => {
      vi.useFakeTimers();
      try {
        const service = makeService();
        const { done } = await begin(service);
        const assertion = expect(done).rejects.toThrow('timeout');
        await vi.advanceTimersByTimeAsync(5 * 60 * 1000 + 1);
        await assertion;
      } finally {
        vi.useRealTimers();
      }
    });
  });
});
