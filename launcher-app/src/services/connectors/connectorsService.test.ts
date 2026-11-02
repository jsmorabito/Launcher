import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { ConnectorDeclaration } from 'launcher-sdk/contracts';

vi.mock('../../lib/ipc/commands', () => ({
  connectorListAccounts: vi.fn(),
  connectorFinalizeConnect: vi.fn(),
  connectorGetToken: vi.fn(),
  connectorDisconnect: vi.fn(),
  connectorBeginDynamic: vi.fn(),
  connectorFinalizeDynamic: vi.fn(),
  connectorFetch: vi.fn(),
}));
vi.mock('../../lib/ipc/extensionOrigin', () => ({
  getExtensionFrameOrigin: vi.fn().mockReturnValue('null'),
}));
vi.mock('../log/logService', () => ({
  logService: { error: vi.fn(), warn: vi.fn(), info: vi.fn() },
}));

import * as commands from '../../lib/ipc/commands';
import { ConnectorsService } from './connectorsService.svelte';

const decl = (over: Partial<ConnectorDeclaration> = {}): ConnectorDeclaration => ({
  id: 'linear',
  name: 'Linear',
  multiAccount: true,
  required: true,
  oauth: {
    clientId: 'cid',
    authorizationUrl: 'https://linear.app/oauth/authorize',
    tokenUrl: 'https://api.linear.app/oauth/token',
    scopes: ['read'],
  },
  account: {
    url: 'https://api.linear.app/graphql',
    method: 'POST',
    idPath: 'data.organization.id',
    namePath: 'data.organization.name',
  },
  ...over,
});

const record = (accountId: string, displayName = accountId) => ({
  connectorId: 'linear',
  accountId,
  displayName,
  createdAt: 1,
});

function setup(declarations: ConnectorDeclaration[] | null = [decl()]) {
  const oauth = {
    authorizeForHost: vi.fn().mockResolvedValue(undefined),
    trackHostFlow: vi.fn().mockResolvedValue(undefined),
    newHostFlowId: vi.fn().mockReturnValue('host:flow-1'),
  };
  const service = new ConnectorsService(oauth, () => declarations ?? undefined);
  return { service, oauth };
}

