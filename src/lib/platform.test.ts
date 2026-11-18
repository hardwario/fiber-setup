import { describe, expect, it } from "vitest";
import copy from "../../image/components.yaml?raw";
import { SIMULATED_OFFER } from "./api";
import { cs } from "./i18n/cs";
import { de } from "./i18n/de";
import { en } from "./i18n/en";
import { pl } from "./i18n/pl";
import { sk } from "./i18n/sk";
import { LINKS, links, neededBy, stage, toggle, withRequirements } from "./platform";
import type { Found } from "./types";

const all = SIMULATED_OFFER.components;

/** The components of the copy of `components.yaml`, in its order, with the ports each opens to
 * the network. Read by its lines: two spaces and an id open a component. */
function fromCopy(): { id: string; network: number[] }[] {
  const out: { id: string; network: number[] }[] = [];
  for (const line of copy.split("\n")) {
    const id = line.match(/^ {2}([a-z][a-z0-9-]*):\s*$/);
    if (id) out.push({ id: id[1], network: [] });
    const port = line.match(/port: (\d+), listen: network/);
    if (port && out.length) out[out.length - 1].network.push(Number(port[1]));
  }
  return out;
}

const found = (setup: Record<string, string> | null, platform: Record<string, string> | null): Found => ({
  ip: "192.168.1.57",
  ssh: true,
  setup,
  platform,
});

describe("the components", () => {
  it("of the simulated FIBER are those of the copy", () => {
    expect(all.map((c) => c.id)).toEqual(fromCopy().map((c) => c.id));
  });

  it("have their name and sentence in every language", () => {
    for (const dict of [en, cs, de, sk, pl]) {
      for (const { id } of fromCopy()) {
        const text = (dict.components as Record<string, { title: string; about: string } | undefined>)[id];
        expect(text?.title, id).toBeTruthy();
        expect(text?.about, id).toBeTruthy();
      }
    }
  });

  it("that listen on the network have a link for each such port", () => {
    for (const { id, network } of fromCopy()) {
      expect((LINKS[id] ?? []).map((p) => p.port).sort(), id).toEqual([...network].sort());
    }
  });

  it("come with what they require, and stay while a ticked one needs them", () => {
    expect(withRequirements(["agent"], all)).toEqual(["base", "mosquitto", "agent"]);
    let ticked = toggle("node-red", true, [], all);
    expect(ticked).toEqual(["mosquitto", "node-red"]);
    expect(neededBy("mosquitto", ticked, all)).toEqual(["node-red"]);
    // Mosquitto cannot go while Node-RED needs it; Node-RED can, and then Mosquitto can.
    expect(toggle("mosquitto", false, ticked, all)).toEqual(ticked);
    ticked = toggle("node-red", false, ticked, all);
    expect(toggle("mosquitto", false, ticked, all)).toEqual([]);
  });

  it("give the links of what was chosen", () => {
    const host = "fiber-a1b2c3d4.local";
    expect(links(["base", "mosquitto", "agent"], false, host)).toEqual([]);
    expect(links(["mosquitto", "node-red", "chirpstack"], true, host).map((l) => [l.label, l.href, l.user])).toEqual([
      ["mqtt", "mqtt://fiber-a1b2c3d4.local:1884", "login"],
      ["nodeRed", "http://fiber-a1b2c3d4.local:1880/", "login"],
      ["dashboard", "http://fiber-a1b2c3d4.local:1880/dashboard", "login"],
      ["chirpstack", "http://fiber-a1b2c3d4.local:8080/", "admin"],
    ]);
  });
});

describe("the first start", () => {
  it("is read from the script's record, then the platform's", () => {
    expect(stage(found({ state: "network", release: "0.1.0" }, null), null)).toEqual({
      kind: "network",
      release: "0.1.0",
    });
    expect(stage(found({ state: "install", release: "0.1.0" }, null), null)).toEqual({
      kind: "install",
      release: "0.1.0",
      component: null,
      step: 0,
      steps: 0,
    });
    expect(
      stage(
        found(
          { state: "install", release: "0.1.0" },
          { state: "running", step: "3/5", component: "agent", release: "0.1.0" },
        ),
        null,
      ),
    ).toEqual({ kind: "install", release: "0.1.0", component: "agent", step: 3, steps: 5 });
    expect(stage(found({ state: "failed", release: "0.1.0", error: "download" }, null), null)).toEqual({
      kind: "failed",
      error: "download",
    });
    expect(stage(found({ state: "restart", release: "0.1.0" }, { state: "done", step: "5/5" }), null).kind).toBe(
      "restart",
    );
    // The script's record is gone once it is done; the platform's says how it ended.
    expect(stage(found(null, { state: "done", step: "5/5", component: "" }), null).kind).toBe("ready");
    expect(stage(found(null, { state: "partly", step: "5/5" }), null).kind).toBe("partly");
    expect(stage(found(null, { state: "failed", step: "0/0" }), null).kind).toBe("applyFailed");
  });

  it("keeps what it had while Avahi answers nothing", () => {
    const before = { kind: "download", release: "0.1.0" } as const;
    expect(stage(found(null, null), before)).toBe(before);
    expect(stage(found(null, null), null)).toEqual({ kind: "starting" });
  });
});
