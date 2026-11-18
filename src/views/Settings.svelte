<script lang="ts">
  import Icon from "../lib/components/Icon.svelte";
  import PasswordField from "../lib/components/PasswordField.svelte";
  import SearchSelect from "../lib/components/SearchSelect.svelte";
  import { countryChoices } from "../lib/countries";
  import { timezoneChoices } from "../lib/timezones";
  import { fill, i18n, t } from "../lib/i18n/index.svelte";
  import { neededBy } from "../lib/platform";
  import type { Component, MiniPcie } from "../lib/types";
  import { PHONE_SETUP, wizard, type Network } from "../lib/wizard.svelte";

  const f = $derived(wizard.form);
  const info = $derived(wizard.info);
  // The Ethernet cable always works, so the choice is about Wi-Fi only: a list to pick from, the
  // chosen one opened.
  const NETWORKS: { value: Network; icon: string; desc: "wifiDesc" | "phoneDesc" | "noneDesc" }[] = [
    // The default first: what works with nothing more to enter.
    { value: "none", icon: "ethernet", desc: "noneDesc" },
    { value: "wifi", icon: "wifi", desc: "wifiDesc" },
    ...(PHONE_SETUP ? [{ value: "phone" as Network, icon: "phone", desc: "phoneDesc" as const }] : []),
  ];
  const offer = $derived(info?.platform ?? null);
  /** Its name and sentence in the user's language, or the release's English ones. */
  function text(c: Component): { title: string; about: string } {
    const own = (t().components as Record<string, { title: string; about: string } | undefined>)[c.id];
    return own ?? { title: c.title, about: c.about };
  }
  /** Ticks or unticks a component. Wi-Fi from the phone app comes with the FIBER agent: without
   * it, that choice falls back to no Wi-Fi. */
  function tick(id: string, on: boolean) {
    wizard.tick(id, on);
    if (!f.components.includes("agent") && f.network === "phone") f.network = "none";
  }
  const CARDS: MiniPcie[] = ["none", "lte", "rak5146"];

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

<!-- Each setting is one line: its name on the left, the field on the right. The names make a
     column the eye runs down, and the fields fill the box. -->
