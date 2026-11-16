/** Theme and language: the user's choice, kept in localStorage (a per-computer convenience). */
import { i18n, isLang, type Lang, type LanguagePref } from "./i18n/index.svelte";

export type ThemePref = "system" | "light" | "dark";

const query = typeof window === "undefined" ? null : window.matchMedia("(prefers-color-scheme: dark)");
const KEY = "fiber-setup.prefs";

function load(): { theme: ThemePref; language: LanguagePref } {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return {
      theme: ["system", "light", "dark"].includes(v.theme) ? v.theme : "system",
      language: v.language === "system" || isLang(v.language) ? v.language : "system",
    };
  } catch {
    return { theme: "system", language: "system" };
  }
}

class Prefs {
  theme = $state<ThemePref>("system");
  language = $state<LanguagePref>("system");
  systemDark = $state(query?.matches ?? false);
  /** The language the computer uses, from the backend. */
  systemLang = $state<Lang>("en");

  constructor() {
    const p = load();
    this.theme = p.theme;
    this.language = p.language;
    query?.addEventListener("change", (e) => (this.systemDark = e.matches));
  }

  get effectiveTheme(): "light" | "dark" {
    if (this.theme === "system") return this.systemDark ? "dark" : "light";
    return this.theme;
  }

  apply() {
    const root = document.documentElement;
    if (this.theme === "system") delete root.dataset.theme;
    else root.dataset.theme = this.theme;
    i18n.set(this.language === "system" ? this.systemLang : this.language);
  }

  save(change: (p: Prefs) => void) {
    change(this);
    this.apply();
    try {
      localStorage.setItem(KEY, JSON.stringify({ theme: this.theme, language: this.language }));
    } catch {
      // Private mode or blocked storage: the choice lasts until the window closes.
    }
  }
}

export const prefs = new Prefs();
