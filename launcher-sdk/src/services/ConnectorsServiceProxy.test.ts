import { describe, expect, it, vi } from 'vitest';
import { ConnectorsServiceProxy } from './ConnectorsServiceProxy';

function makeProxy() {
  const proxy = new ConnectorsServiceProxy();
  const invoke = vi.fn().mockResolvedValue(undefined);
  (proxy as unknown as { broker: { invoke: typeof invoke } }).broker = { invoke };
  return { proxy, invoke };
}

describe('ConnectorsServiceProxy', () => {
  it('getAccounts sends connectorId', async () => {
    const { proxy, invoke } = makeProxy();
    await proxy.getAccounts('linear');
    expect(invoke).toHaveBeenCalledWith('connectors:getAccounts', { connectorId: 'linear' });
  });

  it('getToken sends connectorId then accountId in dispatch order', async () => {
    const { proxy, invoke } = makeProxy();
    await proxy.getToken('linear', 'org1');
    const [, payload] = invoke.mock.calls[0];
    expect(Object.keys(payload)).toEqual(['connectorId', 'accountId']);
  });

  it('fetch sends connectorId, accountId, path, options in dispatch order', async () => {
    const { proxy, invoke } = makeProxy();
    await proxy.fetch('tasks', 'https://t.example.com', '/mcp', { method: 'POST', body: '{}' });
    const [type, payload, , timeout] = invoke.mock.calls[0];
    expect(type).toBe('connectors:fetch');
    expect(Object.keys(payload)).toEqual(['connectorId', 'accountId', 'path', 'options']);
    expect(payload.options).toEqual({ method: 'POST', body: '{}' });
    expect(timeout).toBe(35_000);
  });

  it('fetch allows the broker to wait longer than a custom request timeout', async () => {
    const { proxy, invoke } = makeProxy();
    await proxy.fetch('tasks', 'a', '/x', { timeout: 60_000 });
    expect(invoke.mock.calls[0][3]).toBe(65_000);
  });

  it('onChange fires for connector events only and unsubscribes', () => {
    const { proxy } = makeProxy();
    const cb = vi.fn();
    const off = proxy.onChange(cb);
    window.dispatchEvent(
      new MessageEvent('message', {
        data: { type: 'launcher:connectors:changed', connectorId: 'linear' },
      }),
    );
    window.dispatchEvent(new MessageEvent('message', { data: { type: 'other' } }));
    expect(cb).toHaveBeenCalledTimes(1);
    expect(cb).toHaveBeenCalledWith('linear');
    off();
    window.dispatchEvent(
      new MessageEvent('message', {
        data: { type: 'launcher:connectors:changed', connectorId: 'x' },
      }),
    );
    expect(cb).toHaveBeenCalledTimes(1);
  });
});
