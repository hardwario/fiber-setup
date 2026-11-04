<script lang="ts">
  /** Theme and language for the sidebar, in one row under the steps (tower-bridge's controls, made
   * compact so the step flow fits). */
  import { LANGUAGES, type LanguagePref, fill, languageName, t } from "../i18n/index.svelte";
  import { prefs, type ThemePref } from "../prefs.svelte";
  import Icon from "./Icon.svelte";

  const THEMES: { value: ThemePref; icon: string }[] = [
    { value: "system", icon: "monitor" },
    { value: "light", icon: "sun" },
    { value: "dark", icon: "moon" },
  ];

  function themeLabel(v: ThemePref): string {
    const n = t().nav;
    return v === "system" ? n.themeSystem : v === "light" ? n.themeLight : n.themeDark;
  }

  // Our own list rather than a native <select>: the system draws that one's list in its own
  // plain style, unlike the field (Marek, 2026-11-04).
  const options = $derived<{ value: LanguagePref; label: string; lang?: string }[]>([
    { value: "system", label: fill(t().nav.languageSystem, { name: languageName(prefs.systemLang) }) },
    ...LANGUAGES.map((l) => ({ value: l.code, label: l.name, lang: l.code })),
  ]);
  const current = $derived(options.find((o) => o.value === prefs.language) ?? options[0]);
  const short = $derived(prefs.language === "system" ? t().nav.languageAuto : prefs.language.toUpperCase());
  let open = $state(false);
  let active = $state(0);
  let box = $state<HTMLDivElement | null>(null);

  function toggle() {
    open = !open;
    active = Math.max(0, options.findIndex((o) => o.value === prefs.language));
  }

  function choose(v: LanguagePref) {
    prefs.save((p) => (p.language = v));
    open = false;
  }

  function key(e: KeyboardEvent) {
    if (!open && (e.key === "ArrowDown" || e.key === "ArrowUp")) toggle();
    else if (e.key === "ArrowDown") active = Math.min(options.length - 1, active + 1);
    else if (e.key === "ArrowUp") active = Math.max(0, active - 1);
    else if (e.key === "Enter" && open) choose(options[active].value);
    else if (e.key === "Escape") open = false;
    else return;
    e.preventDefault();
  }
</script>

<div class="prefs">
  <span class="sr" id="prefs-theme-label">{t().nav.appearance}</span>
  <div class="themes" role="radiogroup" aria-labelledby="prefs-theme-label">
    {#each THEMES as th (th.value)}
      <button
        type="button"
        role="radio"
        aria-checked={prefs.theme === th.value}
        aria-label={themeLabel(th.value)}
        title={themeLabel(th.value)}
        class:active={prefs.theme === th.value}
        onclick={() => prefs.save((p) => (p.theme = th.value))}
      >
        <Icon name={th.icon} size={14} />
      </button>
    {/each}
  </div>
  <span class="sr" id="prefs-lang-label">{t().nav.language}</span>
  <div
    class="lang"
    bind:this={box}
    onfocusout={(e) => {
      if (!box?.contains(e.relatedTarget as Node | null)) open = false;
    }}
  >
    <button
      type="button"
      class="current"
      class:open
      aria-haspopup="listbox"
      aria-expanded={open}
      aria-labelledby="prefs-lang-label"
      title={`${t().nav.language}: ${current.label}`}
      onclick={toggle}
      onkeydown={key}
    >
      <Icon name="globe" size={14} />
      <span class="text">{short}</span>
      <Icon name="chevron" size={14} />
    </button>
    {#if open}
      <!-- Opens upwards: the switcher sits at the bottom of the sidebar. mousedown, not click: it
           comes before the button's blur closes the list. -->
      <ul class="list" role="listbox" aria-labelledby="prefs-lang-label">
        {#each options as o, i (o.value)}
          <li
            role="option"
            lang={o.lang}
            aria-selected={o.value === prefs.language}
            class:active={i === active}
            class:chosen={o.value === prefs.language}
            onmousedown={(e) => {
              e.preventDefault();
              choose(o.value);
            }}
            onmousemove={() => (active = i)}
          >
            {o.label}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .prefs {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  /* Read by screen readers, not shown. */
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    border: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .themes {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: 1px solid var(--border);
  }
  .themes button {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 24px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }
  .themes button:hover {
    color: var(--text);
  }
  .themes button.active {
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
  .lang {
    position: relative;
    flex: 0 0 auto;
  }
  .current {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 7px 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-muted);
    font-size: 12.5px;
    cursor: pointer;
  }
  .current:hover,
  .current.open {
    color: var(--text);
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-2);
  }
  .list {
    position: absolute;
    z-index: 20;
    right: 0;
    bottom: calc(100% + 4px);
    width: max-content;
    min-width: 100%;
    margin: 0;
    padding: 4px;
    list-style: none;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-strong);
    background: var(--surface);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18);
  }
  li {
    padding: 6px 8px;
    border-radius: 6px;
    font-size: 12.5px;
    color: var(--text-2);
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  li.active {
    background: var(--hover);
    color: var(--text);
  }
  li.chosen {
    color: var(--accent);
  }
</style>
