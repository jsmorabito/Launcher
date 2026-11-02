/**
 * Wire contract between the Settings window and the main window for connector
 * actions. Sign-in must run in the main window: it owns the OAuth deep-link
 * listener and the pending-flow state, and it is where extension iframes live.
 * Settings therefore sends a request event and waits for the matching result.
 */
export const CONNECTOR_REQUEST_EVENT = 'launcher:connectors:request';
export const CONNECTOR_RESULT_EVENT = 'launcher:connectors:result';
/** Broadcast after any connect/disconnect so open windows can refresh. */
export const CONNECTOR_CHANGED_EVENT = 'launcher:connectors:changed';

export type ConnectorRequest =
  | {
      requestId: string;
      action: 'connect';
      extensionId: string;
      connectorId: string;
      serverUrl?: string;
    }
  | {
      requestId: string;
      action: 'disconnect';
      extensionId: string;
      connectorId: string;
      accountId: string;
    };

export interface ConnectorResult {
  requestId: string;
  ok: boolean;
  error?: string;
  accountName?: string;
  alreadyConnected?: boolean;
}

export interface ConnectorChanged {
  extensionId: string;
  connectorId: string;
}
