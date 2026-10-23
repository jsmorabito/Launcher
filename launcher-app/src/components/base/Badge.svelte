<script lang="ts">
  let {
    text,
    variant = 'default',
    mono = false,
    bordered = false,
  }: {
    text: string;
    variant?: 'default' | 'success' | 'warning' | 'danger' | 'info';
    mono?: boolean;
    bordered?: boolean;
  } = $props();

  const variantClass = $derived(`badge-${variant}`);
</script>

<span class="badge {variantClass}" class:mono class:bordered>
  {text}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-0-5) var(--space-2);
    font-size: var(--font-size-2xs);
    font-weight: 600;
    line-height: normal;
    border-radius: var(--radius-xs);
    white-space: nowrap;
    user-select: none;
    text-transform: uppercase;
    letter-spacing: 0.02em;
  }

  .badge-default {
    background-color: var(--color-bg-subtle);
    color: var(--color-text-muted);
  }

  .badge-success {
    background-color: color-mix(in srgb, var(--color-success) 12%, transparent);
    color: var(--color-success);
  }

  .badge-warning {
    background-color: color-mix(in srgb, var(--color-warning) 12%, transparent);
    color: var(--color-warning);
  }

  .badge-danger {
    background-color: color-mix(in srgb, var(--color-danger) 12%, transparent);
    color: var(--color-danger);
  }

  .badge-info {
    background-color: var(--color-accent-faint);
    color: var(--color-accent);
  }

  .mono {
    font-family: var(--font-mono);
  }

  .bordered {
    border: 1px solid var(--color-border);
  }
</style>
