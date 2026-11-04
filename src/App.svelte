<script lang="ts">
  import { onMount } from "svelte";
  import symbol from "./assets/hardwario-symbol.svg";
  import symbolReversed from "./assets/hardwario-symbol-reversed.svg";
  // The official stacked FIBER logo (HARDWARIO over FIBER) from hardwario/artwork, as recorded in
  // src/assets/artwork.lock.json.
  import logo from "./assets/brand/hardwario-fiber-logo-stacked.svg";
  import logoReversed from "./assets/brand/hardwario-fiber-logo-stacked-reversed.svg";
  import Icon from "./lib/components/Icon.svelte";
  import PrefsSwitcher from "./lib/components/PrefsSwitcher.svelte";
  import { fill, t } from "./lib/i18n/index.svelte";
  import { prefs } from "./lib/prefs.svelte";
  import { STEPS, wizard } from "./lib/wizard.svelte";
  import Connect from "./views/Connect.svelte";
  import Done from "./views/Done.svelte";
  import FirstStart from "./views/FirstStart.svelte";
  import Image from "./views/Image.svelte";
  import Jumper from "./views/Jumper.svelte";
  import Restart from "./views/Restart.svelte";
  import Settings from "./views/Settings.svelte";
  import Welcome from "./views/Welcome.svelte";
  import Write from "./views/Write.svelte";

  /** The sidebar folded to its numbers; remembered, as in tower-bridge. */
  const COLLAPSED_KEY = "fiber-setup.sidebar.collapsed";
  let collapsed = $state(readCollapsed());

  function readCollapsed(): boolean {
    try {
      return localStorage.getItem(COLLAPSED_KEY) === "1";
    } catch {
      return false;
    }
  }

  function toggleSidebar() {
    collapsed = !collapsed;
    try {
      localStorage.setItem(COLLAPSED_KEY, collapsed ? "1" : "0");
    } catch {
      // Private mode: the sidebar opens expanded next time.
    }
  }

  onMount(() => {
    prefs.apply();
    wizard.init();
  });

  const titles = $derived({
    welcome: t().welcome.title,
    image: t().image.title,
    settings: t().settings.title,
    jumper: t().jumper.title,
    connect: t().connect.title,
    write: t().write.title,
    restart: t().restart.title,
    start: t().start.title,
    done: t().done.title,
  });
  const last = $derived(wizard.index === STEPS.length - 1);
  const busy = $derived(wizard.flash.busy);
  let main = $state<HTMLElement | null>(null);
  $effect(() => {
    void wizard.step;
    main?.scrollTo({ top: 0 });
  });

  function link(url: string) {
    wizard.api?.openLink(url);
  }
</script>

