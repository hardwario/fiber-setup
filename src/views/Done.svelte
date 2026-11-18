<script lang="ts">
  import CopyCommand from "../lib/components/CopyCommand.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import { fill, t } from "../lib/i18n/index.svelte";
  import { links } from "../lib/platform";
  import { wizard } from "../lib/wizard.svelte";

  const f = $derived(wizard.written ?? wizard.form);
  const offer = $derived(wizard.info?.platform ?? null);
  // The names they were written with: fiber-<serial> with the FIBER platform.
  const names = $derived(wizard.names);
  /** The chosen components by their names, in the user's language. */
  const chosen = $derived(
    (offer?.components ?? [])
      .filter((c) => f.components.includes(c.id))
      .map((c) => (t().components as Record<string, { title: string } | undefined>)[c.id]?.title ?? c.title)
      .join(", "),
  );
  /** Where the chosen components are reached: by name, a FIBER's own page each. */
  const pages = $derived(names.map((name) => ({ name, links: links(f.components, f.mqttLan, `${name}.local`) })));
  // One FIBER: also by its address, when it was found.
  const found = $derived(names.length === 1 ? wizard.found[names[0]] : undefined);
  // The password it asks for: hidden until the user wants to see it, whatever its length.
  let shown = $state(false);
</script>

<section class="card pad">
  <h2><Icon name="terminal" size={16} /> {t().done.loginTitle}</h2>
  <p class="muted">{t().done.loginHint}</p>
  {#each names as name (name)}
    <CopyCommand text={`ssh ${f.username}@${name}.local`} />
  {/each}
  {#if found}
    <p class="muted">{t().done.loginIp}</p>
    <CopyCommand text={`ssh ${f.username}@${found.ip}`} />
  {/if}
  <!-- What it asks for then: the login from the settings. -->
  <div class="login">
    <div class="row">
      <Icon name="user" size={15} /><span class="k">{t().settings.user}</span><span class="mono">{f.username}</span>
    </div>
    <div class="row">
      <Icon name="lock" size={15} /><span class="k">{t().settings.password}</span>
      <span class="secret">
        <span class="mono">{shown ? f.password : "••••••••"}</span>
        <button
          class="eye"
          type="button"
          aria-pressed={shown}
          aria-label={shown ? t().common.hidePassword : t().common.showPassword}
          title={shown ? t().common.hidePassword : t().common.showPassword}
          onclick={() => (shown = !shown)}
        >
          <Icon name={shown ? "eye-off" : "eye"} size={16} />
        </button>
      </span>
    </div>
  </div>
</section>

{#if pages.some((p) => p.links.length > 0)}
  <section class="card pad">
    <h2><Icon name="globe" size={16} /> {t().done.servicesTitle}</h2>
    <p class="muted">{t().done.servicesHint}</p>
    {#each pages as page (page.name)}
      {#if pages.length > 1}<div class="mono fiber">{page.name}.local</div>{/if}
      <ul class="links">
        {#each page.links as l (l.href)}
          <li>
            <span class="k">{t().done.links[l.label]}</span>
            {#if l.href.startsWith("http")}
              <button class="link mono" type="button" onclick={() => wizard.api?.openLink(l.href)}
                >{l.href} <Icon name="external" size={12} /></button
              >
            {:else}
              <span class="mono">{l.href}</span>
            {/if}
            {#if l.user === "admin"}<span class="muted small">{t().done.admin}</span>{/if}
          </li>
        {/each}
      </ul>
    {/each}
  </section>
{/if}

<section class="card pad summary">
  <div class="row">
    <Icon name="monitor" size={15} /><span class="k">{t().done.name}</span><span class="mono"
      >{names.map((n) => `${n}.local`).join(", ")}</span
    >
  </div>
  <div class="row">
    <Icon name={f.network === "wifi" ? "wifi" : f.network === "phone" ? "phone" : "ethernet"} size={15} /><span class="k"
      >{t().done.network}</span
    ><span
      >{f.network === "wifi" ? fill(t().settings.wifiSummary, { ssid: f.ssid }) : f.network === "phone" ? t().settings.phone : t().settings.noneSummary}{found
        ? ` · ${found.ip}`
        : ""}</span
    >
  </div>
  <div class="row">
    <Icon name="package" size={15} /><span class="k">{t().done.software}</span><span
      >{f.components.length > 0
        ? fill(t().done.softwareFiber, { version: offer?.release.version ?? "", components: chosen })
        : t().done.softwarePlain}</span
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
  /* A value that needs two lines keeps its name and glyph on its first one. */
  .row {
    display: flex;
    flex-wrap: wrap;
    overflow-wrap: anywhere;
    align-items: flex-start;
    gap: 10px;
  }
  .row :global(svg) {
    margin-top: 3px;
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
  .login {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-top: 4px;
  }
  .login .k {
    flex-basis: 150px;
  }
  .secret {
    display: flex;
    align-items: center;
    gap: 8px;
    -webkit-user-select: text;
    user-select: text;
  }
  .eye {
    display: grid;
    place-items: center;
    width: 28px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    cursor: pointer;
  }
  .eye:hover {
    background: var(--hover);
    color: var(--text);
  }
  .eye :global(svg) {
    margin-top: 0;
  }
  .actions {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
  }
  /* A component's page: its name, its address, and ChirpStack's own user name. */
  .links {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .links li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 10px;
    overflow-wrap: anywhere;
  }
  .links .k {
    flex: 0 0 110px;
    color: var(--text-muted);
  }
  .fiber {
    font-weight: 500;
    margin-top: 4px;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
    text-align: left;
  }
  .link:hover {
    text-decoration: underline;
  }
  .small {
    font-size: 12.5px;
  }
  p {
    margin: 0;
  }
</style>
