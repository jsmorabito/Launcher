import type { IpcPendingMessage } from '../../lib/ipc/iframeLifecycleCommands';

const WIRE: Record<IpcPendingMessage['kind'], string | null> = {
  command: 'launcher:command:execute',
  action: 'launcher:action:execute',
  viewSubmit: 'launcher:view:submit',
  viewSearch: 'launcher:view:search',
  predictiveWarm: null,
};

export function toWireMessage(
  message: IpcPendingMessage,
): { type: string; payload: Record<string, unknown> } | null {
  const type = WIRE[message.kind];
  if (!type) return null;
  return { type, payload: message.payload };
}
