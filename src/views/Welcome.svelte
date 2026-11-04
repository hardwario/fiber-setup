<script lang="ts">
  import fiber from "../assets/photos/fiber-product.png";
  import Icon from "../lib/components/Icon.svelte";
  import { t } from "../lib/i18n/index.svelte";
</script>

<div class="split">
  <div class="col">
    <p class="lead">{t().welcome.lead}</p>
    <section class="card pad">
      <h2>{t().welcome.needTitle}</h2>
      <ul class="checks">
        {#each t().welcome.needs as need, i (i)}
          <li><Icon name="check" size={16} /><span>{need}</span></li>
        {/each}
      </ul>
    </section>
    <section class="card pad">
      <h2><Icon name="clock" size={16} /> {t().welcome.timeTitle}</h2>
      <p>{t().welcome.time}</p>
    </section>
    <div class="note warning"><Icon name="warning" size={16} /><span>{t().welcome.erase}</span></div>
  </div>
  <div class="col photo">
    <img src={fiber} alt="HARDWARIO FIBER" />
  </div>
</div>

<style>
  .photo img {
    width: 100%;
    max-width: 420px;
    margin: 0 auto;
    display: block;
  }
  .checks {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .checks li {
    display: flex;
    gap: 10px;
    align-items: flex-start;
  }
  .checks li :global(svg) {
    flex-shrink: 0;
    margin-top: 2px;
    color: var(--good);
  }
  p {
    margin: 0;
  }
</style>
