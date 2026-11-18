<script lang="ts">
  import downDark from "../assets/drawings/iso-fiber-down-button-dark-theme.svg";
  import down from "../assets/drawings/iso-fiber-down-button.svg";
  import CopyCommand from "../lib/components/CopyCommand.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Picture from "../lib/components/Picture.svelte";
  import Status from "../lib/components/Status.svelte";
  import Text from "../lib/components/Text.svelte";
  import { fill, t } from "../lib/i18n/index.svelte";
  import type { Stage } from "../lib/platform";
  import { wizard } from "../lib/wizard.svelte";

  const f = $derived(wizard.written ?? wizard.form);
  /** Whether the written FIBERs install the FIBER platform. */
  const fiber = $derived(f.components.length > 0);
  let now = $state(Date.now());

  // Look for <name>.local every 5 s while this step is open; a search can take a few seconds.
  $effect(() => {
    let stop = false;
    const loop = async () => {
      while (!stop) {
        await wizard.search();
        for (let i = 0; i < 5 && !stop; i++) {
          await new Promise((r) => setTimeout(r, 1000));
          now = Date.now();
        }
      }
    };
    loop();
    return () => (stop = true);
  });

  const elapsed = $derived.by(() => {
    const s = Math.max(0, Math.round((now - wizard.searchSince) / 1000));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  });
  // The names from the first start: fiber-<serial> with the FIBER platform.
  const names = $derived(wizard.names);
  const many = $derived(names.length > 1);
  const found = $derived(names.filter((n) => wizard.found[n]).length);
  const first = $derived(wizard.found[names[0]]);
  const hub = $derived(many && fiber ? `${t().start.hubUnknown}.local` : names.map((n) => `${n}.local`).join(", "));
  const timeline = $derived((fiber ? t().start.timeline : t().start.timelinePlain).map((s) => fill(s, { hub })));

  /** A component's name in the user's language; `common` is the platform's own first step. */
  function title(id: string): string {
    if (id === "common") return t().start.stage.common;
    const own = (t().components as Record<string, { title: string } | undefined>)[id]?.title;
    return own ?? wizard.info?.platform?.components.find((c) => c.id === id)?.title ?? id;
  }
  /** How far a FIBER is, in words; none before its first start could be read. */
  function said(stage: Stage | undefined): string | null {
    if (!stage) return null;
    const words = t().start.stage;
    switch (stage.kind) {
      case "download":
      case "restart":
      case "network":
      case "clock":
        return fill(words[stage.kind], { release: stage.release });
      case "install":
        return stage.component
          ? fill(words.installing, { component: title(stage.component), step: stage.step, steps: stage.steps })
          : fill(words.install, { release: stage.release });
      case "failed":
        return fill(words.failed, {
          error: (t().start.errors as Record<string, string | undefined>)[stage.error] ?? stage.error,
        });
      default:
        return words[stage.kind];
    }
  }
  /** The tone of a FIBER's line: done, waiting, or something to look at. */
  function tone(stage: Stage | undefined): "info" | "good" | "warning" {
    if (stage?.kind === "ready") return "good";
    if (stage?.kind === "failed" || stage?.kind === "partly" || stage?.kind === "applyFailed") return "warning";
    return "info";
  }
  const ended = (stage: Stage | undefined) => !!stage && ["ready", "partly", "applyFailed"].includes(stage.kind);
  /** With the FIBER platform a FIBER is through when its installation has ended; without it, when
   * it is on the network. */
  const through = $derived(names.filter((n) => wizard.found[n] && (!fiber || ended(wizard.stages[n]))).length);
  const firstStage = $derived(wizard.stages[names[0]]);
  // What happens at each point of the timeline: it starts, installs, restarts.
  const GLYPHS = ["power", "download", "refresh"];
</script>

