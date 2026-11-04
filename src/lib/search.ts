/** Matching for the searchable pick lists: case and accents do not matter ("cesko" finds Česko). */

export interface Choice {
  value: string;
  label: string;
  /** Extra words that should find it (an English name, a code), besides the label. */
  keywords?: string;
}

export function fold(s: string): string {
  return s.normalize("NFD").replace(/\p{Diacritic}/gu, "").toLowerCase();
}

/** The choices whose label or keywords contain every typed word; those starting with the first word
 * come first. */
export function filterChoices(choices: Choice[], query: string): Choice[] {
  const words = fold(query).split(/[\s,/()]+/).filter(Boolean);
  if (!words.length) return choices;
  const hits = choices.filter((c) => {
    const text = fold(`${c.label} ${c.keywords ?? ""}`);
    return words.every((w) => text.includes(w));
  });
  const starts = (c: Choice) => {
    const parts = fold(`${c.label} ${c.keywords ?? ""}`).split(/[\s,/()]+/);
    return parts.some((p) => p.startsWith(words[0]));
  };
  return [...hits.filter(starts), ...hits.filter((c) => !starts(c))];
}
