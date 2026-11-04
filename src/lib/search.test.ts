import { describe, expect, it } from "vitest";
import { countryChoices } from "./countries";
import { filterChoices } from "./search";
import { timezoneChoices } from "./timezones";

describe("search", () => {
  it("finds countries by any name, without accents", () => {
    const cs = countryChoices("cs");
    expect(filterChoices(cs, "cesko")[0].value).toBe("CZ");
    expect(filterChoices(cs, "czech")[0].value).toBe("CZ");
    expect(filterChoices(cs, "SK").map((c) => c.value)).toContain("SK");
    // Czech names work in the English app too.
    expect(filterChoices(countryChoices("en"), "nemecko")[0].value).toBe("DE");
  });

  it("finds time zones by city, Czech name and offset", () => {
    const tz = timezoneChoices("cs");
    expect(filterChoices(tz, "prague")[0].value).toBe("Europe/Prague");
    expect(filterChoices(tz, "praha")[0].value).toBe("Europe/Prague");
    expect(filterChoices(tz, "new york")[0].value).toBe("America/New_York");
  });

  it("shows everything for an empty query", () => {
    const cs = countryChoices("en");
    expect(filterChoices(cs, "  ")).toHaveLength(cs.length);
  });
});
