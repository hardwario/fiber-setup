<script lang="ts">
  import fiberDark from "../assets/drawings/iso-fiber-closed-dark-theme.svg";
  import fiber from "../assets/drawings/iso-fiber-closed.svg";
  import Icon from "../lib/components/Icon.svelte";
  import UpdateNotice from "../lib/components/UpdateNotice.svelte";
  import { t } from "../lib/i18n/index.svelte";
  import { prefs } from "../lib/prefs.svelte";

  // One glyph for each thing on the "What you need" list, in its order.
  const NEEDS = ["plug", "usb", "wrench", "wifi"];
</script>

<UpdateNotice />

<img class="product" src={prefs.effectiveTheme === "dark" ? fiberDark : fiber} alt="HARDWARIO FIBER" />

<section class="card pad">
  <h2><Icon name="list-checks" size={16} /> {t().welcome.needTitle}</h2>
  <ul class="needs">
    {#each t().welcome.needs as need, i (i)}
      <li><span class="glyph"><Icon name={NEEDS[i] ?? "check"} size={17} /></span><span>{need}</span></li>
    {/each}
  </ul>
</section>

<section class="card pad">
  <h2><Icon name="clock" size={16} /> {t().welcome.timeTitle}</h2>
  <p>{t().welcome.time}</p>
</section>

<div class="note warning"><Icon name="warning" size={16} /><span>{t().welcome.erase}</span></div>

<style>
  /* The drawing of the FIBER, on the ground plate made for the theme. */
  .product {
    display: block;
    width: 100%;
    max-width: 340px;
    margin: 0 auto;
  }
  .needs {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .needs li {
    display: flex;
    gap: 12px;
    align-items: center;
  }
  .glyph {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-2);
  }
  p {
    margin: 0;
  }
</style>
