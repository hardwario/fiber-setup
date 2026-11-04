<script lang="ts">
  /** Numbered instructions for the hands-on steps. */
  let { items }: { items: string[] } = $props();
</script>

<ol class="steps">
  {#each items as item, i (i)}
    <li><span class="n">{i + 1}</span><span>{item}</span></li>
  {/each}
</ol>

<style>
  .steps {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    font-size: 14.5px;
    line-height: 1.5;
  }
  .n {
    flex-shrink: 0;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--accent-weak);
    color: var(--accent);
    font-size: 12.5px;
    font-weight: 600;
  }
</style>
