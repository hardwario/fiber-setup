/** Time zones for the pick list: every IANA zone the browser engine knows, with its offset now and
 * its name in the app's language ("Central European Time"), which the search also matches. */
import type { Choice } from "./search";

/** A few zones for an engine without `Intl.supportedValuesOf`. */
const FALLBACK = ["UTC", "Europe/London", "Europe/Prague", "Europe/Bratislava", "Europe/Berlin", "Europe/Warsaw"];

function zones(): string[] {
  try {
    const all = (Intl as unknown as { supportedValuesOf?: (k: string) => string[] }).supportedValuesOf?.("timeZone");
    if (all?.length) return all.includes("UTC") ? all : ["UTC", ...all];
  } catch {
    // Older engine: the short list.
  }
  return FALLBACK;
}

function part(zone: string, locale: string, style: "shortOffset" | "longGeneric"): string {
  try {
    const parts = new Intl.DateTimeFormat(locale, { timeZone: zone, timeZoneName: style }).formatToParts(new Date());
    return parts.find((p) => p.type === "timeZoneName")?.value ?? "";
  } catch {
    return "";
  }
}

/** Czech names of nearby capitals, so "Praha" or "Vídeň" find their zone too. */
const LOCAL_NAMES: Record<string, string> = {
  "Europe/Prague": "Praha",
  "Europe/Vienna": "Vídeň",
  "Europe/Warsaw": "Varšava",
  "Europe/Berlin": "Berlín",
  "Europe/London": "Londýn",
  "Europe/Rome": "Řím",
  "Europe/Budapest": "Budapešť",
  "Europe/Kyiv": "Kyjev",
  "Europe/Copenhagen": "Kodaň",
  "Europe/Brussels": "Brusel",
  "Europe/Athens": "Atény",
  "Europe/Lisbon": "Lisabon",
};

/** `extra`: a zone to list even if the engine does not know it (the computer's own). */
export function timezoneChoices(locale: string, extra?: string | null): Choice[] {
  const list = zones();
  if (extra && !list.includes(extra)) list.push(extra);
  return list
    .map((zone) => {
      const offset = part(zone, "en", "shortOffset");
      const name = part(zone, locale, "longGeneric");
      return {
        value: zone,
        label: `${zone.replace(/_/g, " ")}${offset ? ` (${offset})` : ""}`,
        keywords: `${name} ${part(zone, "en", "longGeneric")} ${zone} ${LOCAL_NAMES[zone] ?? ""}`,
      };
    })
    .sort((a, b) => a.value.localeCompare(b.value));
}
