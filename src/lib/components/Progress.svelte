<script lang="ts">
  /** One job's current stage: its name, a bar and the megabytes when it has them. */
  import { t } from "../i18n/index.svelte";
  import type { Phase } from "../types";
  import Icon from "./Icon.svelte";

  let { phase, done, total, busy }: { phase: Phase | null; done: number; total: number; busy: boolean } = $props();

  const pct = $derived(total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0);
  const mb = (n: number) => Math.round(n / 1048576).toLocaleString();
</script>

{#if phase}
  <div class="progress" aria-live="polite">
    <div class="row">
      {#if busy}<Icon name="loader" size={14} />{:else}<Icon name="check" size={14} />{/if}
      <span class="name">{t().phase[phase]}</span>
      {#if total > 0}<span class="nums tabular">{mb(done)} / {mb(total)} MB · {pct} %</span>{/if}
    </div>
    <div class="bar" class:indeterminate={busy && total === 0}>
      <div class="fill" style:width={total > 0 ? `${pct}%` : busy ? "30%" : "100%"}></div>
    </div>
  </div>
{/if}

<style>
  .progress {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-2);
  }
  .name {
    flex: 1;
    font-weight: 500;
    color: var(--text);
  }
  .nums {
    font-size: 12.5px;
    color: var(--text-muted);
  }
  .bar {
    position: relative;
    height: 8px;
    border-radius: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 4px;
    background: var(--accent);
    transition: width 0.25s ease;
  }
  .indeterminate .fill {
    position: absolute;
    animation: slide 1.4s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      left: -30%;
    }
    to {
      left: 100%;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .indeterminate .fill {
      animation-duration: 4s;
    }
  }
</style>
