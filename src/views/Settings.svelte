<script lang="ts">
  import Icon from "../lib/components/Icon.svelte";
  import PasswordField from "../lib/components/PasswordField.svelte";
  import SearchSelect from "../lib/components/SearchSelect.svelte";
  import { countryChoices } from "../lib/countries";
  import { timezoneChoices } from "../lib/timezones";
  import { fill, i18n, t } from "../lib/i18n/index.svelte";
  import { wizard, type Network } from "../lib/wizard.svelte";

  const f = $derived(wizard.form);
  const info = $derived(wizard.info);
  // One way to the network is chosen: a list to pick from, the chosen one opened.
  const NETWORKS: { value: Network; icon: string; desc: "wifiDesc" | "ethernetDesc" | "phoneDesc" }[] = [
    { value: "wifi", icon: "wifi", desc: "wifiDesc" },
    { value: "ethernet", icon: "ethernet", desc: "ethernetDesc" },
    { value: "phone", icon: "phone", desc: "phoneDesc" },
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
  <div class="fields">
    {#if f.installFiber}
      <div class="field">
        <span>{t().settings.hostname}</span>
        <div class="control">
          <Icon name="monitor" size={15} />
          <div class="input wide mono fixed">{t().start.hubUnknown}</div>
        </div>
        <span class="hint">{t().settings.hostnameFiber}</span>
      </div>
    {:else}
      <label class="field">
        <span>{t().settings.hostname}</span>
        <div class="control">
          <Icon name="monitor" size={15} />
          <input class="input wide mono" bind:value={f.hostname} onblur={touch("hostname")} spellcheck="false" autocomplete="off" />
        </div>
        {#if err("hostname")}<span class="error">{err("hostname")}</span>{:else}<span class="hint"
            >{fill(t().settings.hostnameHint, { name: f.hostname || "fiber" })}</span
          >{/if}
      </label>
    {/if}
  </div>
</section>

<section class="card pad">
  <h2><Icon name="key" size={16} /> {t().settings.login}</h2>
  <div class="fields">
    <label class="field">
      <span>{t().settings.user}</span>
      <div class="control">
        <Icon name="user" size={15} />
        <input class="input wide mono" bind:value={f.username} onblur={touch("username")} spellcheck="false" autocomplete="off" />
      </div>
      {#if err("username")}<span class="error">{err("username")}</span>{/if}
    </label>
    <label class="field">
      <span>{t().settings.password}</span>
      <PasswordField bind:value={f.password} autocomplete="new-password" onblur={touch("password")} />
      {#if err("password")}<span class="error">{err("password")}</span>{:else}<span class="hint"
          >{fill(t().settings.passwordHint, { user: f.username })}</span
        >{/if}
    </label>
    <label class="field">
      <span>{t().settings.passwordAgain}</span>
      <PasswordField bind:value={f.password2} autocomplete="new-password" onblur={touch("password2")} />
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
  <p class="muted">{t().settings.networkLead}</p>
  <div class="choices" role="radiogroup" aria-label={t().settings.network}>
    {#each NETWORKS as n (n.value)}
      {@const chosen = f.network === n.value}
      <div class="choice" class:chosen>
        <label class="head">
          <input type="radio" name="network" value={n.value} bind:group={f.network} />
          <span class="glyph"><Icon name={n.icon} size={18} /></span>
          <span class="text">
            <span class="title">{t().settings[n.value]}</span>
            <span class="desc">{t().settings[n.desc]}</span>
          </span>
        </label>
        {#if chosen && n.value === "wifi"}
          <div class="more fields">
            <label class="field">
              <span>{t().settings.ssid}</span>
              <div class="control">
                <Icon name="wifi" size={15} />
                <input class="input wide" bind:value={f.ssid} onblur={touch("ssid")} spellcheck="false" autocomplete="off" />
              </div>
              {#if err("ssid")}<span class="error">{err("ssid")}</span>{/if}
            </label>
            <label class="field">
              <span>{t().settings.wifiPassword}</span>
              <PasswordField bind:value={f.wifiPassword} onblur={touch("wifiPassword")} />
              {#if err("wifiPassword")}<span class="error">{err("wifiPassword")}</span>{:else}<span class="hint"
                  >{t().settings.wifiPasswordHint}</span
                >{/if}
            </label>
            <label class="check">
              <input type="checkbox" bind:checked={f.hidden} />
              <span>{t().settings.hidden}</span>
            </label>
          </div>
        {:else if chosen && n.value === "ethernet"}
          <div class="more"><p class="muted">{t().settings.ethernetHint}</p></div>
        {:else if chosen}
          <div class="more"><p class="muted">{t().settings.phoneHint}</p></div>
        {/if}
      </div>
    {/each}
  </div>
  {#if networkError}<div class="note critical"><Icon name="warning" size={16} /><span>{networkError}</span></div>{/if}
</section>

<section class="card pad">
  <h2><Icon name="gps" size={16} /> {t().settings.region}</h2>
  <div class="fields">
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
</section>

<style>
  /* One field under the other, all of one width: the eye goes straight down. */
  .fields {
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-width: 460px;
  }
  .fixed {
    display: flex;
    align-items: center;
    color: var(--text-2);
    background: var(--surface-2);
  }
  .choices {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .choice {
    border: 1.5px solid var(--border-strong);
    border-radius: 10px;
    background: var(--surface);
    transition:
      border-color 0.12s ease,
      background 0.12s ease;
  }
  .choice:hover:not(.chosen) {
    border-color: var(--text-muted);
  }
  .choice.chosen {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 4%, var(--surface));
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    cursor: pointer;
  }
  .head input {
    width: 16px;
    height: 16px;
    margin: 0;
    flex-shrink: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .glyph {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    flex-shrink: 0;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-2);
  }
  .chosen .glyph {
    background: var(--accent-weak);
    color: var(--accent);
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
  /* What the chosen way needs, inside its box, in line with its title. */
  .more {
    padding: 2px 14px 14px 90px;
  }
  .check {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    cursor: pointer;
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
  @media (max-width: 620px) {
    .more {
      padding-left: 14px;
    }
  }
</style>
