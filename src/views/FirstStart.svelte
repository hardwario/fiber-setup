<script lang="ts">
  import downDark from "../assets/drawings/iso-fiber-down-button-dark-theme.svg";
  import down from "../assets/drawings/iso-fiber-down-button.svg";
  import CopyCommand from "../lib/components/CopyCommand.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Picture from "../lib/components/Picture.svelte";
  import Status from "../lib/components/Status.svelte";
  import Text from "../lib/components/Text.svelte";
  import { fill, t } from "../lib/i18n/index.svelte";
  import { wizard } from "../lib/wizard.svelte";

  const f = $derived(wizard.written ?? wizard.form);
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
  // The names from the first start: fiber-<serial> with the FIBER software.
  const names = $derived(wizard.names);
  const many = $derived(names.length > 1);
  const found = $derived(names.filter((n) => wizard.found[n]).length);
  const first = $derived(wizard.found[names[0]]);
  const hub = $derived(many && f.installFiber ? `${t().start.hubUnknown}.local` : names.map((n) => `${n}.local`).join(", "));
  const timeline = $derived((f.installFiber ? t().start.timeline : t().start.timelinePlain).map((s) => fill(s, { hub })));
  // What happens at each point of the timeline: it starts, installs, restarts.
  const GLYPHS = ["power", "download", "refresh"];
</script>

<!-- Whether the FIBERs are on the network yet, pinned to the top of the page. -->
{#if many}
  <Status tone={found === names.length ? "good" : "info"} busy={found < names.length}>
    {fill(t().start.foundMany, { found, total: names.length })}
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
        <li class:there={!!at}>
          <div class="who">
            <Icon name={at ? "circle-check" : "search"} size={15} />
            <span class="mono">{name}.local</span>
            <span class="where">{at ? at.ip : t().start.rowLooking}</span>
          </div>
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
    {f.installFiber ? t().start.sameNetwork : t().start.sameNetworkPlain}
  </p>
{/if}

<section class="card pad">
  <ol class="timeline">
    {#each timeline as item, i (i)}
      <li><span class="glyph"><Icon name={GLYPHS[i] ?? "clock"} size={16} /></span><span>{item}</span></li>
    {/each}
  </ol>
</section>

<!-- Only with the FIBER software: it is what drives the display and its buttons, and what the
     install log belongs to. A plain Raspberry Pi OS shows nothing there. -->
{#if f.installFiber}
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
