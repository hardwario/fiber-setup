<script lang="ts">
  import Icon from "../lib/components/Icon.svelte";
  import Status from "../lib/components/Status.svelte";
  import Text from "../lib/components/Text.svelte";
  import { fill, t } from "../lib/i18n/index.svelte";
  import type { FlashReport, Phase } from "../lib/types";
  import { wizard, type Job } from "../lib/wizard.svelte";

  // A FIBER connected while this step is open joins the list; one taken away leaves it.
  $effect(() => wizard.watchUsb(1500));

  const image = $derived(wizard.image.result);
  const gb = (n: number) => `${(n / 1e9).toLocaleString(undefined, { maximumFractionDigits: 2 })} GB`;
  const time = (s: number) => `${Math.floor(s / 60)} min ${s % 60} s`;
  const mb = (n: number) => Math.round(n / 1048576).toLocaleString();
  const percent = (j: Job<FlashReport>) => (j.total > 0 ? Math.min(100, Math.round((j.done / j.total) * 100)) : 0);

  const IDLE: Job<FlashReport> = { busy: false, phase: null, since: 0, done: 0, total: 0, result: null, error: null, log: [] };

  // A board that is started over USB answers within seconds. One that does not is waiting in
  // the middle of an earlier start, and only RESET brings it back: say so, where the eye is.
  const SLOW = 10_000;
  let now = $state(Date.now());
  $effect(() => {
    if (!wizard.writing) return;
    const tick = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(tick);
  });
  const stuck = (j: Job<FlashReport>) => j.busy && (j.phase === null || j.phase === "boot") && now - j.since > SLOW;
  /** The FIBERs of this step, by their USB ports: written, being written, and to be written. */
  const ports = $derived(wizard.ports);
  const many = $derived(ports.length > 1);
  const job = (port: string): Job<FlashReport> => wizard.jobs[port] ?? IDLE;
  /** One FIBER: its write, shown stage by stage. */
  const fl = $derived(job(ports[0] ?? ""));
  const written = $derived(wizard.done.length);
  const failed = $derived(ports.filter((port) => !job(port).busy && job(port).error).length);

  // With the FIBER software a name comes from the serial number, known once it is written;
  // without it the FIBERs are numbered on from the name in the settings.
  const names = $derived(
    ports.map((port, i) => job(port).result?.hostname ?? (wizard.form.installFiber ? null : wizard.nameAt(i))),
  );
  // In the summary: one FIBER's name, or the names of several; with the FIBER software the
  // pattern they all follow (each line below shows its own once it is written).
  const name = $derived(
    many && wizard.form.installFiber
      ? `${t().start.hubUnknown}.local`
      : names.map((n) => `${n ?? t().start.hubUnknown}.local`).join(", "),
  );

  // The write as the stages it goes through, so that five minutes of it can be followed: each
  // is still to come, running (with its megabytes, when it counts them) or behind.
  const ORDER: Phase[] = ["boot", "wait_disk", "ready", "settings", "write", "verify", "done"];
  const STAGES: { phases: Phase[]; icon: string }[] = [
    { phases: ["boot", "wait_disk", "ready"], icon: "usb" },
    { phases: ["settings"], icon: "sliders" },
    { phases: ["write"], icon: "drive" },
    { phases: ["verify"], icon: "shield" },
  ];
  const at = $derived(fl.result ? ORDER.length - 1 : fl.phase ? ORDER.indexOf(fl.phase) : -1);
  type State = "todo" | "now" | "failed" | "done";
  function stageState(stage: { phases: Phase[] }): State {
    const first = ORDER.indexOf(stage.phases[0]);
    const last = ORDER.indexOf(stage.phases[stage.phases.length - 1]);
    if (at > last) return "done";
    if (at < first) return "todo";
    return fl.busy ? "now" : fl.error ? "failed" : "todo";
  }
  /** Several FIBERs: where each one is. */
  const deviceState = (j: Job<FlashReport>): State => (j.result ? "done" : j.busy ? "now" : j.error ? "failed" : "todo");

  const log = $derived(ports.flatMap((port) => job(port).log.map((line) => (many ? `[${port}] ${line}` : line))));
  let logBox = $state<HTMLPreElement | null>(null);
  $effect(() => {
    void log.length;
    if (logBox) logBox.scrollTop = logBox.scrollHeight;
  });
</script>

