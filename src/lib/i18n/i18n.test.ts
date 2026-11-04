import { describe, expect, it } from "vitest";
import { cs } from "./cs";
import { en } from "./en";

/** Every key path, with list lengths, so a missing or extra text shows by name. */
function shape(v: unknown, path = ""): string[] {
  if (Array.isArray(v)) return [`${path}[${v.length}]`];
  if (v && typeof v === "object") {
    return Object.entries(v).flatMap(([k, x]) => shape(x, path ? `${path}.${k}` : k));
  }
  return [path];
}

function strings(v: unknown): string[] {
  if (typeof v === "string") return [v];
  if (Array.isArray(v)) return v.flatMap(strings);
  if (v && typeof v === "object") return Object.values(v).flatMap(strings);
  return [];
}

describe("i18n", () => {
  it("has the same texts in every language", () => {
    expect(shape(cs).sort()).toEqual(shape(en).sort());
  });

  it("keeps the same placeholders", () => {
    const placeholders = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort();
    const e = strings(en);
    const c = strings(cs);
    e.forEach((s, i) => expect(placeholders(c[i]), s).toEqual(placeholders(s)));
  });

  it("has no long dashes (tower-bridge rule)", () => {
    for (const s of [...strings(en), ...strings(cs)]) expect(s).not.toMatch(/[\u2013\u2014]/);
  });
});
