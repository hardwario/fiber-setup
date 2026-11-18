/** The FIBER platform in the app: which components come with which, where a chosen one can be
 * reached, and how far a FIBER's first start is, from its two Avahi records (`check.rs`). */
import type { Component, Found } from "./types";

/** The chosen components and everything they require, in the order of the release. */
export function withRequirements(ids: string[], all: Component[]): string[] {
  const wanted = new Set(ids);
  for (let grew = true; grew; ) {
    grew = false;
    for (const c of all) {
      if (!wanted.has(c.id)) continue;
      for (const r of c.requires) {
        if (!wanted.has(r)) {
          wanted.add(r);
          grew = true;
        }
      }
    }
  }
  return all.filter((c) => wanted.has(c.id)).map((c) => c.id);
}

/** The ticked components that need `id`, directly or through another. */
export function neededBy(id: string, ticked: string[], all: Component[]): string[] {
  return ticked.filter((other) => other !== id && withRequirements([other], all).includes(id));
}

/** Ticks a component with what it requires, or unticks it; one that a ticked component needs
 * stays (the picker does not let it go). */
export function toggle(id: string, on: boolean, ticked: string[], all: Component[]): string[] {
  if (on) return withRequirements([...ticked, id], all);
  if (neededBy(id, ticked, all).length > 0) return ticked;
  return ticked.filter((t) => t !== id);
}

/** What a page of a component is: its name on the Done step, and whether it asks for the
 * login's user name or ChirpStack's own (`admin`, the platform's T9). */
export interface Link {
  label: "nodeRed" | "dashboard" | "chirpstack" | "mqtt";
  url: (host: string) => string;
  /** The setting it needs, as the port in `components.yaml` names it. */
  when?: "mqtt.lan";
  user: "login" | "admin";
}

/** Where each component that listens on the network is reached. A test checks that every port
 * the copy of `components.yaml` opens to the network has a link here. */
export const LINKS: Record<string, { port: number; links: Link[] }[]> = {
  "node-red": [
    {
      port: 1880,
      links: [
        { label: "nodeRed", url: (h) => `http://${h}:1880/`, user: "login" },
        { label: "dashboard", url: (h) => `http://${h}:1880/dashboard`, user: "login" },
      ],
    },
  ],
  chirpstack: [{ port: 8080, links: [{ label: "chirpstack", url: (h) => `http://${h}:8080/`, user: "admin" }] }],
  mosquitto: [{ port: 1884, links: [{ label: "mqtt", url: (h) => `mqtt://${h}:1884`, when: "mqtt.lan", user: "login" }] }],
};

/** The pages of the chosen components, for a FIBER called `host`. */
export function links(components: string[], mqttLan: boolean, host: string): (Link & { href: string })[] {
  return components.flatMap((id) =>
    (LINKS[id] ?? []).flatMap((p) =>
      p.links.filter((l) => l.when !== "mqtt.lan" || mqttLan).map((l) => ({ ...l, href: l.url(host) })),
    ),
  );
}

/** How far a FIBER's first start is. */
export type Stage =
  /** Found, and no record yet: the script starts in a moment. */
  | { kind: "starting" }
  /** What the first-start script does. */
  | { kind: "network" | "clock" | "download" | "restart"; release: string }
  /** The script runs the installer; with the platform's step once its apply has begun. */
  | { kind: "install"; release: string; component: string | null; step: number; steps: number }
  /** A step failed; the script tries again by itself. */
  | { kind: "failed"; error: string }
  /** Done: every component is up, or not every one yet (the platform tries again), or the apply
   * could not run at all. */
  | { kind: "ready" | "partly" | "applyFailed" };

/** Reads the two records. While Avahi registers a changed record it answers nothing for a moment,
 * so a FIBER that has neither keeps the stage it had (`before`). */
export function stage(found: Found, before: Stage | null): Stage {
  const setup = found.setup;
  const platform = found.platform;
  const release = setup?.release ?? platform?.release ?? "";
  const [step, steps] = (platform?.step ?? "0/0").split("/").map((n) => Number(n) || 0);
  const running = platform?.state === "running";
  if (setup) {
    switch (setup.state) {
      case "network":
      case "clock":
      case "download":
      case "restart":
        return { kind: setup.state, release };
      case "failed":
        return { kind: "failed", error: setup.error ?? "" };
      default:
        return {
          kind: "install",
          release,
          component: running ? platform?.component || null : null,
          step: running ? step : 0,
          steps: running ? steps : 0,
        };
    }
  }
  switch (platform?.state) {
    case "running":
      return { kind: "install", release, component: platform.component || null, step, steps };
    case "done":
      return { kind: "ready" };
    case "partly":
      return { kind: "partly" };
    case "failed":
      return { kind: "applyFailed" };
    default:
      return before ?? { kind: "starting" };
  }
}
