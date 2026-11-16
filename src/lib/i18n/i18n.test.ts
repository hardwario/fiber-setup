import { describe, expect, it } from "vitest";
import { cs } from "./cs";
import { de } from "./de";
import { en } from "./en";
import { LANGUAGES, langOf, pieces, plain } from "./index.svelte";
import { pl } from "./pl";
import { sk } from "./sk";

const OTHERS = { cs, de, sk, pl };

/** Every text with its key path (`restart.steps.1`), so a missing, extra or odd one shows by name. */
function texts(v: unknown, path = ""): [string, string][] {
  if (typeof v === "string") return [[path, v]];
  if (Array.isArray(v)) return v.flatMap((x, i) => texts(x, `${path}.${i}`));
  if (v && typeof v === "object") return Object.entries(v).flatMap(([k, x]) => texts(x, path ? `${path}.${k}` : k));
  return [];
}

const english = new Map(texts(en));

describe("i18n", () => {
  it("lists the languages in their order", () => {
    expect(LANGUAGES.map((l) => l.code)).toEqual(["en", "cs", "de", "sk", "pl"]);
    expect(Object.keys(OTHERS).sort()).toEqual(
      LANGUAGES.map((l) => l.code)
        .filter((c) => c !== "en")
        .sort(),
    );
  });

  it.each(Object.entries(OTHERS))("%s has the same texts as English", (_, dict) => {
    expect(
      texts(dict)
        .map(([path]) => path)
        .sort(),
    ).toEqual([...english.keys()].sort());
  });

  it.each(Object.entries(OTHERS))("%s keeps the placeholders", (_, dict) => {
    const placeholders = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort();
    for (const [path, text] of texts(dict)) expect(placeholders(text), path).toEqual(placeholders(english.get(path) ?? ""));
  });

  /** A name that stands on the FIBER is the same in every language, and marked wherever English marks it. */
  it.each(Object.entries(OTHERS))("%s marks the same names on the FIBER", (_, dict) => {
    const labels = (s: string) =>
      pieces(s)
        .filter((p) => p.label)
        .map((p) => p.text)
        .sort();
    for (const [path, text] of texts(dict)) expect(labels(text), path).toEqual(labels(english.get(path) ?? ""));
  });

  it("has no long dashes and no unpaired mark", () => {
    for (const dict of [en, ...Object.values(OTHERS)]) {
      for (const [path, text] of texts(dict)) {
        expect(text, path).not.toMatch(/[–—]/);
        expect((text.match(/`/g) ?? []).length % 2, path).toBe(0);
      }
    }
  });

  it("takes a text apart at the names on the FIBER", () => {
    expect(pieces("Move the jumper to `BOOT`.")).toEqual([
      { text: "Move the jumper to ", label: false },
      { text: "BOOT", label: true },
      { text: ".", label: false },
    ]);
    expect(pieces("`RESET` again")).toEqual([
      { text: "RESET", label: true },
      { text: " again", label: false },
    ]);
    expect(pieces("Nothing marked")).toEqual([{ text: "Nothing marked", label: false }]);
    expect(plain("press `RESET` again")).toBe("press RESET again");
  });

  it("finds the app's language for the system's", () => {
    expect(langOf("cs-CZ")).toBe("cs");
    expect(langOf("de_AT")).toBe("de");
    expect(langOf("SK")).toBe("sk");
    expect(langOf("pl-PL")).toBe("pl");
    expect(langOf("fr-FR")).toBe("en");
    expect(langOf("")).toBe("en");
  });
});
