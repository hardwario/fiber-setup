<script lang="ts">
  import Icon from "../lib/components/Icon.svelte";
  import { fill, t } from "../lib/i18n/index.svelte";
  import type { Phase } from "../lib/types";
  import { wizard } from "../lib/wizard.svelte";

  const fl = $derived(wizard.flash);
  const image = $derived(wizard.image.result);
  const gb = (n: number) => `${(n / 1e9).toLocaleString(undefined, { maximumFractionDigits: 2 })} GB`;
  const time = (s: number) => `${Math.floor(s / 60)} min ${s % 60} s`;
  // With the FIBER software the name comes from the serial number, known once it is written.
  const name = $derived(
    fl.result?.hostname ?? (wizard.form.installFiber ? t().start.hubUnknown : wizard.form.hostname),
  );
  // The write as the stages it goes through, so that five minutes of it can be followed: each
  // is still to come, running (with its megabytes, when it counts them) or behind.
  const ORDER: Phase[] = ["boot", "wait_disk", "settings", "write", "verify", "done"];
  const STAGES: { phases: Phase[]; icon: string }[] = [
    { phases: ["boot", "wait_disk"], icon: "usb" },
    { phases: ["settings"], icon: "sliders" },
    { phases: ["write"], icon: "drive" },
    { phases: ["verify"], icon: "shield" },
  ];
  const at = $derived(fl.result ? ORDER.length - 1 : fl.phase ? ORDER.indexOf(fl.phase) : -1);
  function stageState(stage: { phases: Phase[] }): "todo" | "now" | "failed" | "done" {
    const first = ORDER.indexOf(stage.phases[0]);
    const last = ORDER.indexOf(stage.phases[stage.phases.length - 1]);
    if (at > last) return "done";
    if (at < first) return "todo";
    return fl.busy ? "now" : fl.error ? "failed" : "todo";
  }
  const pct = $derived(fl.total > 0 ? Math.min(100, Math.round((fl.done / fl.total) * 100)) : 0);
  const mb = (n: number) => Math.round(n / 1048576).toLocaleString();
  let logBox = $state<HTMLPreElement | null>(null);
  $effect(() => {
    void fl.log.length;
    if (logBox) logBox.scrollTop = logBox.scrollHeight;
  });
</script>


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

<section class="card pad summary">
  <h2><Icon name="list-checks" size={16} /> {t().write.what}</h2>
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

<section class="card pad" aria-live="polite">
  <h2><Icon name="drive" size={16} /> {t().write.plan}</h2>
  <ol class="stages">
    {#each STAGES as stage (stage.icon)}
      {@const st = stageState(stage)}
      {@const phase = stage.phases.find((ph) => ph === fl.phase) ?? stage.phases[0]}
      <li class={st}>
        <span class="glyph">
          {#if st === "done"}<Icon name="check" size={16} stroke={3} />{:else if st === "now"}<Icon name="loader" size={16} />{:else if st === "failed"}<Icon
              name="warning"
              size={16}
            />{:else}<Icon name={stage.icon} size={16} />{/if}
        </span>
        <div class="stage">
          <div class="line">
            <span class="label">{t().phase[phase]}</span>
            {#if st === "now" && fl.total > 0}<span class="nums tabular">{mb(fl.done)} / {mb(fl.total)} MB · {pct} %</span>{/if}
          </div>
          {#if st === "now" && fl.total > 0}
            <div class="bar"><div class="fill" style:width={`${pct}%`}></div></div>
          {/if}
        </div>
      </li>
    {/each}
  </ol>
</section>

{#if !fl.busy && !fl.result}
  <div class="note warning">
    <Icon name="warning" size={16} />
    <span>
      {t().welcome.erase}
      {#if wizard.info?.os === "macos"}<br /><span class="small">{t().write.macPassword}</span>{/if}
    </span>
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
    gap: 9px;
  }
  .summary h2 {
    margin-bottom: 3px;
  }
  .stages {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .stages li {
    display: flex;
    gap: 12px;
    align-items: center;
    color: var(--text-muted);
  }
  .glyph {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-muted);
  }
  .now {
    color: var(--text);
    font-weight: 500;
  }
  .now .glyph {
    background: var(--accent-weak);
    color: var(--accent);
  }
  li.done {
    color: var(--text-2);
  }
  .done .glyph {
    background: color-mix(in srgb, var(--good) 14%, transparent);
    color: var(--good);
  }
  .failed {
    color: var(--critical);
  }
  .failed .glyph {
    background: var(--critical-weak);
    color: var(--critical);
  }
  .stage {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 2px 12px;
  }
  .nums {
    font-size: 12.5px;
    font-weight: 400;
    color: var(--text-muted);
  }
  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.25s ease;
  }
  /* A value that needs two lines keeps its name and glyph on its first one. */
  .row {
    display: flex;
    flex-wrap: wrap;
    overflow-wrap: anywhere;
    align-items: flex-start;
    gap: 8px;
  }
  .row :global(svg) {
    margin-top: 3px;
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
