<script lang="ts">
  import usbDark from "../assets/drawings/iso-fiber-open-usb-cable-reset-dark-theme.svg";
  import usb from "../assets/drawings/iso-fiber-open-usb-cable-reset.svg";
  import Icon from "../lib/components/Icon.svelte";
  import Status from "../lib/components/Status.svelte";
  import Steps from "../lib/components/Steps.svelte";
  import Text from "../lib/components/Text.svelte";
  import { fill, t } from "../lib/i18n/index.svelte";
  import { MAX_DEVICES, wizard } from "../lib/wizard.svelte";

  // Look for the boards every second while this step is open. Found, the page says so and stays:
  // the user reads it and goes on with Next (it used to go on by itself after a moment, before
  // the line could be read).
  $effect(() => wizard.watchUsb(1000));

  const boards = $derived(wizard.boards);
  const access = $derived(wizard.access);
  const targets = $derived(wizard.targets);
  const status = $derived.by((): { tone: "info" | "good" | "warning"; busy: boolean; text: string } => {
    if (boards.length === 0) return { tone: "info", busy: true, text: t().connect.waiting };
    if (boards.length === 1) {
      const found = boards[0].kind === "gadget" ? t().connect.gadget : t().connect.found;
      return { tone: "good", busy: false, text: fill(found, { port: boards[0].port }) };
    }
    const text = fill(t().connect.foundMany, { count: boards.length, chosen: targets.length });
    return { tone: targets.length > 0 ? "good" : "warning", busy: false, text };
  });
  // The same step is a permission on Linux and a driver on Windows.
  const text = $derived(
    wizard.info?.os === "windows"
      ? {
          title: t().connect.driverTitle,
          text: t().connect.driverText,
          button: t().connect.driverButton,
          done: t().connect.driverDone,
          failed: t().connect.driverFailed,
        }
      : {
          title: t().connect.accessTitle,
          text: t().connect.accessText,
          button: t().connect.accessButton,
          done: t().connect.accessDone,
          failed: t().connect.accessFailed,
        },
  );
</script>

{#if wizard.info && !wizard.info.tools_found}
  <div class="note critical"><Icon name="warning" size={16} /><span>{t().connect.toolsMissing}</span></div>
{/if}

<!-- What the app sees on USB now, pinned to the top of the page. One thing turns at a time:
     while access is being allowed, its button does. -->
<Status tone={status.tone} busy={status.busy && !wizard.accessBusy}>{status.text}</Status>

<!-- Several FIBERs: all are written with the same settings, unless one is unticked. -->
{#if boards.length > 1}
  <section class="card pad">
    <h2><Icon name="list-checks" size={16} /> {t().connect.listTitle}</h2>
    <ul class="boards">
      {#each boards as b (b.port)}
        {@const over = wizard.overLimit(b.port)}
        <li class:over>
          <label>
            <input
              type="checkbox"
              checked={wizard.chosen(b.port)}
              disabled={over}
              onchange={(e) => wizard.choose(b.port, e.currentTarget.checked)}
            />
            <span class="glyph"><Icon name="usb" size={15} /></span>
            <span class="port">{fill(t().connect.choose, { port: b.port })}</span>
            <span class="kind">{b.kind === "gadget" ? t().connect.kindGadget : t().connect.kindBoot}</span>
          </label>
        </li>
      {/each}
    </ul>
    {#if boards.length > MAX_DEVICES}
      <div class="note"><Icon name="info" size={16} /><span>{fill(t().connect.limit, { max: MAX_DEVICES })}</span></div>
    {/if}
  </section>
{/if}

{#if access?.rules_installed && wizard.info?.os !== "macos"}
  <p class="muted small ok"><Icon name="check" size={12} /> {text.done}</p>
{/if}

{#if access && !access.rules_installed}
  <section class="card pad access">
    <h2><Icon name="shield" size={16} /> {text.title}</h2>
    <p>{text.text}</p>
    <div>
      <button class="btn primary" disabled={wizard.accessBusy} onclick={() => wizard.allowAccess()}>
        {#if wizard.accessBusy}<Icon name="loader" size={14} />{:else}<Icon name="lock" size={14} />{/if}
        {text.button}
      </button>
    </div>
    {#if wizard.accessError}
      <div class="note critical">
        <Icon name="warning" size={16} /><span>{fill(text.failed, { error: wizard.accessError })}</span>
      </div>
    {/if}
  </section>
{/if}

<section class="card pad">
  <Steps items={t().connect.steps} pictures={{ 2: { src: usb, dark: usbDark, alt: t().connect.picture } }} />
</section>

{#if boards.length === 0}
  <p class="muted small"><Text value={t().connect.tip} /></p>
{/if}
<p class="muted small">{fill(t().connect.more, { max: MAX_DEVICES })}</p>

<style>
  .access p,
  p {
    margin: 0;
  }
  .small {
    font-size: 12.5px;
  }
  .boards {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .boards label {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border: 1.5px solid var(--border-strong);
    border-radius: 10px;
    cursor: pointer;
  }
  .boards label:has(input:checked) {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 4%, var(--surface));
  }
  .boards input {
    width: 16px;
    height: 16px;
    margin: 0;
    flex-shrink: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .boards .glyph {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    flex-shrink: 0;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-2);
  }
  .boards .port {
    font-weight: 600;
  }
  .boards .kind {
    margin-left: auto;
    font-size: 12.5px;
    color: var(--text-muted);
  }
  /* One more than can be written at once. */
  .boards .over label {
    border-color: var(--border);
    cursor: default;
  }
  .boards .over .glyph,
  .boards .over .port {
    opacity: 0.5;
  }
  .ok {
    display: flex;
    align-items: center;
    gap: 5px;
  }
</style>