<!-- Whether the FIBERs are on the network yet, pinned to the top of the page. -->
{#if many}
  <Status tone={through === names.length ? "good" : "info"} busy={through < names.length}>
    {fill(t().start.foundMany, { found, total: names.length })}
  </Status>
{:else if first && fiber}
  <Status tone={tone(firstStage)} busy={!ended(firstStage) && firstStage?.kind !== "failed"}>
    {fill(t().start.found, { name: `${names[0]}.local`, ip: first.ip })}
    {said(firstStage ?? { kind: "starting" })}
  </Status>
{:else}
  <Status tone={first ? "good" : "info"} busy={!first}>
    {first
      ? fill(t().start.found, { name: `${names[0]}.local`, ip: first.ip })
      : fill(t().start.looking, { name: `${names[0]}.local` })}
  </Status>
{/if}

{#if many}
  <section class="card pad">
    <ul class="devices">
      {#each names as name (name)}
        {@const at = wizard.found[name]}
        {@const stage = wizard.stages[name]}
        <li class:there={!!at}>
          <div class="who">
            <Icon name={at ? (fiber && !ended(stage) ? "loader" : "circle-check") : "search"} size={15} />
            <span class="mono">{name}.local</span>
            <span class="where">{at ? at.ip : t().start.rowLooking}</span>
          </div>
          {#if at && fiber}<div class="stage {tone(stage)}">{said(stage ?? { kind: "starting" })}</div>{/if}
          {#if at?.ssh}<CopyCommand text={`ssh ${f.username}@${name}.local`} />{/if}
        </li>
      {/each}
    </ul>
  </section>
{:else if first?.ssh}
  <section class="card pad">
    <p class="muted">{t().start.ssh}</p>
    <CopyCommand text={`ssh ${f.username}@${names[0]}.local`} />
  </section>
{/if}
{#if found < names.length}
  <p class="muted small">
    {fill(t().start.notFound, { time: elapsed })}
    {fiber ? t().start.sameNetwork : t().start.sameNetworkPlain}
  </p>
{/if}

<section class="card pad">
  <ol class="timeline">
    {#each timeline as item, i (i)}
      <li><span class="glyph"><Icon name={GLYPHS[i] ?? "clock"} size={16} /></span><span>{item}</span></li>
    {/each}
  </ol>
</section>

<!-- Only with the FIBER platform: it is what drives the display and its buttons, and what the
     install log belongs to. A plain Raspberry Pi OS shows nothing there. -->
{#if fiber}
  <section class="card pad display">
    <div class="about">
      <h2><Icon name="monitor" size={16} /> {t().start.displayTitle}</h2>
      <p><Text value={t().start.display} /></p>
    </div>
    <div class="product"><Picture src={down} dark={downDark} alt={t().start.displayPicture} /></div>
  </section>

  <p class="muted small">{t().start.log}</p>
{/if}

<style>
  .timeline {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .timeline li {
    display: flex;
    gap: 12px;
    align-items: center;
  }
  .glyph {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-2);
  }
  .small {
    font-size: 12.5px;
  }
  /* Several FIBERs: each with where it is, and how to log in once it answers. */
  .devices {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .devices li {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .who {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-muted);
  }
  .who .mono {
    color: var(--text);
    font-weight: 500;
  }
  .where {
    margin-left: auto;
    font-size: 12.5px;
  }
  .there .who > :global(svg) {
    color: var(--good);
  }
  /* How far it is, under its name. */
  .stage {
    padding-left: 23px;
    font-size: 12.5px;
    color: var(--text-muted);
  }
  .stage.warning {
    color: var(--warning);
  }
  p {
    margin: 0;
  }
  /* The text and, beside it, what it talks about. */
  .card.pad.display {
    flex-direction: row;
    align-items: center;
    gap: 20px;
  }
  .about {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }
  .product {
    width: 300px;
    flex-shrink: 0;
  }
  @container (max-width: 520px) {
    .card.pad.display {
      flex-direction: column;
      align-items: flex-start;
    }
    .product {
      width: 100%;
    }
  }
</style>
