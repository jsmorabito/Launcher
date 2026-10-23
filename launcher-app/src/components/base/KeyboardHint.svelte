<script lang="ts">
  let {
    keys,
    action,
  }: {
    keys: string | string[];
    action?: string;
  } = $props();

  const keyList = $derived(Array.isArray(keys) ? keys : [keys]);
</script>

<div class="keyboard-hint">
  <div class="key-group">
    {#each keyList as key}
      <kbd class="text-mono">
        <!-- ↵ has a top-heavy bounding box; nudge down to optically center it. -->
        {#if key === '↵'}
          <span class="glyph-return">{key}</span>
        {:else}
          {key}
        {/if}
      </kbd>
    {/each}
  </div>
  {#if action}
    <span class="action text-caption">{action}</span>
  {/if}
</div>

<style>
  .keyboard-hint {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--font-size-xs);
    color: var(--color-text-muted);
    user-select: none;
  }

  .key-group {
    display: flex;
    align-items: center;
    gap: var(--space-0-5);
  }

  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 24px;
    height: 21px;
    padding: 0 var(--space-2);
    background-color: var(--color-accent-subtle);
    color: var(--color-text-muted);
    border-radius: var(--radius-sm);
    border: none;
    /* Soft inset rim. */
    box-shadow: inset 0 0 2px 0.5px var(--kbd-rim);
    font-weight: 500;
  }

  .action {
    color: var(--color-text-faint);
    font-weight: 400;
  }

  .glyph-return {
    display: inline-block;
    transform: translateY(1px);
    line-height: 1;
  }
</style>
