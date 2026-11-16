<script lang="ts">
  import Icon from "../lib/components/Icon.svelte";
  import Progress from "../lib/components/Progress.svelte";
  import { fill, t } from "../lib/i18n/index.svelte";
  import { wizard } from "../lib/wizard.svelte";

  const img = $derived(wizard.image);
  const info = $derived(wizard.info);
  const gb = (n: number) => (n / 1e9).toLocaleString(undefined, { maximumFractionDigits: 1 });
</script>

<!-- The two things that make a working FIBER, in the order they get onto it. -->
<section class="card pad part">
  <div class="glyph"><Icon name="package" size={22} /></div>
  <div class="what">
    <div class="top">
      <div class="names">
        <div class="label">{t().image.system}</div>
        {#if img.result}
          <div class="name">{img.result.name}</div>
        {:else if info}
          <div class="name">{info.image.name} ({info.image.version})</div>
        {/if}
      </div>
      <!-- While it is being got ready, the line below says how far it is. -->
      {#if img.result && !img.busy}
        <span class="pill good"><Icon name="circle-check" size={14} /> {t().image.ready}</span>
      {/if}
    </div>
    {#if img.result}
      <div class="muted small">
        {img.result.official ? t().image.official : t().image.custom}
        · {gb(img.result.size)} GB · <span class="mono">sha256 {img.result.source_sha256.slice(0, 12)}…</span>
      </div>
    {/if}
    {#if img.busy}
      <Progress phase={img.phase} done={img.done} total={img.total} busy={img.busy} />
    {/if}
    {#if img.error}
      <div class="note critical"><Icon name="warning" size={16} /><span>{fill(t().image.failed, { error: img.error })}</span></div>
    {/if}
    {#if img.otherCopy && !img.result}
      <div class="muted small">{t().image.reuse}</div>
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
      <span class="muted small">{t().image.cache}</span>
    </div>
  </div>
</section>

<section class="card pad part" class:off={!wizard.form.installFiber}>
  <div class="glyph"><Icon name="download" size={22} /></div>
  <div class="what">
    <div class="top">
      <div class="names">
        <div class="label">{t().image.later}</div>
        <div class="name">{t().image.fiber}</div>
      </div>
    </div>
    <div class="muted small">{wizard.form.installFiber ? t().image.fiberText : t().image.fiberOff}</div>
  </div>
</section>

<style>
  .card.pad.part {
    flex-direction: row;
    align-items: flex-start;
    gap: 16px;
  }
  .glyph {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: var(--radius-sm);
    background: var(--accent-weak);
    color: var(--accent);
    flex-shrink: 0;
  }
  .off .glyph {
    background: var(--surface-2);
    color: var(--text-muted);
  }
  .what {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }
  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: 6px 12px;
  }
  .names {
    display: flex;
    flex-direction: column;
    gap: 1px;
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
    align-items: center;
    gap: 8px 12px;
    flex-wrap: wrap;
    padding-top: 2px;
  }
</style>