<!-- How the write ended, pinned to the top of the page. -->
{#if many && !wizard.writing && written + failed > 0}
  <Status tone={failed === 0 ? "good" : written === 0 ? "critical" : "warning"}>
    {fill(t().write.doneMany, { done: written, total: written + failed })}
    {#if failed > 0}<br /><span class="small"><Text value={fill(t().write.failedMany, { failed })} /></span>{/if}
  </Status>
{:else if !many && fl.result}
  <Status tone="good">
    <strong>{t().write.doneTitle}.</strong>
    {fill(t().write.doneText, { size: gb(fl.result.written), time: time(fl.result.seconds) })}
    {#if fl.result.serial}<br /><span class="mono small">{fill(t().write.serial, { serial: fl.result.serial })}</span>{/if}
  </Status>
{:else if !many && fl.error}
  <Status tone="critical">
    {fill(t().write.failed, { error: fl.error })}<br /><span class="small"><Text value={t().write.retryHint} /></span>
  </Status>
{/if}

<section class="card pad summary">
  <h2><Icon name="list-checks" size={16} /> {t().write.what}</h2>
  <div class="row"><Icon name="package" size={15} /><span class="k">{t().steps.image}</span><span>{image?.name}</span></div>
  <div class="row">
    <Icon name="monitor" size={15} /><span class="k">{t().settings.hostname}</span><span class="mono">{name}</span>
  </div>
  <div class="row">
    <Icon name={wizard.form.network === "wifi" ? "wifi" : "ethernet"} size={15} /><span class="k"
      >{t().settings.network}</span
    ><span>{wizard.form.network === "wifi"
        ? fill(t().settings.wifiSummary, { ssid: wizard.form.ssid })
        : wizard.form.network === "phone"
          ? t().settings.phone
          : t().settings.noneSummary}</span>
  </div>
  <div class="row"><Icon name="usb" size={15} /><span class="k">USB</span><span class="mono">{ports.join(", ")}</span></div>
</section>

<!-- Several FIBERs to write: all at once, or one after another. -->
{#if wizard.toWrite.length > 1 && !wizard.writing}
  <section class="card pad">
    <h2><Icon name="sliders" size={16} /> {t().write.mode}</h2>
    <div class="choices" role="radiogroup" aria-label={t().write.mode}>
      {#each [true, false] as together (together)}
        <label class="choice" class:chosen={wizard.together === together}>
          <input type="radio" name="order" value={together} bind:group={wizard.together} />
          <span class="text">
            <span class="title">{together ? t().write.together : t().write.oneByOne}</span>
            <span class="desc">{together ? t().write.togetherDesc : t().write.oneByOneDesc}</span>
          </span>
        </label>
      {/each}
    </div>
  </section>
{/if}

<section class="card pad" aria-live="polite">
  <h2><Icon name="drive" size={16} /> {t().write.plan}</h2>
  {#if many}
    <ol class="stages">
      {#each ports as port, i (port)}
        {@const j = job(port)}
        {@const st = deviceState(j)}
        <li class={st}>
          <span class="glyph">
            {#if st === "done"}<Icon name="check" size={16} stroke={3} />{:else if st === "now"}<Icon name="loader" size={16} />{:else if st === "failed"}<Icon
                name="warning"
                size={16}
              />{:else}<Icon name="usb" size={16} />{/if}
          </span>
          <div class="stage">
            <div class="line">
              <span class="label"
                >{fill(t().connect.choose, { port })}{#if names[i]}<span class="who mono">{names[i]}.local</span>{/if}</span
              >
              {#if st === "now" && j.total > 0}<span class="nums tabular">{mb(j.done)} / {mb(j.total)} MB · {percent(j)} %</span>{/if}
            </div>
            {#if st === "done" && j.result}
              <span class="what">{fill(t().write.deviceDone, { time: time(j.result.seconds) })}</span>
            {:else if st === "failed"}
              <span class="what">{j.error}</span>
            {:else if st === "now"}
              <span class="what">{t().phase[j.phase ?? "boot"]}</span>
              {#if stuck(j)}<span class="what press"><Text value={t().write.resetHint} /></span>{/if}
            {/if}
            {#if st === "now" && j.total > 0}
              <div class="bar"><div class="fill" style:width={`${percent(j)}%`}></div></div>
            {/if}
          </div>
        </li>
      {/each}
    </ol>
  {:else}
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
              {#if st === "now" && fl.total > 0}<span class="nums tabular">{mb(fl.done)} / {mb(fl.total)} MB · {percent(fl)} %</span>{/if}
            </div>
            {#if st === "now" && stuck(fl)}<span class="what press"><Text value={t().write.resetHint} /></span>{/if}
            {#if st === "now" && fl.total > 0}
              <div class="bar"><div class="fill" style:width={`${percent(fl)}%`}></div></div>
            {/if}
          </div>
        </li>
      {/each}
    </ol>
  {/if}
</section>

{#if !wizard.writing && wizard.toWrite.length > 0}
  <div class="note warning">
    <Icon name="warning" size={16} />
    <span>
      {t().welcome.erase}
      {#if wizard.info?.os === "macos"}<br /><span class="small">{t().write.macPassword}</span>{/if}
    </span>
  </div>
{/if}

{#if log.length}
  <details class="card pad log">
    <summary><Icon name="terminal" size={14} /> {t().common.details}</summary>
    <pre bind:this={logBox}>{log.join("\n")}</pre>
  </details>
{/if}

<style>
  /* The choice between writing all at once and one after another. */
  .choices {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border: 1.5px solid var(--border-strong);
    border-radius: 10px;
    cursor: pointer;
  }
  .choice.chosen {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 4%, var(--surface));
  }
  .choice input {
    width: 16px;
    height: 16px;
    margin: 0;
    flex-shrink: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .title {
    font-weight: 600;
  }
  .desc {
    font-size: 12.5px;
    color: var(--text-muted);
  }
  /* One FIBER of several: its name beside its port, and what it is doing under them. */
  .who {
    margin-left: 10px;
    font-weight: 400;
    color: var(--text-muted);
  }
  .what {
    font-size: 12.5px;
    font-weight: 400;
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }
  .failed .what {
    color: var(--critical);
  }
  /* What to do when a start takes long: to be seen, not only read. */
  .what.press {
    color: var(--warning-text);
    font-weight: 500;
  }
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
