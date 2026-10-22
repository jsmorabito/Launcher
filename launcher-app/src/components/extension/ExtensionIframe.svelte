<script lang="ts">
  import { logService as logger } from '../../services/log/logService';
  import { extensionIframeManager } from '../../services/extension/extensionIframeManager.svelte';
  import { viewRegistry } from '../../services/extension/viewRegistry.svelte';
  import type { ExtensionManifest } from 'launcher-sdk/contracts';
  import { collectThemeVariables } from '../../lib/themeVariables';
  import { buildFontFaceCSS } from '../../lib/themeFonts';
  import { feedbackService } from '../../services/feedback/feedbackService.svelte';
  import { t } from '../../services/i18n';
  import Button from '../base/Button.svelte';
  import {
    announceActiveViewShortcuts,
    forgetAnnouncedShortcuts,
  } from '../../services/action/viewShortcutSync.svelte';

  let {
    extensionId,
    manifest,
    view = null,
  }: {
    extensionId: string;
    manifest: ExtensionManifest | null;
    view?: string | null;
  } = $props();

  let iframeElement = $state<HTMLIFrameElement>();
  let fontCSS: Promise<string> | null = null;

  const isWindows = navigator.userAgent.toLowerCase().includes('windows');

  let iframeSrc = $derived(
    isWindows
      ? `http://launcher-extension.localhost/${extensionId}/view.html${view ? `?view=${view.split('/')[1] || 'DefaultView'}` : ''}`
      : `launcher-extension://${extensionId}/view.html${view ? `?view=${view.split('/')[1] || 'DefaultView'}` : ''}`,
  );

  // A view that never announces `launcher:extension:loaded` (missing or stale
  // bundle, script error before the SDK boots) would otherwise render as an
  // empty pane, so surface it and offer a reload.
  const LOAD_TIMEOUT_MS = 8000;
  let loaded = $state(false);
  let loadFailed = $state(false);
  let reloadNonce = $state(0);
  let armedSrc = '';

  let mountToken = $derived(viewRegistry.getEntry(extensionId)?.mountToken);

  function reloadView() {
    loaded = false;
    loadFailed = false;
    reloadNonce += 1;
  }

  function handleIframeError() {
    loadFailed = true;
    feedbackService.report({
      source: 'extension',
      kind: 'extension_crash',
      severity: 'error',
      retryable: true,
      context: { extensionId, role: 'view' },
      extensionId,
    });
  }

  function handleMessage(event: MessageEvent) {
    if (!iframeElement || event.source !== iframeElement.contentWindow) return;
    const { type, payload } = event.data;
    if (type === 'launcher:extension:loaded') {
      loaded = true;
      loadFailed = false;
      return;
    }
    if (type === 'launcher:feedback:uncaught') {
      feedbackService.report({
        source: 'extension',
        kind:
          payload?.kind === 'iframe_unhandled_rejection'
            ? 'iframe_unhandled_rejection'
            : 'iframe_uncaught',
        severity: 'error',
        retryable: false,
        context: { extensionId, role: 'view' },
        extensionId,
        developerDetail: payload?.developerDetail,
      });
      return;
    }
    if (type === 'launcher:extension:input-focus') {
      extensionIframeManager.hasInputFocus = !!payload?.focused;
      return;
    }
    if (type === 'launcher:extension:keydown') {
      const { key, code, metaKey, ctrlKey, shiftKey, altKey, repeat, isComposing } = payload || {};
      const syntheticEvent = new KeyboardEvent('keydown', {
        key,
        code,
        repeat: !!repeat,
        isComposing: !!isComposing,
        metaKey,
        ctrlKey,
        shiftKey,
        altKey,
        bubbles: true,
        cancelable: true,
      });
      window.dispatchEvent(syntheticEvent);
      return;
    }
    logger.debug(`Received message from iframe (${extensionId}): ${type}`);
  }

  async function handleIframeLoad() {
    sendMessage('launcher:theme:variables', collectThemeVariables(document.documentElement));
    // A fresh load starts with no announced shortcuts: forget what this
    // extension was last sent, then resend the live set if its view is active.
    forgetAnnouncedShortcuts(extensionId);
    announceActiveViewShortcuts();
    const css = await (fontCSS ?? buildFontFaceCSS());
    sendMessage('launcher:theme:fonts', css);
  }

  $effect(() => {
    logger.info(`ExtensionIframe mounted for ${extensionId}`);
    window.addEventListener('message', handleMessage);

    // Warm up the font cache eagerly
    fontCSS = buildFontFaceCSS();

    const observer = new MutationObserver(() => {
      sendMessage('launcher:theme:variables', collectThemeVariables(document.documentElement));
    });
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    });

    return () => {
      window.removeEventListener('message', handleMessage);
      observer.disconnect();
      extensionIframeManager.hasInputFocus = false;
    };
  });

  $effect(() => {
    // Re-arm whenever the view target changes or the user reloads.
    void reloadNonce;
    if (iframeSrc !== armedSrc) {
      armedSrc = iframeSrc;
      loaded = false;
      loadFailed = false;
    }
    if (loaded) return;
    const timer = setTimeout(() => {
      if (!loaded) {
        loadFailed = true;
        logger.warn(
          `Extension view for ${extensionId} did not signal ready in ${LOAD_TIMEOUT_MS}ms`,
        );
      }
    }, LOAD_TIMEOUT_MS);
    return () => clearTimeout(timer);
  });

  import { getExtensionFrameOrigin } from '../../lib/ipc/extensionOrigin';

  // Keep exported function — accessible via bind:this in Svelte 5
  export function sendMessage(type: string, payload: any) {
    if (iframeElement && iframeElement.contentWindow) {
      iframeElement.contentWindow.postMessage(
        { type, payload },
        getExtensionFrameOrigin(extensionId),
      );
    }
  }
</script>

<div class="relative w-full h-full">
  {#key reloadNonce}
    <iframe
      bind:this={iframeElement}
      data-extension-id={extensionId}
      data-role="view"
      data-mount-token={mountToken != null ? String(mountToken) : undefined}
      src={iframeSrc}
      title="Extension Sandbox - {manifest?.name || extensionId}"
      class="w-full h-full border-none bg-transparent"
      sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
      onload={handleIframeLoad}
      onerror={handleIframeError}
    ></iframe>
  {/key}
  {#if loadFailed && !loaded}
    <div
      class="absolute inset-0 flex flex-col items-center justify-center gap-3 p-6 text-center"
      role="alert"
    >
      <p class="text-[var(--text-primary)] font-medium">
        {t('dialogs.extension_load_failed.title')}
      </p>
      <p class="text-[var(--text-secondary)] text-sm max-w-sm">
        {t('dialogs.extension_load_failed.body', { name: manifest?.name || extensionId })}
      </p>
      <Button onclick={reloadView} class="btn-primary">
        {t('dialogs.extension_load_failed.reload')}
      </Button>
    </div>
  {/if}
</div>
