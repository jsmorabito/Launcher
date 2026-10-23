<script lang="ts">
  import { t } from '../../services/i18n';
  const detailViewPromise = import('./DetailView.svelte');
</script>

{#await detailViewPromise}
  <div class="flex items-center justify-center p-8 text-[var(--color-text-muted)]">
    <div class="animate-pulse text-sm">{t('common.loading')}</div>
  </div>
{:then module}
  {@const DetailView = module.default}
  <DetailView />
{:catch}
  <div class="p-4 text-center text-[var(--color-danger)] text-sm font-mono">
    {t('features.store.ui.lazy_failed')}
  </div>
{/await}
