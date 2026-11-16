<script lang="ts">
  /** A text from the dictionary. What is written between backticks there is a name that stands
   * on the FIBER itself (BOOT, RESET, EEWP…): it is set as a label, in monospace on a ground,
   * so that it reads as the thing to look for on the device. */
  import { pieces } from "../i18n/index.svelte";

  let { value }: { value: string } = $props();
</script>

{#each pieces(value) as piece, i (i)}{#if piece.label}<span class="onboard">{piece.text}</span>{:else}{piece.text}{/if}{/each}
