<script lang="ts">
  import Modal from '../base/Modal.svelte';
  import { Button } from '../index';
  import { t } from '../../services/i18n';
  import { connectPromptStore } from '../../services/connectors/connectPromptStore.svelte';
  import { connectorsService } from '../../services/connectors';
  import { commandService } from '../../services/extension/commandService.svelte';
  import { logService } from '../../services/log/logService';

  let error = $state('');
  let serverUrl = $state('');
  // Prefill the address once per prompted connector, then let the user edit it.
  let prefilledFor = $state('');

  const active = $derived(connectPromptStore.active);
  const target = $derived(active?.missing[0]);
  const busy = $derived(
    !!active && !!target && connectorsService.isConnecting(active.extensionId, target.id),
  );

  $effect(() => {
    const key = active && target ? `${active.extensionId}/${target.id}` : '';
    if (key && key !== prefilledFor) {
      prefilledFor = key;
      serverUrl = target?.server?.defaultUrl ?? '';
    }
  });

  const needsAddress = $derived(!!target?.server && serverUrl.trim() === '');

  async function handleConnect() {
    if (!active || !target || busy || needsAddress) return;
    const { extensionId, commandObjectId } = active;
    error = '';
    try {
      await connectorsService.connect(extensionId, target.id, { serverUrl });
    } catch (err) {
      error = t('settings.extensions.connectors.connect_error', {
        error: err instanceof Error ? err.message : String(err),
      });
      return;
    }

    // Other required connectors may still be missing; the prompt moves on to
    // the next one, and resumes the command once none are left.
    const stillMissing = await connectorsService.missingRequired(extensionId);
    if (stillMissing.length > 0) {
      connectPromptStore.open({ ...active, missing: stillMissing });
      return;
    }
    connectPromptStore.close();
    try {
      await commandService.executeCommand(commandObjectId);
    } catch (err) {
      logService.error(`ConnectPromptHost: command re-invocation failed: ${err}`);
    }
  }

  function handleCancel() {
    if (busy) return;
    error = '';
    connectPromptStore.close();
  }
</script>

{#if active && target}
  <Modal
    isOpen={true}
    labelledBy="connect-prompt-title"
    width="440px"
    onEscape={handleCancel}
    onEnter={handleConnect}
  >
    {#snippet children()}
      <h2 id="connect-prompt-title" class="modal-title">
        {t('settings.extensions.connectors.prompt_title', { name: target.name })}
      </h2>
      <p class="modal-subtitle">
        {t('settings.extensions.connectors.prompt_body', {
          extension: active.extensionId,
          name: target.name,
          command: active.commandId,
        })}
      </p>
      {#if target.server}
        <label class="server-field">
          <span>{t('settings.extensions.connectors.server_label')}</span>
          <input
            type="text"
            bind:value={serverUrl}
            placeholder={target.server.placeholder ??
              t('settings.extensions.connectors.server_placeholder')}
            disabled={busy}
            spellcheck="false"
            autocapitalize="off"
          />
        </label>
      {/if}
      {#if error}<p class="modal-error" role="alert">{error}</p>{/if}
    {/snippet}
    {#snippet actions()}
      <Button disabled={busy} onclick={handleCancel}>{t('common.cancel')}</Button>
      <Button class="btn-primary" disabled={busy || needsAddress} onclick={handleConnect}>
        {busy
          ? t('settings.extensions.connectors.connecting', { name: target.name })
          : t('settings.extensions.connectors.prompt_connect', { name: target.name })}
      </Button>
    {/snippet}
  </Modal>
{/if}

<style>
  .modal-title {
    margin: 0 0 var(--space-2) 0;
    font-size: var(--font-size-lg);
    font-weight: 600;
    color: var(--color-text);
    font-family: var(--font-ui);
  }

  .modal-subtitle {
    margin: 0 0 var(--space-4) 0;
    font-size: var(--font-size-sm);
    color: var(--color-text-muted);
    line-height: 1.5;
    font-family: var(--font-ui);
  }

  .server-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0 0 var(--space-4) 0;
    font-size: var(--font-size-sm);
    color: var(--color-text-muted);
    font-family: var(--font-ui);
  }

  .server-field input {
    background: var(--color-bg-subtle);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-3);
    font: inherit;
  }

  .modal-error {
    margin: 0 0 var(--space-4) 0;
    font-size: var(--font-size-sm);
    color: var(--color-danger);
    font-family: var(--font-ui);
  }
</style>
