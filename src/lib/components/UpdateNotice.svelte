<script lang="ts">
  /** A newer version of FIBER Setup is out: says so and leads to its downloads. Closed, it stays
   * closed for that version (the step list still names it). */
  import { fill, t } from "../i18n/index.svelte";
  import { wizard } from "../wizard.svelte";
  import Icon from "./Icon.svelte";

  const update = $derived(wizard.updateNotice);
</script>

{#if update}
  <div class="update" role="status">
    <span class="icon"><Icon name="download" size={18} /></span>
    <div class="content">
      <div class="title">{fill(t().update.title, { version: update.version })}</div>
      <div class="body">{t().update.body}</div>
    </div>
    <div class="actions">
      <button class="btn primary" type="button" onclick={() => wizard.openRelease()}>
        {t().update.download}
        <Icon name="external" size={13} />
      </button>
      <button
        class="btn ghost close"
        type="button"
        aria-label={t().common.close}
        title={t().common.close}
        onclick={() => wizard.dismissUpdate()}
      >
        <Icon name="close" size={15} />
      </button>
    </div>
  </div>
{/if}

<style>
  .update {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border-radius: var(--radius);
    background: var(--accent-weak);
  }
  .icon {
    display: flex;
    color: var(--accent);
  }
  .content {
    flex: 1;
    min-width: 0;
  }
  .title {
    font-weight: 600;
  }
  .body {
    margin-top: 1px;
    color: var(--text-2);
    font-size: 13px;
  }
  .actions {
    display: flex;
    gap: 4px;
    flex: none;
  }
  .close {
    width: 30px;
    padding: 0;
    justify-content: center;
  }
</style>
