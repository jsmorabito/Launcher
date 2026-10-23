<script lang="ts">
  import { t } from '../../services/i18n';
  import { describePermission } from '../../services/extension/permissionCatalog';
  import { describeSecretRefs } from '../../services/extension/secretRefDisplay';

  let { permissions, permissionArgs = {} } = $props<{
    permissions: string[];
    permissionArgs?: Record<string, unknown>;
  }>();

  function argsFor(permission: string): { chips: string[] | null; json: string | null } {
    const value = permissionArgs?.[permission];
    if (permission === 'network:secrets') return { chips: null, json: null };
    if (value === undefined || value === null) return { chips: null, json: null };
    if (Array.isArray(value) && value.every((item) => typeof item === 'string')) {
      return { chips: value as string[], json: null };
    }
    return { chips: null, json: JSON.stringify(value, null, 2) };
  }
</script>

<div class="permission-list">
  {#each permissions as permission (permission)}
    {@const info = describePermission(permission)}
    {@const args = argsFor(permission)}
    <div class="permission-item">
      <div class="permission-item-head">
        <span class="permission-title">{info.title}</span>
        <code class="permission-scope text-mono">{permission}</code>
      </div>
      <p class="permission-desc">
        {info.description}
        {#if !info.known}
          <span class="permission-caution">{t('settings.extensions.review_carefully')}</span>
        {/if}
      </p>
      {#if permission === 'network:secrets'}
        {#each describeSecretRefs(permissionArgs?.[permission]) as line}
          <p class="permission-desc text-caption">{line}</p>
        {/each}
      {:else if args.chips}
        <div class="permission-args">
          {#each args.chips as chip (chip)}
            <code class="permission-arg-chip font-mono text-xs">{chip}</code>
          {/each}
        </div>
      {:else if args.json}
        <pre class="permission-arg-json font-mono text-xs overflow-x-auto">{args.json}</pre>
      {/if}
    </div>
  {/each}
</div>

<style>
  .permission-list {
    display: flex;
    flex-direction: column;
    /* design-ok: 1px structural gap creates hairline dividers via background bleed-through, not a scaled spacing value */
    gap: 1px;
    background: var(--color-border);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .permission-item {
    background: var(--color-bg-subtle-opaque);
    padding: var(--space-4) var(--space-5);
  }

  .permission-item-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .permission-title {
    font-size: var(--font-size-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .permission-scope {
    font-size: var(--font-size-xs);
    color: var(--color-text-muted);
  }

  .permission-desc {
    margin: var(--space-1) 0 0;
    font-size: var(--font-size-xs);
    color: var(--color-text-muted);
    line-height: 1.45;
  }

  .permission-args {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin-top: var(--space-2);
  }

  .permission-arg-chip,
  .permission-arg-json {
    background: var(--color-bg-subtle);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-xs);
    padding: var(--space-0-5) var(--space-2);
    color: var(--color-text);
    word-break: break-all;
  }

  .permission-arg-json {
    padding: var(--space-2) var(--space-3);
    white-space: pre;
  }

  .permission-caution {
    color: var(--color-danger);
  }
</style>
