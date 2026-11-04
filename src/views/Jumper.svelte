<script lang="ts">
  import jumper from "../assets/photos/board-jumper-boot.jpg";
  import overview from "../assets/photos/board-overview.jpg";
  import Photo from "../lib/components/Photo.svelte";
  import Steps from "../lib/components/Steps.svelte";
  import { t } from "../lib/i18n/index.svelte";
</script>

<div class="split">
  <div class="col">
    <p class="lead">{t().jumper.lead}</p>
    <section class="card pad"><Steps items={t().jumper.steps} /></section>
  </div>
  <div class="photos">
    <Photo src={overview} alt={t().common.photo} caption={t().jumper.where} />
    <Photo src={jumper} alt={t().common.photo} caption={t().jumper.caption} />
  </div>
</div>
