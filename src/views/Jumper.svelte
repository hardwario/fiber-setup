<script lang="ts">
  import jumper from "../assets/photos/board-jumper-boot.jpg";
  import overview from "../assets/photos/board-overview.jpg";
  import Steps from "../lib/components/Steps.svelte";
  import { t } from "../lib/i18n/index.svelte";
</script>

<p class="lead">{t().jumper.lead}</p>

<section class="card pad">
  <Steps
    items={t().jumper.steps}
    photos={{ 2: { src: overview, caption: t().jumper.where }, 3: { src: jumper, caption: t().jumper.caption } }}
  />
</section>
