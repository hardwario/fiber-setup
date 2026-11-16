<script lang="ts">
  /** A password field: hidden as usual, with a button to show what was typed. */
  import { t } from "../i18n/index.svelte";
  import Icon from "./Icon.svelte";

  let {
    value = $bindable(),
    id,
    autocomplete = "off",
    icon = "lock",
    onblur,
  }: { value: string; id?: string; autocomplete?: "off" | "new-password"; icon?: string; onblur?: () => void } = $props();
  let shown = $state(false);
</script>

<div class="control password">
  <Icon name={icon} size={15} />
  <input
    class="input wide"
    type={shown ? "text" : "password"}
    {id}
    bind:value
    {autocomplete}
    {onblur}
    spellcheck="false"
    autocapitalize="off"
    autocorrect="off"
  />
  <!-- Not a stop for the Tab key: from a password it goes on to the next field. A click on it
       leaves the cursor in the field, to go on typing. -->
  <button
    class="eye"
    type="button"
    tabindex="-1"
    onmousedown={(e) => e.preventDefault()}
    aria-pressed={shown}
    aria-label={shown ? t().common.hidePassword : t().common.showPassword}
    title={shown ? t().common.hidePassword : t().common.showPassword}
    onclick={() => (shown = !shown)}
  >
    <Icon name={shown ? "eye-off" : "eye"} size={16} />
  </button>
</div>

<style>
  .password input {
    padding-right: 38px;
  }
  .eye {
    position: absolute;
    right: 3px;
    display: grid;
    place-items: center;
    width: 30px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }
  .eye:hover {
    background: var(--hover);
    color: var(--text);
  }
  .eye[aria-pressed="true"] {
    color: var(--accent);
  }
</style>
