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
  import { PHASES, STEPS, STEP_ICONS, wizard } from "./lib/wizard.svelte";
  import Connect from "./views/Connect.svelte";
  import Done from "./views/Done.svelte";
  import FirstStart from "./views/FirstStart.svelte";
  import Image from "./views/Image.svelte";
  import Jumper from "./views/Jumper.svelte";
  import Restart from "./views/Restart.svelte";
  import Settings from "./views/Settings.svelte";
  import Welcome from "./views/Welcome.svelte";
  import Write from "./views/Write.svelte";

  /** The sidebar folded to its glyphs; remembered, as in tower-bridge. */
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
  /** One or two sentences under the title: what this step is for. */
  const leads = $derived({
    welcome: t().welcome.lead,
    image: t().image.lead,
    settings: t().settings.lead,
    jumper: t().jumper.lead,
    connect: t().connect.lead,
    write: t().write.lead,
    restart: t().restart.lead,
    start: t().start.lead,
    done: t().done.lead,
  });
  const last = $derived(wizard.index === STEPS.length - 1);
  /** On the Write step the bar's button writes; once written it is Next again. */
  const toWrite = $derived(wizard.step === "write" && !wizard.flash.result);
  /** Next to the bar's button: what the step waits for, so a greyed Next explains itself. */
  const hint = $derived.by((): { text: string; busy: boolean } | null => {
    switch (wizard.step) {
      case "image":
        return wizard.image.busy ? { text: t().image.preparing, busy: true } : null;
      case "settings":
        return Object.keys(wizard.errors).some((k) => wizard.touched[k]) ? { text: t().settings.fix, busy: false } : null;
      case "connect":
        if (!wizard.port) return { text: t().connect.waiting, busy: true };
        if (wizard.access && !wizard.access.rules_installed) {
          return { text: wizard.info?.os === "windows" ? t().connect.needDriver : t().connect.needAccess, busy: false };
        }
        return null;
      case "write":
        return wizard.flash.busy ? { text: t().write.keep, busy: false } : null;
      default:
        return null;
    }
  });
  /** A step that is behind the user; all of them once the last one is reached. */
  const completed = (i: number) => i < wizard.reached || wizard.reached === STEPS.length - 1;
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
        <img class="logo" src={prefs.effectiveTheme === "dark" ? logoReversed : logo} alt="HARDWARIO FIBER" />
      {/if}
    </div>
    <!-- The steps in their three parts (Prepare, Write, Start), each step by its glyph: nine steps
         read as three. The open one is filled, the ones behind carry a check. -->
    <nav class="phases" aria-label={t().nav.steps}>
      {#each PHASES as phase (phase.id)}
        {@const behind = phase.steps.filter((s) => completed(STEPS.indexOf(s))).length}
        <div class="phase">
          {#if !collapsed}
            <div class="phase-head" class:complete={behind === phase.steps.length}>
              <span>{t().nav.phases[phase.id]}</span>
              <span class="count">
                {#if behind === phase.steps.length}
                  <Icon name="check" size={11} stroke={3} />
                {:else}
                  {fill(t().nav.phaseCount, { done: behind, total: phase.steps.length })}
                {/if}
              </span>
            </div>
          {/if}
          {#each phase.steps as step (step)}
            {@const i = STEPS.indexOf(step)}
            {@const reachable = i <= wizard.reached && !busy}
            {@const done = completed(i) && wizard.step !== step}
            <button
              class="item"
              class:active={wizard.step === step}
              class:done
              aria-current={wizard.step === step ? "step" : undefined}
              disabled={!reachable}
              title={collapsed ? t().steps[step] : undefined}
              aria-label={collapsed ? t().steps[step] : undefined}
              onclick={() => wizard.go(step)}
            >
              <Icon name={STEP_ICONS[step]} size={15} />
              {#if !collapsed}
                <span class="label">{t().steps[step]}</span>
                {#if done}<span class="mark"><Icon name="check" size={13} stroke={3} /></span>{/if}
              {/if}
            </button>
          {/each}
        </div>
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
        <header class="page-head" class:finished={last}>
          <div class="glyph"><Icon name={STEP_ICONS[wizard.step]} size={24} /></div>
          <div class="titles">
            <div class="step-of">{fill(t().common.stepOf, { n: wizard.index + 1, total: STEPS.length })}</div>
            <h1>{titles[wizard.step]}</h1>
            <p class="lead">{leads[wizard.step]}</p>
          </div>
        </header>
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
            <div class="forward">
              {#if hint}
                <span class="hint">{#if hint.busy}<Icon name="loader" size={14} />{/if}<span>{hint.text}</span></span>
              {/if}
              {#if busy}
                <button class="btn large" onclick={() => wizard.cancel()}>{t().common.cancel}</button>
              {/if}
              {#if toWrite}
                <!-- The step's one action, where the eye already is for Next. -->
                <button class="btn primary large" disabled={busy} onclick={() => wizard.startFlash()}>
                  {#if busy}
                    <Icon name="loader" size={15} />
                    {t().write.writing}
                  {:else}
                    <Icon name="drive" size={15} />
                    {wizard.flash.error ? t().common.retry : t().write.start}
                  {/if}
                </button>
              {:else}
                <button
                  class="btn primary large"
                  disabled={busy || (!wizard.canNext && wizard.step !== "settings")}
                  onclick={() => wizard.next()}
                >
                  {wizard.step === "start" ? t().start.finish : t().common.next}
                  <Icon name="arrow-right" size={15} />
                </button>
              {/if}
            </div>
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
  /* The menu half as large again as tower-bridge's: the window's page zoom (src-tauri) gives
     1.25, this the rest, inside the 250 px / 72 px columns above. */
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
    padding: 4px 8px 8px;
  }
  .collapsed .brand {
    justify-content: center;
    padding: 4px 0;
  }
  .brand img {
    display: block;
    flex-shrink: 0;
  }
  .brand .logo {
    width: 150px;
    max-width: 100%;
    height: auto;
  }
  .phases {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding-top: 6px;
  }
  .phase {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .phase-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 8px 3px;
    color: var(--text-muted);
    font-size: 10px;
    font-weight: 650;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .phase-head .count {
    display: grid;
    letter-spacing: 0;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    text-transform: none;
  }
  .phase-head.complete {
    color: var(--good-text);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 30px;
    padding: 0 8px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--text-2);
    font-size: 13px;
    font-weight: 500;
    text-align: left;
    cursor: pointer;
  }
  .item:hover:not(:disabled):not(.active) {
    background: var(--hover);
    color: var(--text);
  }
  .item:disabled {
    color: var(--text-muted);
    cursor: default;
  }
  .item.active {
    background: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  .item .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item .mark {
    display: grid;
    margin-left: auto;
    color: var(--good);
  }
  /* Folded: the glyphs alone, a line between the parts; a step behind the user is green. */
  .collapsed .phases {
    gap: 0;
  }
  .collapsed .phase {
    align-items: center;
    padding: 6px 0;
  }
  .collapsed .phase + .phase {
    border-top: 1px solid var(--border);
  }
  .collapsed .item {
    justify-content: center;
    width: 32px;
    padding: 0;
  }
  .collapsed .item.done {
    color: var(--good-text);
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
  /* One column, read from the top down: the page's head, then its parts one under the other. */
  .view {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: 100%;
    max-width: 740px;
    margin: 0 auto;
    padding: 28px 28px 32px;
  }
  .page-head {
    display: flex;
    align-items: flex-start;
    gap: 16px;
    padding-bottom: 4px;
  }
  .titles {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .page-head .glyph {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    flex-shrink: 0;
    border-radius: 14px;
    background: var(--accent-weak);
    color: var(--accent);
  }
  .page-head.finished .glyph {
    background: color-mix(in srgb, var(--good) 14%, transparent);
    color: var(--good);
  }
  .page-head h1 {
    font-size: 23px;
    line-height: 1.25;
    letter-spacing: -0.015em;
    text-wrap: balance;
  }
  .step-of {
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--accent);
    margin-bottom: 1px;
  }
  .page-head .lead {
    margin-top: 6px;
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
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    max-width: 740px;
    margin: 0 auto;
    padding: 12px 28px;
  }
  .forward {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    min-width: 0;
  }
  .forward .hint {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    margin-right: 4px;
    color: var(--text-muted);
    font-size: 13px;
  }
  .forward .hint span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    line-height: 1.5;
    color: var(--text-2);
    max-width: 62ch;
  }
  main :global(.card.pad) {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 18px 20px;
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
</style>