<section class="card pad">
  <h2><Icon name="monitor" size={16} /> {t().settings.device}</h2>
  <div class="lines">
    {#if wizard.installFiber}
      <div class="line">
        <span class="name">{t().settings.hostname}</span>
        <div class="value">
          <div class="control">
            <Icon name="monitor" size={15} />
            <div class="input wide mono fixed">{t().start.hubUnknown}</div>
          </div>
          <span class="hint">{t().settings.hostnameFiber}</span>
        </div>
      </div>
    {:else}
      <div class="line">
        <label class="name" for="hostname">{t().settings.hostname}</label>
        <div class="value">
          <div class="control">
            <Icon name="monitor" size={15} />
            <input
              id="hostname"
              class="input wide mono"
              bind:value={f.hostname}
              onblur={touch("hostname")}
              spellcheck="false"
              autocomplete="off"
            />
          </div>
          {#if err("hostname")}<span class="error">{err("hostname")}</span>{:else}<span class="hint"
              >{fill(t().settings.hostnameHint, { name: f.hostname || "fiber" })}</span
            >{/if}
        </div>
      </div>
    {/if}
    <div class="line">
      <label class="name" for="timezone">{t().settings.timezone}</label>
      <div class="value">
        <SearchSelect id="timezone" choices={timezones} bind:value={f.timezone} placeholder={t().settings.typeToSearch} />
        {#if err("timezone")}<span class="error">{err("timezone")}</span>{/if}
      </div>
    </div>
  </div>
</section>

<section class="card pad">
  <h2><Icon name="key" size={16} /> {t().settings.login}</h2>
  <div class="lines">
    <div class="line">
      <label class="name" for="username">{t().settings.user}</label>
      <div class="value">
        <div class="control">
          <Icon name="user" size={15} />
          <input
            id="username"
            class="input wide mono"
            bind:value={f.username}
            onblur={touch("username")}
            spellcheck="false"
            autocomplete="off"
          />
        </div>
        {#if err("username")}<span class="error">{err("username")}</span>{/if}
      </div>
    </div>
    <div class="line">
      <label class="name" for="password">{t().settings.password}</label>
      <div class="value">
        <PasswordField id="password" bind:value={f.password} autocomplete="new-password" onblur={touch("password")} />
        {#if err("password")}<span class="error">{err("password")}</span>{:else}<span class="hint"
            >{fill(t().settings.passwordHint, { user: f.username })}</span
          >{/if}
      </div>
    </div>
    <div class="line">
      <label class="name" for="password2">{t().settings.passwordAgain}</label>
      <div class="value">
        <PasswordField id="password2" bind:value={f.password2} autocomplete="new-password" onblur={touch("password2")} />
        {#if err("password2")}<span class="error">{err("password2")}</span>{/if}
      </div>
    </div>
    <div class="line">
      <span class="name">{t().settings.sshKeyName}</span>
      <div class="value">
        {#if info?.defaults.ssh_key}
          <label class="check">
            <input type="checkbox" bind:checked={f.useSshKey} />
            <span>
              {t().settings.sshKey}
              <span class="hint block mono">{fill(t().settings.sshKeyFrom, { path: info.defaults.ssh_key_path ?? "" })}</span>
            </span>
          </label>
        {:else}
          <span class="hint none">{t().settings.noSshKey}</span>
        {/if}
      </div>
    </div>
  </div>
</section>

{#if offer}
  <section class="card pad">
    <h2><Icon name="package" size={16} /> {t().settings.software}</h2>
    <p class="muted">{fill(t().settings.softwareLead, { version: offer.release.version })}</p>
    <div class="components">
      {#each offer.components as c (c.id)}
        {@const on = f.components.includes(c.id)}
        {@const needs = neededBy(c.id, f.components, offer.components)}
        <div class="component" class:on>
          <label class="check">
            <input type="checkbox" checked={on} disabled={needs.length > 0} onchange={(e) => tick(c.id, e.currentTarget.checked)} />
            <span class="text">
              <span class="title">{text(c).title}</span>
              {#if c.size?.download}<span class="hint">{fill(t().settings.download, { size: c.size.download })}</span>{/if}
              <span class="hint block">{text(c).about}</span>
              {#if needs.length > 0}
                <span class="hint block"
                  >{fill(t().settings.neededBy, {
                    names: needs.map((id) => text(offer.components.find((k) => k.id === id) ?? c).title).join(", "),
                  })}</span
                >
              {/if}
            </span>
          </label>
          {#if on && c.ports.some((p) => p.when === "mqtt.lan")}
            <label class="check more-check">
              <input type="checkbox" bind:checked={f.mqttLan} />
              <span>{t().settings.mqttLan}</span>
            </label>
          {/if}
        </div>
      {/each}
    </div>
    {#if wizard.installFiber}
      <div class="lines">
        <div class="line">
          <label class="name" for="minipcie">{t().settings.minipcie}</label>
          <div class="value">
            <select id="minipcie" class="input wide" bind:value={f.minipcie}>
              {#each CARDS as card (card)}<option value={card}>{t().settings.cards[card]}</option>{/each}
            </select>
            <span class="hint">{t().settings.minipcieHint}</span>
          </div>
        </div>
      </div>
    {:else}
      <div class="note"><Icon name="info" size={16} /><span>{t().settings.nothing}</span></div>
    {/if}
  </section>
{/if}

<section class="card pad">
  <h2><Icon name="globe" size={16} /> {t().settings.network}</h2>
  <p class="muted">{t().settings.networkLead}</p>
  <div class="choices" role="radiogroup" aria-label={t().settings.network}>
    {#each NETWORKS as n (n.value)}
      {@const chosen = f.network === n.value}
      {@const off = n.value === "phone" && !f.components.includes("agent")}
      <div class="choice" class:chosen class:off>
        <label class="head">
          <input type="radio" name="network" value={n.value} bind:group={f.network} disabled={off} />
          <span class="glyph"><Icon name={n.icon} size={18} /></span>
          <span class="text">
            <span class="title">{t().settings[n.value]}</span>
            <span class="desc">{off ? t().settings.phoneOff : t().settings[n.desc]}</span>
          </span>
        </label>
        {#if chosen && n.value === "wifi"}
          <div class="more lines">
            <div class="line">
              <label class="name" for="ssid">{t().settings.ssid}</label>
              <div class="value">
                <div class="control">
                  <Icon name="wifi" size={15} />
                  <input id="ssid" class="input wide" bind:value={f.ssid} onblur={touch("ssid")} spellcheck="false" autocomplete="off" />
                </div>
                {#if err("ssid")}<span class="error">{err("ssid")}</span>{/if}
              </div>
            </div>
            <div class="line">
              <label class="name" for="wifi-password">{t().settings.wifiPassword}</label>
              <div class="value">
                <PasswordField id="wifi-password" bind:value={f.wifiPassword} onblur={touch("wifiPassword")} />
                {#if err("wifiPassword")}<span class="error">{err("wifiPassword")}</span>{:else}<span class="hint"
                    >{t().settings.wifiPasswordHint}</span
                  >{/if}
                <label class="check">
                  <input type="checkbox" bind:checked={f.hidden} />
                  <span>{t().settings.hidden}</span>
                </label>
              </div>
            </div>
          </div>
        {:else if chosen && n.value === "phone"}
          <div class="more"><p class="muted">{t().settings.phoneHint}</p></div>
        {:else if chosen}
          <div class="more"><p class="muted">{t().settings.noneHint}</p></div>
        {/if}
      </div>
    {/each}
  </div>
  {#if f.network !== "none"}
    <div class="lines">
      <div class="line">
        <label class="name" for="country">{t().settings.country}</label>
        <div class="value">
          <SearchSelect id="country" choices={countries} bind:value={f.country} placeholder={t().settings.typeToSearch} />
          <span class="hint">{t().settings.countryHint}</span>
        </div>
      </div>
    </div>
  {/if}
  {#if networkError}<div class="note critical"><Icon name="warning" size={16} /><span>{networkError}</span></div>{/if}
</section>

<style>
  .lines {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .line {
    display: grid;
    grid-template-columns: 190px minmax(0, 1fr);
    gap: 4px 20px;
    align-items: start;
  }
  /* In line with the text in its field. */
  .name {
    padding-top: 7px;
    font-size: 13.5px;
    font-weight: 500;
    color: var(--text-2);
  }
  .value {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
  }
  .fixed {
    display: flex;
    align-items: center;
    color: var(--text-2);
    background: var(--surface-2);
  }
  .hint {
    font-size: 12px;
    color: var(--text-muted);
  }
  .hint.none {
    padding-top: 8px;
  }
  .error {
    font-size: 12px;
    color: var(--critical);
  }
  .block {
    display: block;
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
  .choice:hover:not(.chosen):not(.off) {
    border-color: var(--text-muted);
  }
  .choice.chosen {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 4%, var(--surface));
  }
  /* A choice that cannot be made now: pale, and its text says what it needs. */
  .choice.off {
    border-color: var(--border);
  }
  .choice.off .head,
  .choice.off .head input {
    cursor: default;
  }
  .choice.off .glyph,
  .choice.off .title {
    opacity: 0.5;
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
  /* What the chosen way needs, inside its box. Its names line up with the ones outside:
     190 px less the box's own 15.5 px of border and padding. */
  .more {
    padding: 4px 14px 14px;
  }
  .more .line {
    grid-template-columns: 174.5px minmax(0, 1fr);
  }
  .more p {
    padding-left: 76px;
  }
  .check {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    cursor: pointer;
  }
  .value > .check {
    padding-top: 6px;
  }
  .check input {
    margin-top: 3px;
    accent-color: var(--accent);
  }
  /* The components: one line each, its sentence below its name. */
  .components {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .component {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .component .text {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px 8px;
  }
  .component .text .block {
    flex-basis: 100%;
  }
  /* A setting of the component, under its sentence. */
  .more-check {
    padding-left: 24px;
    font-size: 13px;
  }
  p {
    margin: 0;
  }
  /* Too narrow for a name beside its field: the name goes above it. */
  @container (max-width: 520px) {
    .line,
    .more .line {
      grid-template-columns: minmax(0, 1fr);
    }
    .name {
      padding-top: 0;
    }
    .more p {
      padding-left: 0;
    }
  }
</style>
