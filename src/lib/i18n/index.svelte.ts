import { cs } from "./cs";
import { de } from "./de";
import { en, type Messages } from "./en";
import { pl } from "./pl";
import { sk } from "./sk";

export type Lang = "en" | "cs" | "de" | "sk" | "pl";
export type LanguagePref = "system" | Lang;
export type { Messages };

/** The supported languages, in the order they are shown everywhere, with their native names. */
export const LANGUAGES: { code: Lang; name: string }[] = [
  { code: "en", name: "English" },
  { code: "cs", name: "Čeština" },
  { code: "de", name: "Deutsch" },
  { code: "sk", name: "Slovenčina" },
  { code: "pl", name: "Polski" },
];

export const isLang = (v: unknown): v is Lang => LANGUAGES.some((l) => l.code === v);

/** The app's language for a system language tag (`cs-CZ`, `de_AT`): English when it has none. */
export function langOf(tag: string): Lang {
  const code = tag.toLowerCase().split(/[-_]/)[0];
  return isLang(code) ? code : "en";
}

/** The texts of both languages. In development an edited text file is swapped in here while
 * the app runs: reloading the window instead would put the wizard back at its first step. */
let dicts = $state.raw<Record<Lang, Messages>>({ en, cs, de, sk, pl });
if (import.meta.hot) {
  const codes: Lang[] = ["en", "cs", "de", "sk", "pl"];
  // The paths are written out: the bundler reads them from the source.
  import.meta.hot.accept(["./en", "./cs", "./de", "./sk", "./pl"], (edited) => {
    const next = { ...dicts };
    codes.forEach((code, i) => {
      const texts = edited[i]?.[code] as Messages | undefined;
      if (texts) next[code] = texts;
    });
    dicts = next;
  });
}

class I18n {
  lang = $state<Lang>("en");

  get m(): Messages {
    return dicts[this.lang];
  }

  set(lang: Lang) {
    this.lang = lang;
    if (typeof document !== "undefined") document.documentElement.lang = lang;
  }
}

export const i18n = new I18n();

/** Current dictionary. Reading it inside markup or `$derived` is reactive. */
export const t = (): Messages => i18n.m;

/** Replace `{name}` placeholders. */
export function fill(template: string, vars: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (match, key: string) => (key in vars ? String(vars[key]) : match));
}

/** A text in pieces: the names that stand on the FIBER itself, written between backticks in
 * the texts ("Move the jumper to `BOOT`."), apart from the words around them. */
export function pieces(text: string): { text: string; label: boolean }[] {
  return text
    .split("`")
    .map((text, i) => ({ text, label: i % 2 === 1 }))
    .filter((piece) => piece.text !== "");
}

/** The same text for a place that takes plain words only (a picture's description). */
export const plain = (text: string): string => text.replaceAll("`", "");

export function languageName(code: Lang): string {
  return LANGUAGES.find((l) => l.code === code)?.name ?? code;
}
