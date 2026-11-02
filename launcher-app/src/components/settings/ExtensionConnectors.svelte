<script lang="ts">
  import { onMount } from 'svelte';
  import type { ConnectorDeclaration } from 'launcher-sdk/contracts';
  import Badge from '../base/Badge.svelte';
  import { Button } from '../index';
  import { t } from '../../services/i18n';
  import {
    connectConnector,
    disconnectConnector,
    listConnectorAccounts,
    onConnectorsChanged,
    type ConnectorAccountView,
  } from '../../services/connectors/connectorsClient';

  let { extensionId, connectors }: { extensionId: string; connectors: ConnectorDeclaration[] } =
    $props();

  let accounts = $state<Record<string, ConnectorAccountView[]>>({});
  let busy = $state<Record<string, boolean>>({});
  let errors = $state<Record<string, string>>({});
  // The address typed for connectors that point at a user-supplied server.
  let serverUrls = $state<Record<string, string>>({});

  const serverUrlFor = (c: ConnectorDeclaration) => serverUrls[c.id] ?? c.server?.defaultUrl ?? '';

  async function load(connectorId: string) {
    accounts[connectorId] = await listConnectorAccounts(extensionId, connectorId);
  }

  // Reload whenever the selected extension (or its connectors) changes.
  $effect(() => {
    for (const c of connectors) void load(c.id);
  });

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onConnectorsChanged((e) => {
      if (e.extensionId === extensionId) void load(e.connectorId);
    }).then((fn) => (disposed ? fn() : (unlisten = fn)));
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  async function run(c: ConnectorDeclaration, action: () => ReturnType<typeof connectConnector>) {
    busy[c.id] = true;
    errors[c.id] = '';
    try {
      const res = await action();
      if (!res.ok) {
        errors[c.id] = t('settings.extensions.connectors.connect_error', {
          error: res.error ?? '',
        });
      } else if (res.alreadyConnected) {
        errors[c.id] = t('settings.extensions.connectors.already_connected', {
          name: res.accountName ?? c.name,
        });
      }
    } finally {
      busy[c.id] = false;
      await load(c.id);
    }
  }

  const canAdd = (c: ConnectorDeclaration) => c.multiAccount || (accounts[c.id] ?? []).length === 0;
</script>

<div class="connectors">
  {#each connectors as c (c.id)}
    {@const list = accounts[c.id] ?? []}
    <div class="connector">
      <div class="connector-head">
        <span class="connector-name">{c.name}</span>
        {#if c.required}<Badge text={t('settings.extensions.connectors.required_badge')} />{/if}
      </div>
      {#if c.description}<p class="connector-desc">{c.description}</p>{/if}

      {#if list.length === 0}
        <p class="empty">{t('settings.extensions.connectors.not_connected')}</p>
      {/if}
      {#each list as a (a.accountId)}
        <div class="account">
          <span class="account-name">{a.displayName}</span>
          <Button
            disabled={busy[c.id]}
            onclick={() => run(c, () => disconnectConnector(extensionId, c.id, a.accountId))}
          >
            {t('settings.extensions.connectors.disconnect')}
          </Button>
        </div>
      {/each}

      {#if canAdd(c)}
        {#if c.server}
          <label class="server-field">
            <span>{t('settings.extensions.connectors.server_label')}</span>
            <input
              type="text"
              value={serverUrlFor(c)}
              placeholder={c.server.placeholder ??
                t('settings.extensions.connectors.server_placeholder')}
              disabled={busy[c.id]}
              spellcheck="false"
              autocapitalize="off"
              oninput={(e) => (serverUrls[c.id] = e.currentTarget.value)}
            />
          </label>
        {/if}
        <Button
          class="btn-primary"
          disabled={busy[c.id] || (!!c.server && serverUrlFor(c).trim() === '')}
          onclick={() => run(c, () => connectConnector(extensionId, c.id, serverUrlFor(c)))}
        >
          {#if busy[c.id]}
            {t('settings.extensions.connectors.connecting', { name: c.name })}
          {:else if list.length === 0}
            {t('settings.extensions.connectors.connect')}
          {:else}
            {t('settings.extensions.connectors.connect_another')}
          {/if}
        </Button>
      {/if}
      {#if errors[c.id]}<p class="error" role="alert">{errors[c.id]}</p>{/if}
    </div>
  {/each}
</div>

<style>
  .connectors {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .connector {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: flex-start;
  }

  .connector-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .connector-name {
    font-weight: 600;
    color: var(--color-text);
  }

  .connector-desc,
  .empty {
    margin: 0;
    font-size: var(--font-size-sm);
    color: var(--color-text-muted);
  }

  .account {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg-subtle);
    border-radius: var(--radius-md);
  }

  .server-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    width: 100%;
    font-size: var(--font-size-sm);
    color: var(--color-text-muted);
  }

  .server-field input {
    background: var(--color-bg-subtle);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-3);
    font: inherit;
  }

  .account-name {
    color: var(--color-text);
    font-size: var(--font-size-sm);
  }

  .error {
    margin: 0;
    font-size: var(--font-size-sm);
    color: var(--color-danger);
  }
</style>
