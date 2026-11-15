<script lang="ts">
  import fiber from "../assets/photos/fiber-front-display.png";
  import CopyCommand from "../lib/components/CopyCommand.svelte";
  import Icon from "../lib/components/Icon.svelte";
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
  // One name from the first start: fiber-<serial> with the FIBER software.
  const hub = $derived(`${f.hostname}.local`);
  const timeline = $derived((f.installFiber ? t().start.timeline : t().start.timelinePlain).map((s) => fill(s, { hub })));
  // What happens at each point of the timeline: it starts, installs, restarts.
  const GLYPHS = ["power", "download", "refresh"];
  const looking = $derived(
    [...new Set([f.hostname, wizard.hubName].filter((n): n is string => !!n))].map((n) => `${n}.local`).join(" / "),
  );
</script>

<p class="lead">{t().start.lead}</p>

<section class="card pad">
  <ol class="timeline">
    {#each timeline as item, i (i)}
      <li><span class="glyph"><Icon name={GLYPHS[i] ?? "clock"} size={16} /></span><span>{item}</span></li>
    {/each}
  </ol>
</section>

<section class="card pad" class:good={!!wizard.found}>
  {#if wizard.found}
    <div class="state found">
      <span class="glyph"><Icon name="circle-check" size={20} /></span>
      <span>{fill(t().start.found, { name: `${wizard.found.name}.local`, ip: wizard.found.ip })}</span>
    </div>
    {#if wizard.found.ssh}
      <p class="muted">{t().start.ssh}</p>
      <CopyCommand text={`ssh ${f.username}@${wizard.found.name}.local`} />
    {/if}
  {:else}
    <div class="state">
      <span class="glyph"><Icon name="search" size={18} /></span>
      <span>{fill(t().start.looking, { name: looking })}</span>
      <Icon name="loader" size={16} />
    </div>
    <p class="muted small">{fill(t().start.notFound, { time: elapsed })} {t().start.sameNetwork}</p>
  {/if}
</section>

<section class="card pad">
  <h2><Icon name="monitor" size={16} /> {t().start.displayTitle}</h2>
  <p>{t().start.display}</p>
  <img class="product" src={fiber} alt="HARDWARIO FIBER" />
</section>

<p class="muted small">{t().start.log}</p>

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
  .card.good {
    border-color: color-mix(in srgb, var(--good) 45%, transparent);
  }
  .state {
    display: flex;
    align-items: center;
    gap: 12px;
    font-weight: 500;
  }
  .state > :global(svg) {
    color: var(--text-muted);
  }
  .state .glyph {
    width: 36px;
    height: 36px;
  }
  .found .glyph {
    background: color-mix(in srgb, var(--good) 14%, transparent);
    color: var(--good);
  }
  .small {
    font-size: 12.5px;
  }
  p {
    margin: 0;
  }
  /* The picture is square with the FIBER in its middle: only that middle band is shown. */
  .product {
    display: block;
    width: 100%;
    max-width: 300px;
    aspect-ratio: 10 / 7;
    object-fit: cover;
  }
</style>
