<script lang="ts">
  /** What the app sees right now on a step that waits for something to happen on the FIBER:
   * "waiting for RESET", "it has left boot mode". It stays at the top of the window while the
   * page scrolls under it, so a change is seen wherever the user is reading, and it shows itself
   * for a moment when it changes. Every step with such a state shows it this way, as its first
   * thing under the heading. */
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  type Tone = "info" | "good" | "warning" | "critical";
  let { tone = "info", busy = false, children }: { tone?: Tone; busy?: boolean; children: Snippet } = $props();
  const icon = $derived(busy ? "loader" : tone === "good" ? "circle-check" : tone === "info" ? "info" : "warning");
</script>

<!-- The region itself stays, so that a screen reader says what changed in it. -->
<div class="pinned" role="status" aria-live="polite">
  {#key tone}
    <div class="note {tone} now">
      <Icon name={icon} size={16} /><span>{@render children()}</span>
    </div>
  {/key}
</div>

<style>
  /* Pinned to the top of the scrolled page. Its own ground hides what scrolls under it (a little
     above and beside the note too) and fades out below it. */
  .pinned {
    position: sticky;
    top: 0;
    z-index: 5;
    margin: -12px -8px -4px;
    padding: 12px 8px 4px;
    background: var(--bg);
  }
  .pinned::after {
    content: "";
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    height: 12px;
    background: linear-gradient(var(--bg), transparent);
    pointer-events: none;
  }
  .now {
    font-size: 14px;
    animation: changed 1.2s ease-out;
  }
  /* A new state rings once in its own colour. */
  @keyframes changed {
    from {
      box-shadow: 0 0 0 4px color-mix(in srgb, currentColor 40%, transparent);
    }
    to {
      box-shadow: 0 0 0 4px transparent;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .now {
      animation: none;
    }
  }
</style>
