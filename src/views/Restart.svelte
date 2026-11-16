<script lang="ts">
  import parkedDark from "../assets/drawings/iso-fiber-boot-jumper-parked-dark-theme.svg";
  import parked from "../assets/drawings/iso-fiber-boot-jumper-parked.svg";
  import ethernetDark from "../assets/drawings/iso-fiber-ethernet-cable-dark-theme.svg";
  import ethernet from "../assets/drawings/iso-fiber-ethernet-cable.svg";
  import resetDark from "../assets/drawings/iso-fiber-open-usb-cable-reset-dark-theme.svg";
  import reset from "../assets/drawings/iso-fiber-open-usb-cable-reset.svg";
  import Status from "../lib/components/Status.svelte";
  import Steps from "../lib/components/Steps.svelte";
  import Text from "../lib/components/Text.svelte";
  import { fill, t } from "../lib/i18n/index.svelte";
  import { wizard } from "../lib/wizard.svelte";

  // After writing, a FIBER stays in boot mode (its memory shown as a disk) until RESET; back in
  // the boot ROM after RESET means the jumper stayed on BOOT. Of the FIBERs on USB only those
  // that were written count here: one that was not chosen just waits in its boot ROM.
  // Until this step's own first look, wizard.boards is still an earlier step's list.
  let fresh = $state(false);
  $effect(() => wizard.watchUsb(1500, () => (fresh = true)));
  const written = $derived(wizard.done.map((d) => d.port));
  const many = $derived(written.length > 1);
  const here = $derived(fresh ? wizard.boards.filter((b) => written.includes(b.port)) : []);
  const waiting = $derived(here.filter((b) => b.kind === "gadget").map((b) => b.port));
  const backInBoot = $derived(here.filter((b) => b.kind === "boot").map((b) => b.port));
  let seen = $state(false);
  $effect(() => {
    if (here.length > 0) seen = true;
  });
  // What the app sees of the FIBERs now, pinned to the top of the page.
  const status = $derived.by((): { tone: "info" | "good" | "warning"; busy: boolean; text: string } | null => {
    const r = t().restart;
    if (backInBoot.length > 0) {
      const text = many ? fill(r.stillBootMany, { ports: backInBoot.join(", ") }) : r.stillBoot;
      return { tone: "warning", busy: false, text };
    }
    if (waiting.length > 0) {
      return { tone: "info", busy: true, text: many ? fill(r.waitingMany, { ports: waiting.join(", ") }) : r.waiting };
    }
    if (seen) return { tone: "good", busy: false, text: many ? r.leftMany : r.left };
    return null;
  });
</script>

{#if status}
  <Status tone={status.tone} busy={status.busy}><Text value={status.text} /></Status>
{/if}

<section class="card pad">
  <Steps
    items={t().restart.steps}
    pictures={{
      0: { src: parked, dark: parkedDark, alt: t().restart.parkedPicture },
      1: { src: reset, dark: resetDark, alt: t().restart.resetPicture },
      3: { src: ethernet, dark: ethernetDark, alt: t().restart.ethernetPicture },
    }}
  />
</section>
