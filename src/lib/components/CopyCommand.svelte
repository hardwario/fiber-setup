<script lang="ts">
  /** A command to run somewhere else, with a button that puts it on the clipboard. */
  import { t } from "../i18n/index.svelte";
  import { wizard } from "../wizard.svelte";
  import Icon from "./Icon.svelte";

  let { text }: { text: string } = $props();
  let copied = $state(false);
  let failed = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function copy() {
    clearTimeout(timer);
    try {
      await wizard.copy(text);
      copied = true;
      failed = false;
    } catch {
      // The text stays selectable: it can still be copied by hand.
      failed = true;
    }
    timer = setTimeout(() => {
      copied = false;
      failed = false;
    }, 2000);
  }
</script>

<div class="cmd">
  <Icon name="terminal" size={16} />
  <code>{text}</code>
  <button class="btn" type="button" onclick={copy} aria-live="polite">
    {#if copied}
      <Icon name="check" size={14} /> {t().common.copied}
    {:else if failed}
      <Icon name="warning" size={14} /> {t().common.copyFailed}
    {:else}
      <Icon name="copy" size={14} /> {t().common.copy}
    {/if}
  </button>
</div>

<style>
  .cmd {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 6px 6px 12px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
  }
  .cmd > :global(svg) {
    flex-shrink: 0;
    color: var(--text-muted);
  }
  code {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
    font-size: 13.5px;
    -webkit-user-select: text;
    user-select: text;
    cursor: text;
  }
  .btn {
    flex-shrink: 0;
  }
</style>
