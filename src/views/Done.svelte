<script lang="ts">
  import CopyCommand from "../lib/components/CopyCommand.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import { t } from "../lib/i18n/index.svelte";
  import { wizard } from "../lib/wizard.svelte";

  const f = $derived(wizard.written ?? wizard.form);
  // The name it was written with: fiber-<serial> with the FIBER software.
  const name = $derived(wizard.found?.name ?? f.hostname);
</script>

<p class="lead">{t().done.lead}</p>

<section class="card pad">
  <h2><Icon name="terminal" size={16} /> {t().done.loginTitle}</h2>
  <p class="muted">{t().done.loginHint}</p>
  <CopyCommand text={`ssh ${f.username}@${name}.local`} />
  {#if wizard.found}
    <p class="muted">{t().done.loginIp}</p>
    <CopyCommand text={`ssh ${f.username}@${wizard.found.ip}`} />
  {/if}
</section>

<section class="card pad summary">
  <div class="row">
    <Icon name="monitor" size={15} /><span class="k">{t().done.name}</span><span class="mono">{name}.local</span>
  </div>
  <div class="row">
    <Icon name={f.network === "wifi" ? "wifi" : f.network === "phone" ? "phone" : "ethernet"} size={15} /><span class="k"
      >{t().done.network}</span
    ><span
      >{f.network === "wifi" ? `${t().settings.wifi}: ${f.ssid}` : f.network === "phone" ? t().settings.phone : t().settings.ethernet}{wizard.found
        ? ` · ${wizard.found.ip}`
        : ""}</span
    >
  </div>
  <div class="row">
    <Icon name="package" size={15} /><span class="k">{t().done.software}</span><span
      >{f.installFiber ? t().done.softwareFiber : t().done.softwarePlain}</span
    >
  </div>
</section>

<div class="actions">
  <button class="btn primary large" onclick={() => wizard.another()}><Icon name="refresh" size={15} /> {t().done.another}</button>
  <button class="btn large" onclick={() => wizard.api?.openLink("https://docs.hardwario.com/fiber/")}>
    <Icon name="book" size={15} />
    {t().done.docs}
    <Icon name="external" size={12} />
  </button>
</div>

<style>
  .summary {
    gap: 10px;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    overflow-wrap: anywhere;
    align-items: center;
    gap: 10px;
  }
  .row :global(svg) {
    color: var(--text-muted);
  }
  .k {
    flex: 0 0 110px;
    color: var(--text-muted);
  }
  .row > span:last-child {
    flex: 1 1 200px;
    min-width: 0;
  }
  .actions {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
  }
  p {
    margin: 0;
  }
</style>
