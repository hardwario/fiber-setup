import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import { wizard } from "./lib/wizard.svelte";

// No browser context menu (Reload, Inspect); text fields keep theirs for copy and paste.
document.addEventListener("contextmenu", (e) => {
  const el = e.target as HTMLElement | null;
  if (!el?.closest("input, textarea, [contenteditable='true']")) e.preventDefault();
});

// In the browser preview (npm run dev) only: the wizard on `window`, so a script can open any
// step and take its picture.
if (import.meta.env.DEV) (window as unknown as { __wizard: typeof wizard }).__wizard = wizard;

const target = document.getElementById("app");
if (!target) throw new Error("#app missing");

export default mount(App, { target });
