<script lang="ts">
  import Icon from "../lib/components/Icon.svelte";
  import Progress from "../lib/components/Progress.svelte";
  import { fill, t } from "../lib/i18n/index.svelte";
  import { wizard } from "../lib/wizard.svelte";

  const fl = $derived(wizard.flash);
  const image = $derived(wizard.image.result);
  const gb = (n: number) => `${(n / 1e9).toLocaleString(undefined, { maximumFractionDigits: 2 })} GB`;
  const time = (s: number) => `${Math.floor(s / 60)} min ${s % 60} s`;
  // With the FIBER software the name comes from the serial number, known once it is written.
  const name = $derived(
    fl.result?.hostname ?? (wizard.form.installFiber ? t().start.hubUnknown : wizard.form.hostname),
  );
  let logBox = $state<HTMLPreElement | null>(null);
  $effect(() => {
    void fl.log.length;
    if (logBox) logBox.scrollTop = logBox.scrollHeight;
  });
</script>

<p class="lead">{fill(t().write.lead, { image: image?.name ?? "" })}</p>

<section class="card pad summary">
  <div class="row"><Icon name="package" size={15} /><span class="k">{t().steps.image}</span><span>{image?.name}</span></div>
  <div class="row">
    <Icon name="monitor" size={15} /><span class="k">{t().settings.hostname}</span><span class="mono"
      >{name}.local</span
    >
  </div>
  <div class="row">
    <Icon name={wizard.form.network === "wifi" ? "wifi" : "ethernet"} size={15} /><span class="k"
      >{t().settings.network}</span
    ><span>{wizard.form.network === "wifi"
        ? `${t().settings.wifi}: ${wizard.form.ssid}`
        : wizard.form.network === "phone"
          ? t().settings.phone
          : t().settings.ethernet}</span>
  </div>
  <div class="row"><Icon name="usb" size={15} /><span class="k">USB</span><span class="mono">{wizard.port}</span></div>
</section>

{#if !fl.busy && !fl.result}
  <div class="note warning"><Icon name="warning" size={16} /><span>{t().welcome.erase}</span></div>
  {#if wizard.info?.os === "macos"}
    <p class="muted small">{t().write.macPassword}</p>
  {/if}
  <div>
    <button class="btn primary large" onclick={() => wizard.startFlash()}>
      <Icon name="drive" size={16} />
      {fl.error ? t().common.retry : t().write.start}
    </button>
  </div>
{/if}

{#if fl.busy || fl.result}
  <section class="card pad">
    <Progress phase={fl.phase} done={fl.done} total={fl.total} busy={fl.busy} />
    {#if fl.busy}
      <p class="muted small">{t().write.keep}</p>
      <div><button class="btn" onclick={() => wizard.cancel()}>{t().common.cancel}</button></div>
    {/if}
  </section>
{/if}

{#if fl.result}
  <div class="note good">
    <Icon name="circle-check" size={18} />
    <span>
      <strong>{t().write.doneTitle}.</strong>
      {fill(t().write.doneText, { size: gb(fl.result.written), time: time(fl.result.seconds) })}
      {#if fl.result.serial}<br /><span class="mono small">{fill(t().write.serial, { serial: fl.result.serial })}</span>{/if}
    </span>
  </div>
{/if}

{#if fl.error}
  <div class="note critical">
    <Icon name="warning" size={16} />
    <span>{fill(t().write.failed, { error: fl.error })}<br /><span class="small">{t().write.retryHint}</span></span>
  </div>
{/if}

{#if fl.log.length}
  <details class="card pad log">
    <summary><Icon name="terminal" size={14} /> {t().common.details}</summary>
    <pre bind:this={logBox}>{fl.log.join("\n")}</pre>
  </details>
{/if}

<style>
  .summary {
    gap: 8px;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    overflow-wrap: anywhere;
    align-items: center;
    gap: 8px;
  }
  .row :global(svg) {
    color: var(--text-muted);
  }
  .k {
    flex: 0 0 130px;
    color: var(--text-muted);
  }
  .row > span:last-child {
    flex: 1 1 200px;
    min-width: 0;
  }
  .small {
    font-size: 12.5px;
  }
  p {
    margin: 0;
  }
  .log summary {
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    color: var(--text-2);
    list-style: none;
  }
  .log summary::-webkit-details-marker {
    display: none;
  }
  pre {
    margin: 8px 0 0;
    max-height: 200px;
    overflow: auto;
    font-family: var(--mono);
    font-size: 12px;
    white-space: pre-wrap;
    -webkit-user-select: text;
    user-select: text;
  }
</style>
