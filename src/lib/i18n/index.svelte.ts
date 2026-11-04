import { cs } from "./cs";
import { en, type Messages } from "./en";

export type Lang = "en" | "cs";
export type LanguagePref = "system" | Lang;
export type { Messages };

/** Languages in the order shown, with their native names. tower-bridge has five; these two
 * come first, the others follow when the texts settle. */
export const LANGUAGES: { code: Lang; name: string }[] = [
  { code: "en", name: "English" },
  { code: "cs", name: "Čeština" },
];

const DICTS: Record<Lang, Messages> = { en, cs };

class I18n {
  lang = $state<Lang>("en");

  get m(): Messages {
    return DICTS[this.lang];
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

export function languageName(code: Lang): string {
  return LANGUAGES.find((l) => l.code === code)?.name ?? code;
}
