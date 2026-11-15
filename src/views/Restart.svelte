<script lang="ts">
  import ethernet from "../assets/photos/board-ethernet.jpg";
  import reset from "../assets/photos/board-usb-reset.jpg";
  import Icon from "../lib/components/Icon.svelte";
  import Steps from "../lib/components/Steps.svelte";
  import { t } from "../lib/i18n/index.svelte";
  import { wizard } from "../lib/wizard.svelte";

  // After writing, the FIBER stays in USB mode (its memory shown as a disk) until RESET; back in
  // the boot ROM after RESET means the jumper stayed on BOOT.
  // Until this step's own first look, wizard.boards is still the Connect step's list (the boot ROM).
  let fresh = $state(false);
  $effect(() => {
    wizard.refreshUsb().then(() => (fresh = true));
    const id = setInterval(() => wizard.refreshUsb(), 1500);
    return () => clearInterval(id);
  });
  const inUsb = $derived(fresh && wizard.boards.length > 0);
  const backInBoot = $derived(fresh && wizard.boards.some((b) => b.kind === "boot"));
  let seen = $state(false);
  $effect(() => {
    if (inUsb) seen = true;
  });
</script>

<section class="card pad">
  <Steps
    items={t().restart.steps}
    photos={{ 1: { src: reset, caption: t().restart.caption }, 3: { src: ethernet, caption: t().restart.ethernet } }}
  />
</section>

{#if backInBoot}
  <div class="note warning"><Icon name="warning" size={16} /><span>{t().restart.stillBoot}</span></div>
{:else if inUsb}
  <div class="note"><Icon name="loader" size={16} /><span>{t().restart.waiting}</span></div>
{:else if seen}
  <div class="note good"><Icon name="circle-check" size={16} /><span>{t().restart.left}</span></div>
{/if}
