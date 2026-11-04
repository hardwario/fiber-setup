<script lang="ts">
  import Icon from "../lib/components/Icon.svelte";
  import SearchSelect from "../lib/components/SearchSelect.svelte";
  import { countryChoices } from "../lib/countries";
  import { timezoneChoices } from "../lib/timezones";
  import { fill, i18n, t } from "../lib/i18n/index.svelte";
  import { wizard, type Network } from "../lib/wizard.svelte";

  const f = $derived(wizard.form);
  const info = $derived(wizard.info);
  const NETWORKS: { value: Network; icon: string }[] = [
    { value: "wifi", icon: "wifi" },
    { value: "ethernet", icon: "ethernet" },
    { value: "phone", icon: "phone" },
  ];

  /** The message for a field, once the user has left it (or pressed Next). */
  function err(field: string): string | null {
    const key = wizard.errors[field];
    if (!key || !wizard.touched[field]) return null;
    return t().settings.err[key as keyof ReturnType<typeof t>["settings"]["err"]];
  }
  const touch = (field: string) => () => (wizard.touched[field] = true);
  const networkError = $derived(wizard.errors.network ? t().settings.err.phone : null);
  const countries = $derived(countryChoices(i18n.lang));
  // Empty = no time zone in the settings: the FIBER keeps Raspberry Pi OS's default (UTC).
  const timezones = $derived([
    { value: "", label: t().settings.timezoneNone },
    ...timezoneChoices(i18n.lang, info?.defaults.timezone),
  ]);
</script>

<p class="lead">{t().settings.lead}</p>

<section class="card pad">
  <h2><Icon name="monitor" size={16} /> {t().settings.device}</h2>
  <div class="grid">
    {#if f.installFiber}
      <div class="field">
        <span>{t().settings.hostname}</span>
        <div class="input wide mono fixed">{t().start.hubUnknown}</div>
        <span class="hint">{t().settings.hostnameFiber}</span>
      </div>
    {:else}
      <label class="field">
        <span>{t().settings.hostname}</span>
        <input class="input wide mono" bind:value={f.hostname} onblur={touch("hostname")} spellcheck="false" autocomplete="off" />
        {#if err("hostname")}<span class="error">{err("hostname")}</span>{:else}<span class="hint"
            >{fill(t().settings.hostnameHint, { name: f.hostname || "fiber" })}</span
          >{/if}
      </label>
    {/if}
    <label class="field">
      <span>{t().settings.user}</span>
      <input class="input wide mono" bind:value={f.username} onblur={touch("username")} spellcheck="false" />
      {#if err("username")}<span class="error">{err("username")}</span>{/if}
    </label>
    <label class="field">
      <span>{t().settings.password}</span>
      <input class="input wide" type="password" bind:value={f.password} onblur={touch("password")} autocomplete="new-password" />
      {#if err("password")}<span class="error">{err("password")}</span>{:else}<span class="hint"
          >{fill(t().settings.passwordHint, { user: f.username })}</span
        >{/if}
    </label>
    <label class="field">
      <span>{t().settings.passwordAgain}</span>
      <input class="input wide" type="password" bind:value={f.password2} onblur={touch("password2")} autocomplete="new-password" />
      {#if err("password2")}<span class="error">{err("password2")}</span>{/if}
    </label>
  </div>
  {#if info?.defaults.ssh_key}
    <label class="check">
      <input type="checkbox" bind:checked={f.useSshKey} />
      <span>
        {t().settings.sshKey}
        <span class="hint block mono">{fill(t().settings.sshKeyFrom, { path: info.defaults.ssh_key_path ?? "" })}</span>
      </span>
    </label>
  {:else}
    <p class="muted small"><Icon name="key" size={12} /> {t().settings.noSshKey}</p>
  {/if}
</section>

<section class="card pad">
  <h2><Icon name="globe" size={16} /> {t().settings.network}</h2>
  <div class="segments" role="radiogroup" aria-label={t().settings.network}>
    {#each NETWORKS as n (n.value)}
      <button
        type="button"
        role="radio"
        aria-checked={f.network === n.value}
        class:active={f.network === n.value}
        onclick={() => (f.network = n.value)}
      >
        <Icon name={n.icon} size={16} />
        <span>{t().settings[n.value]}</span>
      </button>
    {/each}
  </div>

  {#if f.network === "wifi"}
    <div class="grid">
      <label class="field">
        <span>{t().settings.ssid}</span>
        <input class="input wide" bind:value={f.ssid} onblur={touch("ssid")} spellcheck="false" autocomplete="off" />
        {#if err("ssid")}<span class="error">{err("ssid")}</span>{/if}
      </label>
      <label class="field">
        <span>{t().settings.wifiPassword}</span>
        <input class="input wide" type="password" bind:value={f.wifiPassword} onblur={touch("wifiPassword")} autocomplete="off" />
        {#if err("wifiPassword")}<span class="error">{err("wifiPassword")}</span>{/if}
      </label>

      <label class="check">
        <input type="checkbox" bind:checked={f.hidden} />
        <span>{t().settings.hidden}</span>
      </label>
    </div>
  {:else if f.network === "ethernet"}
    <div class="note"><Icon name="ethernet" size={16} /><span>{t().settings.ethernetHint}</span></div>
  {:else}
    <div class="note"><Icon name="phone" size={16} /><span>{t().settings.phoneHint}</span></div>
  {/if}
  <div class="grid">
    {#if f.network !== "ethernet"}
      <div class="field">
        <label for="country">{t().settings.country}</label>
        <SearchSelect id="country" choices={countries} bind:value={f.country} placeholder={t().settings.typeToSearch} />
        <span class="hint">{t().settings.countryHint}</span>
      </div>
    {/if}
    <div class="field">
      <label for="timezone">{t().settings.timezone}</label>
      <SearchSelect id="timezone" choices={timezones} bind:value={f.timezone} placeholder={t().settings.typeToSearch} />
      {#if err("timezone")}<span class="error">{err("timezone")}</span>{/if}
    </div>
  </div>
</section>

<section class="card pad">
  <h2><Icon name="package" size={16} /> {t().settings.software}</h2>
  <label class="check">
    <input type="checkbox" bind:checked={f.installFiber} />
    <span>
      {t().settings.installFiber}
      <span class="hint block">{t().settings.installFiberHint}</span>
    </span>
  </label>
  {#if networkError}<div class="note critical"><Icon name="warning" size={16} /><span>{networkError}</span></div>{/if}
</section>

<style>
  .fixed {
    display: flex;
    align-items: center;
    color: var(--text-2);
    background: var(--surface-2);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(230px, 100%), 1fr));
    gap: 14px 18px;
    align-items: start;
  }
  .segments {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(150px, 100%), 1fr));
    gap: 4px;
    padding: 3px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: 1px solid var(--border);
  }
  .segments button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 34px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text-2);
    font-weight: 500;
    cursor: pointer;
  }
  .segments button:hover:not(:disabled) {
    color: var(--text);
  }
  .segments button.active {
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
  .segments button.active :global(svg) {
    color: var(--accent);
  }
  .check {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    cursor: pointer;
  }
  .grid > .check {
    align-self: center;
  }
  .check input {
    margin-top: 3px;
    accent-color: var(--accent);
  }
  .hint {
    font-size: 12px;
    color: var(--text-muted);
  }
  .block {
    display: block;
  }
  .small {
    font-size: 12.5px;
  }
  p {
    margin: 0;
  }
</style>