describe('ConnectorsService', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.connectorListAccounts).mockResolvedValue([]);
  });

  describe('extension surface', () => {
    it('rejects connectors the extension did not declare', async () => {
      const { service } = setup([decl()]);
      await expect(service.getAccounts('ext.a', 'github')).rejects.toThrow(
        'connector_not_declared',
      );
      await expect(service.getToken('ext.a', 'github', 'x')).rejects.toThrow(
        'connector_not_declared',
      );
      expect(commands.connectorGetToken).not.toHaveBeenCalled();
    });

    it('rejects when the extension declares no connectors at all', async () => {
      const { service } = setup(null);
      await expect(service.getAccounts('ext.a', 'linear')).rejects.toThrow(
        'connector_not_declared',
      );
    });

    it('getAccounts returns id and name only', async () => {
      vi.mocked(commands.connectorListAccounts).mockResolvedValue([record('o1', 'Acme')]);
      const { service } = setup();
      expect(await service.getAccounts('ext.a', 'linear')).toEqual([
        { accountId: 'o1', displayName: 'Acme' },
      ]);
    });

    it('getToken passes the declared token url and client id for refresh', async () => {
      vi.mocked(commands.connectorGetToken).mockResolvedValue('tok');
      const { service } = setup();
      expect(await service.getToken('ext.a', 'linear', 'o1')).toBe('tok');
      expect(commands.connectorGetToken).toHaveBeenCalledWith(
        'ext.a',
        'linear',
        'o1',
        'https://api.linear.app/oauth/token',
        'cid',
      );
    });
  });

  describe('connect()', () => {
    it('runs the host OAuth flow then finalizes under the pending provider id', async () => {
      vi.mocked(commands.connectorFinalizeConnect).mockResolvedValue({
        account: record('o1', 'Acme'),
        alreadyConnected: false,
      });
      const { service, oauth } = setup();

      const res = await service.connect('ext.a', 'linear');

      const [, pendingId, cfg] = oauth.authorizeForHost.mock.calls[0];
      expect(pendingId).toMatch(/^connector-pending:/);
      expect(cfg).toMatchObject({ clientId: 'cid', scopes: ['read'] });
      const finalizeArgs = vi.mocked(commands.connectorFinalizeConnect).mock.calls[0];
      expect(finalizeArgs[2]).toBe(pendingId);
      expect(res).toEqual({
        account: { accountId: 'o1', displayName: 'Acme' },
        alreadyConnected: false,
      });
    });

    it('notifies listeners and clears the connecting flag', async () => {
      vi.mocked(commands.connectorFinalizeConnect).mockResolvedValue({
        account: record('o1'),
        alreadyConnected: false,
      });
      const { service } = setup();
      const listener = vi.fn();
      service.onChange(listener);

      await service.connect('ext.a', 'linear');

      expect(listener).toHaveBeenCalledWith('ext.a', 'linear');
      expect(service.isConnecting('ext.a', 'linear')).toBe(false);
    });

    it('clears the connecting flag when sign-in is denied and does not finalize', async () => {
      const { service, oauth } = setup();
      oauth.authorizeForHost.mockRejectedValue(new Error('OAuth error [access_denied]'));
      await expect(service.connect('ext.a', 'linear')).rejects.toThrow('access_denied');
      expect(service.isConnecting('ext.a', 'linear')).toBe(false);
      expect(commands.connectorFinalizeConnect).not.toHaveBeenCalled();
    });

    it('refuses a second concurrent connect for the same connector', async () => {
      let release!: () => void;
      const { service, oauth } = setup();
      oauth.authorizeForHost.mockReturnValue(new Promise<void>((r) => (release = r)));
      vi.mocked(commands.connectorFinalizeConnect).mockResolvedValue({
        account: record('o1'),
        alreadyConnected: false,
      });

      const first = service.connect('ext.a', 'linear');
      await vi.waitFor(() => expect(service.isConnecting('ext.a', 'linear')).toBe(true));
      await expect(service.connect('ext.a', 'linear')).rejects.toThrow('already in progress');
      release();
      await first;
    });

    it('replaces the previous account for a single-account connector', async () => {
      vi.mocked(commands.connectorListAccounts).mockResolvedValue([record('old')]);
      vi.mocked(commands.connectorFinalizeConnect).mockResolvedValue({
        account: record('new'),
        alreadyConnected: false,
      });
      const { service } = setup([decl({ multiAccount: false })]);

      await service.connect('ext.a', 'linear');

      expect(commands.connectorDisconnect).toHaveBeenCalledWith('ext.a', 'linear', 'old');
    });

    it('keeps existing accounts for a multi-account connector', async () => {
      vi.mocked(commands.connectorListAccounts).mockResolvedValue([record('old')]);
      vi.mocked(commands.connectorFinalizeConnect).mockResolvedValue({
        account: record('new'),
        alreadyConnected: false,
      });
      const { service } = setup();

      await service.connect('ext.a', 'linear');

      expect(commands.connectorDisconnect).not.toHaveBeenCalled();
    });

    it('does not drop the account when a single-account connector re-signs in as the same one', async () => {
      vi.mocked(commands.connectorListAccounts).mockResolvedValue([record('same')]);
      vi.mocked(commands.connectorFinalizeConnect).mockResolvedValue({
        account: record('same'),
        alreadyConnected: true,
      });
      const { service } = setup([decl({ multiAccount: false })]);

      const res = await service.connect('ext.a', 'linear');

      expect(res.alreadyConnected).toBe(true);
      expect(commands.connectorDisconnect).not.toHaveBeenCalled();
    });
  });

  describe('disconnect() and gating', () => {
    it('disconnects and notifies', async () => {
      const { service } = setup();
      const listener = vi.fn();
      service.onChange(listener);
      await service.disconnect('ext.a', 'linear', 'o1');
      expect(commands.connectorDisconnect).toHaveBeenCalledWith('ext.a', 'linear', 'o1');
      expect(listener).toHaveBeenCalledWith('ext.a', 'linear');
    });

    it('reports required connectors that have no account', async () => {
      const { service } = setup([decl(), decl({ id: 'optional', required: false })]);
      expect((await service.missingRequired('ext.a')).map((c) => c.id)).toEqual(['linear']);
      expect(await service.hasRequiredConnections('ext.a')).toBe(false);

      vi.mocked(commands.connectorListAccounts).mockResolvedValue([record('o1')]);
      expect(await service.missingRequired('ext.a')).toEqual([]);
      expect(await service.hasRequiredConnections('ext.a')).toBe(true);
    });

    it('an extension without connectors is never gated', async () => {
      const { service } = setup(null);
      expect(await service.hasRequiredConnections('ext.a')).toBe(true);
      expect(commands.connectorListAccounts).not.toHaveBeenCalled();
    });
  });

  describe('server connectors', () => {
    const tasks = (over: Partial<ConnectorDeclaration> = {}): ConnectorDeclaration => ({
      id: 'tasks',
      name: 'Tasks',
      multiAccount: true,
      server: { defaultUrl: 'http://localhost:4100', scopes: ['tasks'], clientName: 'Launcher' },
      ...over,
    });
    const serverRecord = {
      connectorId: 'tasks',
      accountId: 'https://tasks.example.com',
      displayName: 'tasks.example.com',
      createdAt: 1,
      serverUrl: 'https://tasks.example.com',
    };
    const start = {
      state: 'st',
      authUrl: 'https://tasks.example.com/oauth/authorize?x=1',
      serverUrl: 'https://tasks.example.com',
      clientId: 'cid-dyn',
      tokenUrl: 'https://tasks.example.com/oauth/token',
    };

    beforeEach(() => {
      vi.mocked(commands.connectorBeginDynamic).mockResolvedValue(start);
      vi.mocked(commands.connectorFinalizeDynamic).mockResolvedValue({
        account: serverRecord,
        alreadyConnected: false,
      });
    });

    it('requires the address the user typed', async () => {
      const { service } = setup([tasks()]);
      await expect(service.connect('ext.a', 'tasks')).rejects.toThrow('Enter the address');
      await expect(service.connect('ext.a', 'tasks', { serverUrl: '  ' })).rejects.toThrow(
        'Enter the address',
      );
      expect(commands.connectorBeginDynamic).not.toHaveBeenCalled();
      expect(service.isConnecting('ext.a', 'tasks')).toBe(false);
    });

    it('discovers and registers in Rust, opens the browser, then finalizes with the registered client', async () => {
      const { service, oauth } = setup([tasks()]);

      const res = await service.connect('ext.a', 'tasks', { serverUrl: 'tasks.example.com' });

      const begin = vi.mocked(commands.connectorBeginDynamic).mock.calls[0];
      expect(begin[2]).toBe('host:flow-1');
      expect(begin.slice(3)).toEqual(['tasks.example.com', ['tasks'], 'Launcher']);
      expect(oauth.trackHostFlow).toHaveBeenCalledWith('ext.a', 'host:flow-1', 'st', start.authUrl);
      expect(oauth.authorizeForHost).not.toHaveBeenCalled();
      const fin = vi.mocked(commands.connectorFinalizeDynamic).mock.calls[0];
      expect(fin.slice(1)).toEqual([
        'tasks',
        begin[1],
        'https://tasks.example.com',
        'cid-dyn',
        'https://tasks.example.com/oauth/token',
      ]);
      expect(res.account).toEqual({
        accountId: 'https://tasks.example.com',
        displayName: 'tasks.example.com',
        serverUrl: 'https://tasks.example.com',
      });
    });

    it('does not finalize when the browser sign-in is denied', async () => {
      const { service, oauth } = setup([tasks()]);
      oauth.trackHostFlow.mockRejectedValue(new Error('OAuth error [access_denied]'));
      await expect(
        service.connect('ext.a', 'tasks', { serverUrl: 'https://tasks.example.com' }),
      ).rejects.toThrow('access_denied');
      expect(commands.connectorFinalizeDynamic).not.toHaveBeenCalled();
      expect(service.isConnecting('ext.a', 'tasks')).toBe(false);
    });

    it('surfaces discovery errors from Rust without opening the browser', async () => {
      const { service, oauth } = setup([tasks()]);
      vi.mocked(commands.connectorBeginDynamic).mockRejectedValue(new Error('Could not reach'));
      await expect(
        service.connect('ext.a', 'tasks', { serverUrl: 'https://nope.example.com' }),
      ).rejects.toThrow('Could not reach');
      expect(oauth.trackHostFlow).not.toHaveBeenCalled();
    });

    it('exposes the server address to the extension', async () => {
      vi.mocked(commands.connectorListAccounts).mockResolvedValue([serverRecord]);
      const { service } = setup([tasks()]);
      expect(await service.getAccounts('ext.a', 'tasks')).toEqual([
        {
          accountId: 'https://tasks.example.com',
          displayName: 'tasks.example.com',
          serverUrl: 'https://tasks.example.com',
        },
      ]);
    });

    it('fetch goes through the host for server connectors', async () => {
      vi.mocked(commands.connectorFetch).mockResolvedValue({ status: 200, ok: true, body: '{}' });
      const { service } = setup([tasks()]);
      const res = await service.fetch('ext.a', 'tasks', 'https://t.example.com', '/mcp', {
        method: 'POST',
        body: '{}',
      });
      expect(res.ok).toBe(true);
      expect(commands.connectorFetch).toHaveBeenCalledWith(
        'ext.a',
        'tasks',
        'https://t.example.com',
        '/mcp',
        { method: 'POST', body: '{}' },
      );
    });

    it('fetch is refused for fixed-provider connectors and undeclared ones', async () => {
      const { service } = setup([decl(), tasks()]);
      await expect(service.fetch('ext.a', 'linear', 'o1', '/x')).rejects.toThrow(
        'connector_not_server',
      );
      await expect(service.fetch('ext.a', 'nope', 'o1', '/x')).rejects.toThrow(
        'connector_not_declared',
      );
      expect(commands.connectorFetch).not.toHaveBeenCalled();
    });

    it('asks Rust to refresh with stored endpoints (no manifest oauth)', async () => {
      vi.mocked(commands.connectorGetToken).mockResolvedValue('tok');
      const { service } = setup([tasks()]);
      await service.getToken('ext.a', 'tasks', 'https://tasks.example.com');
      expect(commands.connectorGetToken).toHaveBeenCalledWith(
        'ext.a',
        'tasks',
        'https://tasks.example.com',
        '',
        '',
      );
    });
  });
});
