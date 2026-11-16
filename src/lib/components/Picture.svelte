<script lang="ts">
  /** The picture for a hands-on step: a drawing of the FIBER, made for the light ground and, in
   * `dark`, for the dark one. It stands right under the step's text, so it has no caption; `alt`
   * says what it shows. A click shows it over the whole window. */
  import { t } from "../i18n/index.svelte";
  import { prefs } from "../prefs.svelte";

  let { src, dark, alt }: { src: string; dark?: string; alt: string } = $props();
  let big = $state(false);
  const shown = $derived(dark && prefs.effectiveTheme === "dark" ? dark : src);
</script>

<svelte:window
  onkeydown={(e) => {
    if (big && e.key === "Escape") big = false;
  }}
/>

<figure class="picture">
  <button class="shot" title={t().common.enlarge} onclick={() => (big = true)}><img src={shown} {alt} /></button>
</figure>

{#if big}
  <button class="overlay" title={t().common.close} aria-label={t().common.close} onclick={() => (big = false)}>
    <img src={shown} {alt} />
  </button>
{/if}

<style>
  .picture {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }
  .shot {
    display: block;
    padding: 0;
    border: 0;
    background: none;
    cursor: zoom-in;
  }
  /* A drawing stands on its own ground plate: no frame around it. */
  .shot img {
    display: block;
    width: 100%;
    max-height: 380px;
    object-fit: contain;
  }
  /* Large, on the ground the drawing was made for. */
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 32px;
    border: 0;
    background: var(--surface);
    cursor: zoom-out;
  }
  .overlay img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
</style>
