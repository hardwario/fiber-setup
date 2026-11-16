import { LANGUAGES } from "./i18n/index.svelte";
import type { Choice } from "./search";

/**
 * Countries for the Wi-Fi regulatory domain (the channels and power a FIBER may use there): the
 * ISO 3166-1 alpha-2 codes the Linux wireless regulatory database knows. Names come from the
 * browser engine (`Intl.DisplayNames`) in the app's language.
 */
export const COUNTRY_CODES = [
  "AD", "AE", "AF", "AI", "AL", "AM", "AN", "AR", "AS", "AT", "AU", "AW", "AZ", "BA", "BB", "BD", "BE", "BF", "BG",
  "BH", "BL", "BM", "BN", "BO", "BR", "BS", "BT", "BY", "BZ", "CA", "CF", "CH", "CI", "CL", "CN", "CO", "CR", "CU",
  "CX", "CY", "CZ", "DE", "DK", "DM", "DO", "DZ", "EC", "EE", "EG", "ES", "ET", "FI", "FM", "FR", "GB", "GD", "GE",
  "GF", "GH", "GL", "GP", "GR", "GT", "GU", "GY", "HK", "HN", "HR", "HT", "HU", "ID", "IE", "IL", "IN", "IR", "IS",
  "IT", "JM", "JO", "JP", "KE", "KH", "KN", "KP", "KR", "KW", "KY", "KZ", "LB", "LC", "LI", "LK", "LS", "LT", "LU",
  "LV", "MA", "MC", "MD", "ME", "MF", "MH", "MK", "MN", "MO", "MP", "MQ", "MR", "MT", "MU", "MV", "MW", "MX", "MY",
  "NG", "NI", "NL", "NO", "NP", "NZ", "OM", "PA", "PE", "PF", "PG", "PH", "PK", "PL", "PM", "PR", "PT", "PW", "PY",
  "QA", "RE", "RO", "RS", "RU", "RW", "SA", "SE", "SG", "SI", "SK", "SN", "SR", "SV", "SY", "TC", "TD", "TG", "TH",
  "TN", "TR", "TT", "TW", "TZ", "UA", "UG", "US", "UY", "UZ", "VC", "VE", "VI", "VN", "VU", "WF", "WS", "YE", "YT",
  "ZA", "ZW",
];

/** The countries as pick-list choices, named in `locale` and sorted by that name; the English name
 * and the code find them too. */
export function countryChoices(locale: string): Choice[] {
  const names = (lang: string): Intl.DisplayNames | null => {
    try {
      return new Intl.DisplayNames([lang], { type: "region" });
    } catch {
      // An engine without DisplayNames shows the codes alone.
      return null;
    }
  };
  const local = names(locale);
  // Every app language finds a country, whatever language the app shows (typing "Německo" in the
  // English app too).
  const others = LANGUAGES.map((l) => names(l.code));
  return COUNTRY_CODES.map((code) => {
    const name = local?.of(code) ?? code;
    const keywords = [...others.map((n) => n?.of(code) ?? ""), code].join(" ");
    return { value: code, label: `${name} (${code})`, keywords };
  }).sort((a, b) => a.label.localeCompare(b.label, locale));
}
