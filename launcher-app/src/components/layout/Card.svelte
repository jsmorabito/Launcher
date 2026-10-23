<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title = '',
    children,
  }: {
    title?: string;
    children?: Snippet;
  } = $props();
</script>

<div
  class="p-6 rounded-[var(--radius-lg)] mb-6 border border-[var(--color-border)] flex-shrink-0"
  style="background-color: var(--color-bg-subtle);"
>
  {#if title}
    <h2 class="text-xl font-semibold mb-4 text-[var(--color-text)]">{title}</h2>
  {/if}
  {@render children?.()}
</div>
