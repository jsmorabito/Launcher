import { emit, listen } from '@tauri-apps/api/event';
import * as commands from '../../lib/ipc/commands';
import {
  CONNECTOR_CHANGED_EVENT,
  CONNECTOR_REQUEST_EVENT,
  CONNECTOR_RESULT_EVENT,
  type ConnectorChanged,
  type ConnectorRequest,
  type ConnectorResult,
} from './connectorsBridge';

/** A connect waits on the user's browser, so allow longer than the OAuth timeout. */
const REQUEST_TIMEOUT_MS = 6 * 60 * 1000;

export interface ConnectorAccountView {
  accountId: string;
  displayName: string;
  serverUrl?: string;
}

/** Accounts for one connector, read straight from the database. */
export async function listConnectorAccounts(
  extensionId: string,
  connectorId: string,
): Promise<ConnectorAccountView[]> {
  const records = await commands.connectorListAccounts(extensionId, connectorId);
  return records.map((r) => ({
    accountId: r.accountId,
    displayName: r.displayName,
    ...(r.serverUrl ? { serverUrl: r.serverUrl } : {}),
  }));
}

type DistributiveOmit<T, K extends keyof T> = T extends unknown ? Omit<T, K> : never;

async function request(
  req: DistributiveOmit<ConnectorRequest, 'requestId'>,
): Promise<ConnectorResult> {
  const requestId = crypto.randomUUID();
  let settle!: (r: ConnectorResult) => void;
  const done = new Promise<ConnectorResult>((resolve) => (settle = resolve));

  // Listen before emitting so a fast reply cannot be missed.
  const unlisten = await listen<ConnectorResult>(CONNECTOR_RESULT_EVENT, (e) => {
    if (e.payload.requestId === requestId) settle(e.payload);
  });
  const timer = setTimeout(
    () => settle({ requestId, ok: false, error: 'Timed out waiting for Launcher' }),
    REQUEST_TIMEOUT_MS,
  );
  try {
    await emit(CONNECTOR_REQUEST_EVENT, { ...req, requestId });
    return await done;
  } finally {
    clearTimeout(timer);
    unlisten();
  }
}

/** Ask the main window to run the sign-in flow for a connector. */
export function connectConnector(extensionId: string, connectorId: string, serverUrl?: string) {
  return request({ action: 'connect', extensionId, connectorId, serverUrl });
}

export function disconnectConnector(extensionId: string, connectorId: string, accountId: string) {
  return request({ action: 'disconnect', extensionId, connectorId, accountId });
}

/** Subscribe to account changes made in any window. */
export function onConnectorsChanged(cb: (e: ConnectorChanged) => void): Promise<() => void> {
  return listen<ConnectorChanged>(CONNECTOR_CHANGED_EVENT, (e) => cb(e.payload));
}
