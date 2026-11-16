<script lang="ts">
  /** Numbered instructions for the hands-on steps, one under the other on a line; the picture
   * that goes with a step sits right under its text. */
  import Picture from "./Picture.svelte";
  import Text from "./Text.svelte";

  type Shot = { src: string; dark?: string; alt: string };
  let { items, pictures = {} }: { items: string[]; pictures?: Record<number, Shot> } = $props();
</script>

<ol class="steps">
  {#each items as item, i (i)}
    <li>
      <span class="n">{i + 1}</span>
      <div class="body">
        <p><Text value={item} /></p>
        {#if pictures[i]}
          <Picture src={pictures[i].src} dark={pictures[i].dark} alt={pictures[i].alt} />
        {/if}
      </div>
    </li>
  {/each}
</ol>

<style>
  .steps {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    position: relative;
    display: flex;
    gap: 14px;
    align-items: flex-start;
    padding-bottom: 16px;
    font-size: 14.5px;
    line-height: 1.5;
  }
  li:last-child {
    padding-bottom: 0;
  }
  /* The line from one number to the next. */
  li:not(:last-child)::before {
    content: "";
    position: absolute;
    left: 13px;
    top: 30px;
    bottom: 2px;
    width: 2px;
    border-radius: 1px;
    background: var(--border-strong);
  }
  .n {
    flex-shrink: 0;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: var(--accent-weak);
    color: var(--accent);
    font-size: 13px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .body {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
    padding-top: 3px;
  }
  p {
    margin: 0;
  }
  .body :global(.picture) {
    max-width: 520px;
    padding-bottom: 4px;
  }
</style>
