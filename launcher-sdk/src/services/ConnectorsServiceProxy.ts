import { BaseServiceProxy } from './BaseServiceProxy';
import type {
  ConnectorAccount,
  ConnectorFetchOptions,
  ConnectorFetchResponse,
  IConnectorsService,
} from './IConnectorsService';

/** SDK proxy for host-managed connector accounts. Requires `connectors:use`. */
export class ConnectorsServiceProxy extends BaseServiceProxy implements IConnectorsService {
  getAccounts(connectorId: string): Promise<ConnectorAccount[]> {
    return this.broker.invoke<ConnectorAccount[]>('connectors:getAccounts', { connectorId });
  }

  getToken(connectorId: string, accountId: string): Promise<string> {
    // Key order must match the host dispatch table: connectorId, accountId.
    return this.broker.invoke<string>('connectors:getToken', { connectorId, accountId });
  }

  fetch(
    connectorId: string,
    accountId: string,
    path: string,
    options: ConnectorFetchOptions = {},
  ): Promise<ConnectorFetchResponse> {
    // Key order must match the host dispatch table: connectorId, accountId, path, options.
    const timeout = options.timeout ?? 30_000;
    return this.broker.invoke<ConnectorFetchResponse>(
      'connectors:fetch',
      { connectorId, accountId, path, options },
      undefined,
      timeout + 5_000,
    );
  }

  onChange(callback: (connectorId: string) => void): () => void {
    const handler = (event: MessageEvent) => {
      if (event.data?.type !== 'launcher:connectors:changed') return;
      callback(String(event.data.connectorId));
    };
    window.addEventListener('message', handler);
    return () => window.removeEventListener('message', handler);
  }
}
