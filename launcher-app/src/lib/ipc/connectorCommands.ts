// launcher-app/src/lib/ipc/connectorCommands.ts
// Tauri command wrappers for host-managed connector accounts, re-exported
// through ./commands (the barrel).
import { invokeSafe } from './invokeSafe';

export interface ConnectorAccountRecord {
  connectorId: string;
  accountId: string;
  displayName: string;
  createdAt: number;
  serverUrl?: string | null;
}

export interface DynamicConnectStart {
  state: string;
  authUrl: string;
  serverUrl: string;
  clientId: string;
  tokenUrl: string;
}

export interface ConnectorAccountProbe {
  url: string;
  method?: string;
  body?: string;
  idPath: string;
  namePath: string;
}

export interface ConnectorFinalizeResponse {
  account: ConnectorAccountRecord;
  alreadyConnected: boolean;
}

export async function connectorListAccounts(
  extensionId: string,
  connectorId: string,
): Promise<ConnectorAccountRecord[]> {
  return invokeSafe<ConnectorAccountRecord[]>('connector_list_accounts', {
    extensionId,
    connectorId,
  });
}

export async function connectorFinalizeConnect(
  extensionId: string,
  connectorId: string,
  pendingProviderId: string,
  authorizationUrl: string,
  tokenUrl: string,
  probe: ConnectorAccountProbe,
): Promise<ConnectorFinalizeResponse> {
  return invokeSafe<ConnectorFinalizeResponse>('connector_finalize_connect', {
    extensionId,
    connectorId,
    pendingProviderId,
    authorizationUrl,
    tokenUrl,
    probe,
  });
}

export async function connectorGetToken(
  extensionId: string,
  connectorId: string,
  accountId: string,
  tokenUrl: string,
  clientId: string,
): Promise<string> {
  return invokeSafe<string>('connector_get_token', {
    extensionId,
    connectorId,
    accountId,
    tokenUrl,
    clientId,
  });
}

export async function connectorDisconnect(
  extensionId: string,
  connectorId: string,
  accountId: string,
): Promise<void> {
  await invokeSafe('connector_disconnect', { extensionId, connectorId, accountId });
}

export async function connectorBeginDynamic(
  extensionId: string,
  pendingProviderId: string,
  flowId: string,
  serverUrl: string,
  scopes: string[],
  clientName: string,
): Promise<DynamicConnectStart> {
  return invokeSafe<DynamicConnectStart>('connector_begin_dynamic', {
    extensionId,
    pendingProviderId,
    flowId,
    serverUrl,
    scopes,
    clientName,
  });
}

export async function connectorFinalizeDynamic(
  extensionId: string,
  connectorId: string,
  pendingProviderId: string,
  serverUrl: string,
  clientId: string,
  tokenUrl: string,
): Promise<ConnectorFinalizeResponse> {
  return invokeSafe<ConnectorFinalizeResponse>('connector_finalize_dynamic', {
    extensionId,
    connectorId,
    pendingProviderId,
    serverUrl,
    clientId,
    tokenUrl,
  });
}

export interface ConnectorFetchResult {
  status: number;
  ok: boolean;
  body: string;
}

export async function connectorFetch(
  extensionId: string,
  connectorId: string,
  accountId: string,
  path: string,
  options: {
    method?: string;
    headers?: Record<string, string>;
    body?: string;
    timeout?: number;
  } = {},
): Promise<ConnectorFetchResult> {
  return invokeSafe<ConnectorFetchResult>('connector_fetch', {
    extensionId,
    connectorId,
    accountId,
    path,
    method: options.method ?? null,
    headers: options.headers ?? null,
    body: options.body ?? null,
    timeoutMs: options.timeout ?? null,
  });
}
