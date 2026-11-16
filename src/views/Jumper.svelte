<script lang="ts">
  import jumperDark from "../assets/drawings/iso-fiber-boot-jumper-on-dark-theme.svg";
  import jumper from "../assets/drawings/iso-fiber-boot-jumper-on.svg";
  import overviewDark from "../assets/drawings/iso-fiber-open-boot-jumper-dark-theme.svg";
  import overview from "../assets/drawings/iso-fiber-open-boot-jumper.svg";
  import Steps from "../lib/components/Steps.svelte";
  import { t } from "../lib/i18n/index.svelte";
</script>

<section class="card pad">
  <Steps
    items={t().jumper.steps}
    pictures={{
      2: { src: overview, dark: overviewDark, alt: t().jumper.pinsPicture },
      3: { src: jumper, dark: jumperDark, alt: t().jumper.jumperPicture },
    }}
  />
</section>
