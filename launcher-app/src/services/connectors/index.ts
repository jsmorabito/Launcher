import type { ConnectorDeclaration } from 'launcher-sdk/contracts';
import { extensionOAuthService } from '../oauth/extensionOAuthService.svelte';
import { ConnectorsService } from './connectorsService.svelte';

export { ConnectorsService } from './connectorsService.svelte';
export type { ConnectedAccount, ConnectResult } from './connectorsService.svelte';

type ConnectorsLookup = (extensionId: string) => ConnectorDeclaration[] | undefined;

let lookup: ConnectorsLookup = () => undefined;

/**
 * Supplies the manifest lookup. Called once by the composition root
 * (`buildServiceRegistry`) so this module never imports the extension manager.
 */
export function setConnectorsLookup(fn: ConnectorsLookup): void {
  lookup = fn;
}

export const connectorsService = new ConnectorsService(extensionOAuthService, (id) => lookup(id));
