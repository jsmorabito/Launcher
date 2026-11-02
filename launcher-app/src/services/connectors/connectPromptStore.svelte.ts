import type { ConnectorDeclaration } from 'launcher-sdk/contracts';

/**
 * Reactive store for the "connect an account" modal. The commandService opens
 * it when a command's extension has `required` connectors with no account yet;
 * ConnectPromptHost renders the modal and re-runs the command once connected.
 *
 * `commandObjectId` is the composite id stored by commandService so the host
 * can resume execution after the user connects.
 */
interface ConnectPromptState {
  extensionId: string;
  commandId: string;
  commandObjectId: string;
  missing: ConnectorDeclaration[];
}

class ConnectPromptStore {
  active = $state<ConnectPromptState | null>(null);

  open(state: ConnectPromptState): void {
    this.active = state;
  }

  close(): void {
    this.active = null;
  }
}

export const connectPromptStore = new ConnectPromptStore();
