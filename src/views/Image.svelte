<script lang="ts">
  import Icon from "../lib/components/Icon.svelte";
  import Progress from "../lib/components/Progress.svelte";
  import { fill, t } from "../lib/i18n/index.svelte";
  import { wizard } from "../lib/wizard.svelte";

  const img = $derived(wizard.image);
  const info = $derived(wizard.info);
  const gb = (n: number) => (n / 1e9).toLocaleString(undefined, { maximumFractionDigits: 1 });
</script>

<p class="lead">{t().image.lead}</p>

<section class="card pad">
  <div class="os">
    <div class="icon"><Icon name="package" size={22} /></div>
    <div class="what">
      <div class="label">{t().image.system}</div>
      {#if img.result}
        <div class="name">{img.result.name}</div>
        <div class="muted small">
          {img.result.official ? t().image.official : t().image.custom}
          · {gb(img.result.size)} GB
        </div>
        <div class="mono small muted">sha256 {img.result.source_sha256.slice(0, 16)}…</div>
      {:else if info}
        <div class="name">{info.image.name}</div>
        <div class="muted small">{info.image.version}</div>
      {/if}
    </div>
    <div class="state">
      {#if img.result && !img.busy}
        <span class="pill good"><Icon name="circle-check" size={14} /> {t().image.ready}</span>
      {:else if img.busy}
        <span class="pill"><Icon name="loader" size={14} /> {t().image.preparing}</span>
      {/if}
    </div>
  </div>

  {#if img.busy}
    <Progress phase={img.phase} done={img.done} total={img.total} busy={img.busy} />
  {/if}
  {#if img.error}
    <div class="note critical"><Icon name="warning" size={16} /><span>{fill(t().image.failed, { error: img.error })}</span></div>
  {/if}

  <div class="actions">
    {#if img.error && !img.busy}
      <button class="btn primary" onclick={() => wizard.prepareImage(null)}><Icon name="refresh" size={14} /> {t().common.retry}</button>
    {/if}
    {#if img.busy}
      <button class="btn" onclick={() => wizard.cancel()}>{t().common.cancel}</button>
    {:else}
      <button class="btn" onclick={() => wizard.pickImage()}><Icon name="file" size={14} /> {t().image.useFile}</button>
      {#if img.result && !img.result.official}
        <button class="btn" onclick={() => wizard.prepareImage(null)}>{t().image.useOfficial}</button>
      {/if}
    {/if}
  </div>
</section>

{#if img.otherCopy && !img.result}
  <div class="note"><Icon name="info" size={16} /><span>{t().image.reuse}</span></div>
{/if}
<p class="muted small">{t().image.cache}</p>
<div class="note"><Icon name="info" size={16} /><span>{t().image.next}</span></div>

<style>
  .os {
    display: flex;
    flex-wrap: wrap;
    gap: 10px 14px;
    align-items: flex-start;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: var(--radius-sm);
    background: var(--accent-weak);
    color: var(--accent);
    flex-shrink: 0;
  }
  .what {
    flex: 1 1 260px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .label {
    font-size: 12px;
    color: var(--text-muted);
  }
  .name {
    font-size: 16px;
    font-weight: 600;
  }
  .small {
    font-size: 12.5px;
  }
  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  p {
    margin: 0;
  }
</style>
