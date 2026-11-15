<script lang="ts">
  /** A reference photo for a hands-on step, with its caption. A click shows it over the whole window. */
  import { t } from "../i18n/index.svelte";

  let { src, alt, caption }: { src: string; alt: string; caption?: string } = $props();
  let big = $state(false);
</script>

<svelte:window
  onkeydown={(e) => {
    if (big && e.key === "Escape") big = false;
  }}
/>

<figure class="photo">
  <button class="shot" title={t().common.enlarge} onclick={() => (big = true)}><img {src} {alt} /></button>
  {#if caption}<figcaption>{caption}</figcaption>{/if}
</figure>

{#if big}
  <button class="overlay" title={t().common.close} aria-label={t().common.close} onclick={() => (big = false)}>
    <img {src} {alt} />
  </button>
{/if}

<style>
  .photo {
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
  .shot img {
    display: block;
    width: 100%;
    max-height: 380px;
    object-fit: contain;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface-2);
  }
  figcaption {
    font-size: 12.5px;
    color: var(--text-muted);
  }
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    border: 0;
    background: rgba(0, 0, 0, 0.82);
    cursor: zoom-out;
  }
  .overlay img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    border-radius: var(--radius);
  }
</style>