<div class="app" class:collapsed>
  <aside class="sidebar">
    <div class="brand">
      {#if collapsed}
        <img src={prefs.effectiveTheme === "dark" ? symbolReversed : symbol} alt="HARDWARIO FIBER" width="22" height="28" />
      {:else}
        <div class="wordmark">
          <img class="logo" src={prefs.effectiveTheme === "dark" ? logoReversed : logo} alt="HARDWARIO FIBER" />
          <span class="setup">Setup</span>
        </div>
      {/if}
    </div>
    <!-- The steps as a flow (Marek's sketch, 2026-11-04): a line down the middle, each step a box
         with its number in a circle centred on the box's top edge. -->
    <nav class="flow">
      {#each STEPS as step, i (step)}
        {@const reachable = i <= wizard.reached && !busy}
        <button
          class:active={wizard.step === step}
          class:done={i < wizard.reached && wizard.step !== step}
          aria-current={wizard.step === step ? "step" : undefined}
          disabled={!reachable}
          title={collapsed ? t().steps[step] : undefined}
          aria-label={collapsed ? t().steps[step] : undefined}
          onclick={() => wizard.go(step)}
        >
          <span class="num">{i + 1}</span>
          {#if !collapsed}<span class="box">{t().steps[step]}</span>{/if}
        </button>
      {/each}
    </nav>
    {#if !collapsed}
      <div class="bottom">
        <PrefsSwitcher />
      </div>
      {#if wizard.simulated}
        <div class="sim"><Icon name="info" size={12} /> {t().nav.simulated}</div>
      {/if}
      <div class="about">
        <div class="links">
          <a
            href="https://docs.hardwario.com/fiber/"
            onclick={(e) => {
              e.preventDefault();
              link("https://docs.hardwario.com/fiber/");
            }}>{t().nav.documentation} <Icon name="external" size={11} /></a
          >
          <a
            href="https://www.hardwario.store/"
            onclick={(e) => {
              e.preventDefault();
              link("https://www.hardwario.store/");
            }}>{t().nav.store} <Icon name="external" size={11} /></a
          >
        </div>
      </div>
    {/if}
    <div class="foot">
      <button
        class="fold"
        type="button"
        title={collapsed ? t().nav.expand : undefined}
        aria-label={collapsed ? t().nav.expand : undefined}
        aria-expanded={!collapsed}
        onclick={toggleSidebar}
      >
        <Icon name={collapsed ? "chevron-right" : "chevron-left"} size={14} />
        {#if !collapsed}<span>{t().nav.collapse}</span>{/if}
      </button>
      {#if !collapsed && wizard.info}<span class="version">{fill(t().nav.version, { version: wizard.info.version })}</span>{/if}
    </div>
  </aside>

  <main bind:this={main}>
    {#if wizard.failed}
      <div class="center muted">{wizard.failed}</div>
    {:else if !wizard.info}
      <div class="center muted">{t().common.loading}</div>
    {:else}
      <div class="view">
        <div class="page-head">
          <div>
            <div class="step-of">{fill(t().common.stepOf, { n: wizard.index + 1, total: STEPS.length })}</div>
            <h1>{titles[wizard.step]}</h1>
          </div>
        </div>
        {#if wizard.step === "welcome"}
          <Welcome />
        {:else if wizard.step === "image"}
          <Image />
        {:else if wizard.step === "settings"}
          <Settings />
        {:else if wizard.step === "jumper"}
          <Jumper />
        {:else if wizard.step === "connect"}
          <Connect />
        {:else if wizard.step === "write"}
          <Write />
        {:else if wizard.step === "restart"}
          <Restart />
        {:else if wizard.step === "start"}
          <FirstStart />
        {:else}
          <Done />
        {/if}
      </div>
      {#if !last}
        <footer>
          <div class="footer-inner">
            <button class="btn large" disabled={wizard.index === 0 || busy} onclick={() => wizard.back()}>
              <Icon name="arrow-left" size={15} />
              {t().common.back}
            </button>
            <button
              class="btn primary large"
              disabled={busy || (!wizard.canNext && wizard.step !== "settings")}
              onclick={() => wizard.next()}
            >
              {wizard.step === "start" ? t().start.finish : t().common.next}
              <Icon name="arrow-right" size={15} />
            </button>
          </div>
        </footer>
      {/if}
    {/if}
  </main>
</div>

<style>
  .app {
    display: grid;
    grid-template-columns: 250px 1fr;
    height: 100%;
    grid-template-rows: minmax(0, 1fr);
  }
  .app.collapsed {
    grid-template-columns: 72px 1fr;
  }
  /* The menu half as large again as tower-bridge's (Marek, 2026-11-04): the window's page zoom
     (src-tauri) gives 1.25, this the rest, inside the 250 px / 72 px columns above. */
  .sidebar {
    zoom: 1.2;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    overflow-y: auto;
    padding: 16px 12px 12px;
    background: var(--sidebar);
    border-right: 1px solid var(--border);
  }
  .collapsed .sidebar {
    padding: 18px 10px 14px;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 8px;
    margin: -2px 0;
  }
  .collapsed .brand {
    justify-content: center;
    padding: 4px 0;
  }
  .brand img {
    display: block;
    flex-shrink: 0;
  }
  .wordmark {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
  }
  .wordmark .logo {
    width: 150px;
    max-width: 100%;
    height: auto;
  }
  .setup {
    padding-left: 1px;
    font-size: 15px;
    font-weight: 600;
    line-height: 1;
    color: var(--text-muted);
  }
  .flow {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-top: 6px;
  }
  /* The line the steps hang on; boxes and circles are opaque, so it shows only between them. */
  .flow::before {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 50%;
    width: 2px;
    margin-left: -1px;
    background: var(--border-strong);
  }
  .flow button {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--text-2);
    font-weight: 500;
    cursor: pointer;
  }
  .num {
    position: relative;
    z-index: 1;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    /* Half of it over the box: centred on the box's top edge. */
    margin-bottom: -11px;
    border-radius: 50%;
    border: 1.5px solid var(--border-strong);
    background: var(--sidebar);
    color: var(--text-muted);
    font-size: 11.5px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }
  .box {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    min-height: 28px;
    padding: 11px 8px 3px;
    border: 1.5px solid var(--border-strong);
    border-radius: 10px;
    background: var(--sidebar);
    font-size: 13px;
    line-height: 1.2;
    text-align: center;
  }
  .flow button:hover:not(:disabled):not(.active) .box {
    border-color: var(--text-muted);
    color: var(--text);
  }
  .flow button:disabled {
    cursor: default;
  }
  .flow button:disabled .box {
    border-color: var(--border);
    color: var(--text-muted);
  }
  .flow button:disabled .num {
    border-color: var(--border);
  }
  .flow button.active .num {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .flow button.active .box {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--sidebar));
    color: var(--text);
    font-weight: 600;
  }
  .flow button.done .num {
    background: var(--good);
    border-color: var(--good);
    color: #fff;
  }
  .collapsed .flow {
    gap: 10px;
  }
  .collapsed .num {
    margin-bottom: 0;
  }
  .bottom {
    margin-top: auto;
    padding: 0 2px;
  }
  .sim {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    background: var(--warning-weak);
    color: var(--warning-text);
    font-size: 11.5px;
  }
  .about {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 10px 0;
    border-top: 1px solid var(--border);
    font-size: 12px;
  }
  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
  }
  .links a {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--text-2);
    text-decoration: none;
  }
  .links a:hover {
    color: var(--accent);
    text-decoration: underline;
  }
  .version {
    padding-right: 6px;
    color: var(--text-muted);
    font-size: 11.5px;
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-shrink: 0;
  }
  .fold {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    height: 28px;
    padding: 0 8px;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-muted);
    font-size: 12.5px;
    cursor: pointer;
  }
  .fold:hover {
    background: var(--hover);
    color: var(--text);
  }
  .collapsed .foot {
    justify-content: center;
    margin-top: auto;
  }
  .collapsed .fold {
    justify-content: center;
    padding: 0;
  }
  main {
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    min-width: 0;
  }
  .view {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: 100%;
    max-width: 1100px;
    margin: 0 auto;
    padding: 22px 26px 24px;
  }
  .page-head h1 {
    font-size: 22px;
    letter-spacing: -0.01em;
  }
  .step-of {
    font-size: 12px;
    font-weight: 500;
    color: var(--accent);
    margin-bottom: 2px;
  }
  footer {
    position: sticky;
    bottom: 0;
    background: color-mix(in srgb, var(--bg) 92%, transparent);
    backdrop-filter: blur(6px);
    border-top: 1px solid var(--border);
  }
  .footer-inner {
    display: flex;
    justify-content: space-between;
    max-width: 1100px;
    margin: 0 auto;
    padding: 12px 26px;
  }
  .center {
    display: grid;
    place-items: center;
    height: 100%;
  }
  /* Shared by the step views. */
  main :global(.lead) {
    margin: 0;
    font-size: 15px;
    color: var(--text-2);
    max-width: 70ch;
  }
  main :global(.card.pad) {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px 18px;
  }
  main :global(.card h2) {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14.5px;
  }
  main :global(.card h2 svg) {
    color: var(--accent);
  }
  main :global(.split) {
    display: grid;
    grid-template-columns: minmax(0, 1.15fr) minmax(0, 1fr);
    gap: 20px;
    align-items: start;
  }
  /* Landscape photos, one under the other. */
  main :global(.photos) {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 14px;
    min-width: 0;
  }
  main :global(.col) {
    display: flex;
    flex-direction: column;
    gap: 14px;
    min-width: 0;
  }
  main :global(.note) {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--text-2);
    font-size: 13.5px;
  }
  main :global(.note svg) {
    flex-shrink: 0;
    margin-top: 1px;
  }
  main :global(.note.warning) {
    background: var(--warning-weak);
    border-color: transparent;
    color: var(--warning-text);
  }
  main :global(.note.critical) {
    background: var(--critical-weak);
    border-color: transparent;
    color: var(--critical);
  }
  main :global(.note.good) {
    background: color-mix(in srgb, var(--good) 12%, transparent);
    border-color: transparent;
    color: var(--good-text);
  }
  main :global(.pill) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border-radius: 13px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    font-size: 12.5px;
    font-weight: 500;
    white-space: nowrap;
  }
  main :global(.pill.good) {
    background: color-mix(in srgb, var(--good) 12%, transparent);
    border-color: transparent;
    color: var(--good-text);
  }
  @media (max-width: 980px) {
    main :global(.split) {
      grid-template-columns: 1fr;
    }
  }
</style>
