<script lang="ts">
  import usb from "../assets/photos/board-usb-reset.jpg";
  import Icon from "../lib/components/Icon.svelte";
  import Photo from "../lib/components/Photo.svelte";
  import Steps from "../lib/components/Steps.svelte";
  import { fill, t } from "../lib/i18n/index.svelte";
  import { STEPS, wizard } from "../lib/wizard.svelte";

  // Look for the board every second while this step is open; go on by itself once it is there.
  $effect(() => {
    wizard.refreshUsb();
    const id = setInterval(() => wizard.refreshUsb(), 1000);
    return () => clearInterval(id);
  });

  // Only the first time: coming back to this step from a later one stays here. No cleanup: the
  // polling above re-runs this effect every second, which would cancel the timer each time.
  let advanced = false;
  $effect(() => {
    if (wizard.canNext && !advanced && wizard.reached === STEPS.indexOf("connect")) {
      advanced = true;
      setTimeout(() => {
        if (wizard.step === "connect" && wizard.canNext) wizard.next();
      }, 1500);
    }
  });

  const boards = $derived(wizard.boards);
  const access = $derived(wizard.access);
  const board = $derived(boards.find((b) => b.port === wizard.port));
</script>

<div class="split">
  <div class="col">
    {#if wizard.info && !wizard.info.tools_found}
      <div class="note critical"><Icon name="warning" size={16} /><span>{t().connect.toolsMissing}</span></div>
    {/if}
    {#if access && !access.rules_installed}
      <section class="card pad access">
        <h2><Icon name="shield" size={16} /> {t().connect.accessTitle}</h2>
        <p>{t().connect.accessText}</p>
        <div>
          <button class="btn primary" disabled={wizard.accessBusy} onclick={() => wizard.allowAccess()}>
            {#if wizard.accessBusy}<Icon name="loader" size={14} />{:else}<Icon name="lock" size={14} />{/if}
            {t().connect.accessButton}
          </button>
        </div>
        {#if wizard.accessError}
          <div class="note critical">
            <Icon name="warning" size={16} /><span>{fill(t().connect.accessFailed, { error: wizard.accessError })}</span>
          </div>
        {/if}
      </section>
    {/if}
    <section class="card pad"><Steps items={t().connect.steps} /></section>
    <section class="card pad status">
      {#if board}
        <div class="found">
          <Icon name="circle-check" size={18} />
          <span>{fill(board.kind === "gadget" ? t().connect.gadget : t().connect.found, { port: board.port })}</span>
        </div>
      {:else if boards.length > 1}
        <div class="note warning"><Icon name="warning" size={16} /><span>{t().connect.several}</span></div>
        <div class="ports">
          {#each boards as b (b.port)}
            <button class="btn" onclick={() => wizard.choosePort(b.port)}>
              <Icon name="usb" size={14} />
              {fill(t().connect.choose, { port: b.port })}
            </button>
          {/each}
        </div>
      {:else}
        <div class="waiting"><Icon name="loader" size={16} /><span>{t().connect.waiting}</span></div>
        <p class="muted small">{t().connect.tip}</p>
      {/if}
      {#if access?.rules_installed}
        <p class="muted small ok"><Icon name="check" size={12} /> {t().connect.accessDone}</p>
      {/if}
    </section>
  </div>
  <div class="col">
    <Photo src={usb} alt={t().common.photo} caption={t().connect.caption} />
  </div>
</div>

<style>
  .access p,
  .status p {
    margin: 0;
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
  .small {
    font-size: 12.5px;
  }
  .ports {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .ok {
    display: flex;
    align-items: center;
    gap: 5px;
  }
</style>
