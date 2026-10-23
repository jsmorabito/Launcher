<script lang="ts">
  let {
    options,
    value = $bindable(''),
    onchange,
    onfocus,
    onblur,
  }: {
    options: { value: string; label: string }[];
    value: string;
    /** For callers that persist the choice rather than binding to local
     *  state. Fires only on a real change, so a re-click is not a save. */
    onchange?: (value: string) => void;
    onfocus?: () => void;
    onblur?: () => void;
  } = $props();

  function select(next: string) {
    if (next === value) return;
    value = next;
    onchange?.(next);
  }
</script>

<div class="segmented-control" role="radiogroup">
  {#each options as option}
    <button
      type="button"
      role="radio"
      aria-checked={value === option.value}
      class="segment"
      class:active={value === option.value}
      onclick={() => select(option.value)}
      {onfocus}
      {onblur}
    >
      {option.label}
    </button>
  {/each}
</div>

<style>
  .segmented-control {
    display: flex;
    gap: var(--space-1);
    padding: var(--space-1);
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-border);
    background: var(--color-bg-subtle-opaque);
  }

  .segment {
    flex: 1;
    padding: var(--space-1-5) var(--space-4);
    border: none;
    border-radius: calc(var(--radius-sm) - 2px);
    font-family: var(--font-ui);
    font-size: var(--font-size-sm);
    font-weight: 500;
    color: var(--color-text-faint);
    background: transparent;
    cursor: pointer;
    white-space: nowrap;
    outline: none;
    transition: color var(--transition-fast);
  }

  .segment:hover:not(.active) {
    color: var(--color-text-muted);
  }

  .segment.active {
    background: var(--color-surface);
    color: var(--color-text);
    box-shadow: var(--shadow-xs);
  }

  /* The global *:focus-visible ring is a bare `box-shadow`, so .segment.active
     out-specifies it and the selected segment would show no focus at all.
     Restate the ring here, after .active, and keep the lift alongside it. */
  .segment:focus-visible,
  .segment.active:focus-visible {
    box-shadow: var(--shadow-focus);
  }
</style>
