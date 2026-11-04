<script lang="ts">
  /**
   * A pick list you can type into: the list narrows to what matches, arrow keys move, Enter picks,
   * Escape gives up. Shows the chosen label while it is not being typed in.
   */
  import { filterChoices, type Choice } from "../search";
  import Icon from "./Icon.svelte";

  let {
    choices,
    value = $bindable(),
    placeholder = "",
    empty = "",
    id,
  }: { choices: Choice[]; value: string; placeholder?: string; empty?: string; id: string } = $props();

  let open = $state(false);
  let query = $state("");
  let active = $state(0);
  let input = $state<HTMLInputElement | null>(null);
  let list = $state<HTMLUListElement | null>(null);

  const selected = $derived(choices.find((c) => c.value === value));
  const shown = $derived(filterChoices(choices, query));

  function start() {
    open = true;
    query = "";
    active = Math.max(0, shown.findIndex((c) => c.value === value));
    queueMicrotask(() => scrollToActive());
  }

  function pick(c: Choice) {
    value = c.value;
    open = false;
    query = "";
    input?.blur();
  }

  function scrollToActive() {
    list?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }

  function key(e: KeyboardEvent) {
    if (!open && (e.key === "ArrowDown" || e.key === "Enter")) {
      start();
      e.preventDefault();
      return;
    }
    if (e.key === "ArrowDown") active = Math.min(shown.length - 1, active + 1);
    else if (e.key === "ArrowUp") active = Math.max(0, active - 1);
    else if (e.key === "Enter" && shown[active]) pick(shown[active]);
    else if (e.key === "Escape") {
      open = false;
      query = "";
      input?.blur();
    } else return;
    e.preventDefault();
    scrollToActive();
  }
</script>

<div class="search-select" class:open>
  <span class="icon"><Icon name={open ? "search" : "chevron"} size={14} /></span>
  <input
    bind:this={input}
    {id}
    class="input wide"
    role="combobox"
    aria-expanded={open}
    aria-controls={`${id}-list`}
    aria-autocomplete="list"
    autocomplete="off"
    spellcheck="false"
    placeholder={open ? placeholder : ""}
    value={open ? query : (selected?.label ?? empty)}
    onfocus={start}
    oninput={(e) => {
      query = e.currentTarget.value;
      active = 0;
      if (!open) open = true;
    }}
    onkeydown={key}
    onblur={() => {
      open = false;
      query = "";
    }}
  />
  {#if open}
    <ul class="list" role="listbox" id={`${id}-list`} bind:this={list}>
      {#each shown as c, i (c.value)}
        <!-- mousedown, not click: it comes before the input's blur closes the list. -->
        <li
          role="option"
          data-index={i}
          aria-selected={c.value === value}
          class:active={i === active}
          class:chosen={c.value === value}
          onmousedown={(e) => {
            e.preventDefault();
            pick(c);
          }}
          onmousemove={() => (active = i)}
        >
          {c.label}
        </li>
      {:else}
        <li class="none">-</li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .search-select {
    position: relative;
  }
  .icon {
    position: absolute;
    right: 9px;
    top: 10px;
    display: flex;
    color: var(--text-muted);
    pointer-events: none;
  }
  .search-select :global(input) {
    padding-right: 30px;
  }
  .list {
    position: absolute;
    z-index: 20;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    max-height: 260px;
    overflow-y: auto;
    margin: 0;
    padding: 4px;
    list-style: none;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-strong);
    background: var(--surface);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18);
  }
  li {
    padding: 6px 8px;
    border-radius: 6px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  li.active {
    background: var(--hover);
  }
  li.chosen {
    color: var(--accent);
  }
  li.none {
    color: var(--text-muted);
    cursor: default;
  }
</style>
