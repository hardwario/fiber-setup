<script lang="ts">
  import fiber from "../assets/photos/fiber-front-display.png";
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
  const looking = $derived(
    [...new Set([f.hostname, wizard.hubName].filter((n): n is string => !!n))].map((n) => `${n}.local`).join(" / "),
  );
</script>

<div class="split">
  <div class="col">
    <p class="lead">{t().start.lead}</p>
    <section class="card pad">
      <ol class="timeline">
        {#each timeline as item, i (i)}
          <li><Icon name="clock" size={15} /><span>{item}</span></li>
        {/each}
      </ol>
    </section>
    <section class="card pad">
      {#if wizard.found}
        <div class="found">
          <Icon name="circle-check" size={18} />
          <span>{fill(t().start.found, { name: `${wizard.found.name}.local`, ip: wizard.found.ip })}</span>
        </div>
        {#if wizard.found.ssh}
          <code class="cmd">{fill(t().start.ssh, { user: f.username, name: wizard.found.name })}</code>
        {/if}
      {:else}
        <div class="waiting">
          <Icon name="loader" size={16} />
          <span>{fill(t().start.looking, { name: looking })}</span>
        </div>
        <p class="muted small">{fill(t().start.notFound, { time: elapsed })} {t().start.sameNetwork}</p>
      {/if}
    </section>
    <div class="note"><Icon name="monitor" size={16} /><span>{t().start.display}</span></div>
    <p class="muted small">{t().start.log}</p>
  </div>
  <div class="col">
    <img class="product" src={fiber} alt="HARDWARIO FIBER" />
  </div>
</div>

<style>
  .timeline {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .timeline li {
    display: flex;
    gap: 10px;
    align-items: flex-start;
  }
  .timeline :global(svg) {
    flex-shrink: 0;
    margin-top: 3px;
    color: var(--text-muted);
  }
  .found,
  .waiting {
    display: flex;
    align-items: center;
    gap: 10px;
    font-weight: 500;
  }
  .found :global(svg) {
    color: var(--good);
  }
  .cmd {
    display: block;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: 1px solid var(--border);
    -webkit-user-select: text;
    user-select: text;
  }
  .small {
    font-size: 12.5px;
  }
  p {
    margin: 0;
  }
  .product {
    width: 100%;
    max-width: 380px;
    margin: 0 auto;
    display: block;
  }
</style>
