import { emit } from '@tauri-apps/api/event';
import { appListen } from '../../lib/ipc/bridgeEvents';
import { logService } from '../log/logService';
import type { ConnectorsService } from './connectorsService.svelte';
import {
  CONNECTOR_CHANGED_EVENT,
  CONNECTOR_REQUEST_EVENT,
  CONNECTOR_RESULT_EVENT,
  type ConnectorChanged,
  type ConnectorRequest,
  type ConnectorResult,
} from './connectorsBridge';

const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * Main-window side of the Settings bridge: runs connect/disconnect requests
 * through `service` and reports the outcome. Returns a teardown function.
 */
export async function startConnectorsHost(service: ConnectorsService): Promise<() => void> {
  const unlistenRequests = await appListen<ConnectorRequest>(
    CONNECTOR_REQUEST_EVENT,
    async (event) => {
      const req = event.payload;
      const result: ConnectorResult = { requestId: req.requestId, ok: true };
      try {
        if (req.action === 'connect') {
          const res = await service.connect(req.extensionId, req.connectorId, {
            serverUrl: req.serverUrl,
          });
          result.accountName = res.account.displayName;
          result.alreadyConnected = res.alreadyConnected;
        } else {
          await service.disconnect(req.extensionId, req.connectorId, req.accountId);
        }
      } catch (e) {
        result.ok = false;
        result.error = messageOf(e);
        logService.warn(`connectors: ${req.action} failed: ${result.error}`);
      }
      await emit(CONNECTOR_RESULT_EVENT, result);
    },
  );

  // Tell other windows (Settings) whenever accounts change, including changes
  // made from the main window's own connect prompt.
  const unsubscribe = service.onChange((extensionId, connectorId) => {
    const payload: ConnectorChanged = { extensionId, connectorId };
    void emit(CONNECTOR_CHANGED_EVENT, payload);
  });

  return () => {
    unlistenRequests();
    unsubscribe();
  };
}
