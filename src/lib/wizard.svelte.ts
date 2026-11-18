/** The wizard: which step is open, what each step found, and the actions behind its buttons. */
import { apiReady, type Api } from "./api";
import { COUNTRY_CODES } from "./countries";
import { t } from "./i18n/index.svelte";
import { stage, toggle, type Stage } from "./platform";
import { prefs } from "./prefs.svelte";
import type {
  AccessStatus,
  AppInfo,
  FlashDevice,
  FlashReport,
  Found,
  JobEvent,
  MiniPcie,
  Phase,
  Prepared,
  Release,
  Settings,
  UsbBoard,
} from "./types";

export const STEPS = ["welcome", "image", "settings", "jumper", "connect", "write", "restart", "start", "done"] as const;
export type StepId = (typeof STEPS)[number];
/** The steps in the three parts the user goes through: getting ready at the computer, writing at
 * the FIBER, and its start. The step list shows them as groups. */
export const PHASES: { id: "prepare" | "write" | "start"; steps: StepId[] }[] = [
  { id: "prepare", steps: ["welcome", "image", "settings"] },
  { id: "write", steps: ["jumper", "connect", "write"] },
  { id: "start", steps: ["restart", "start", "done"] },
];
/** The glyph each step goes by, in the step list and at the head of its page. */
export const STEP_ICONS: Record<StepId, string> = {
  welcome: "list-checks",
  image: "package",
  settings: "sliders",
  jumper: "toggle-left",
  connect: "usb",
  write: "drive",
  restart: "power",
  start: "timer",
  done: "circle-check",
};
/** What happens with Wi-Fi: set now, set later from the phone app, or not used. The Ethernet
 * cable is no choice: it always works (DHCP), whatever is chosen here. */
export type Network = "wifi" | "phone" | "none";
/** Whether "Wi-Fi later from my phone" is offered. It needs the platform's Bluetooth, which
 * comes with its task T29; until then the choice is hidden. */
export const PHONE_SETUP = false;

/** How many FIBERs can be written at once (the app's backend holds the same limit). */
export const MAX_DEVICES = 8;

export interface Job<T> {
  busy: boolean;
  phase: Phase | null;
  /** When this phase began (ms), to tell when one takes unusually long. */
  since: number;
  done: number;
  total: number;
  result: T | null;
  error: string | null;
  log: string[];
}

function job<T>(): Job<T> {
  return { busy: false, phase: null, since: Date.now(), done: 0, total: 0, result: null, error: null, log: [] };
}

function apply<T>(j: Job<T>, e: JobEvent) {
  if (e.type === "phase") {
    j.phase = e.phase;
    j.since = Date.now();
    j.done = 0;
    j.total = 0;
  } else if (e.type === "progress") {
    j.phase = e.phase;
    j.done = e.done;
    j.total = e.total;
  } else {
    j.log = [...j.log.slice(-199), e.line];
  }
}

const HOST = /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/;
const USER = /^[a-z][a-z0-9_-]{0,31}$/;
/** Accounts Raspberry Pi OS already has (as in firstboot.rs). */
const SYSTEM_USERS = ["root", "daemon", "bin", "sys", "sync", "games", "man", "lp", "mail", "news", "uucp", "proxy",
  "www-data", "backup", "list", "irc", "nobody", "messagebus", "sshd", "avahi", "polkitd", "systemd-network"];
const TIMEZONE = /^[A-Za-z0-9/_+-]+$/;
const utf8 = (s: string) => new TextEncoder().encode(s).length;
/** The newer version whose notice the user closed; the one after it shows again. */
const DISMISSED_KEY = "fiber-setup.update.dismissed";

function readDismissed(): string | null {
  try {
    return localStorage.getItem(DISMISSED_KEY);
  } catch {
    return null;
  }
}

class Wizard {
  api: Api | null = null;
  /** The browser preview, without the app's backend. */
  simulated = $state(false);
  step = $state<StepId>("welcome");
  reached = $state(0);
  info = $state.raw<AppInfo | null>(null);
  failed = $state<string | null>(null);

  image = $state<Job<Prepared> & { otherCopy: string | null }>({ ...job<Prepared>(), otherCopy: null });
  form = $state({
    hostname: "fiber",
    username: "fiber",
    // Pre-filled, like the user name: a FIBER can be set up without typing anything. The page
    // says so, and the last page shows both again.
    password: "fiber",
    password2: "fiber",
    // The Ethernet cable always works; Wi-Fi is what the user adds.
    network: "none" as Network,
    ssid: "",
    wifiPassword: "",
    hidden: false,
    country: "CZ",
    timezone: "",
    useSshKey: true,
    /** The ticked components of the FIBER platform, with what they require; filled from the
     * release's default when the app starts. None: a plain Raspberry Pi OS. */
    components: [] as string[],
    mqttLan: false,
    minipcie: "none" as MiniPcie,
  });
  /** A field shows its error only after the user left it or pressed Next. */
  touched = $state<Record<string, boolean>>({});

  /** The FIBERs on USB now, in boot mode or with their memory connected. */
  boards = $state.raw<UsbBoard[]>([]);
  access = $state.raw<AccessStatus | null>(null);
  accessBusy = $state(false);
  accessError = $state<string | null>(null);
  /** USB ports of FIBERs the user unticked: found, but not to be written. */
  skipped = $state<string[]>([]);
  /** Several FIBERs are written all at once, or one after another. */
  together = $state(true);

  /** The write of each FIBER, by its USB port: what it is doing, and how it ended. */
  jobs = $state<Record<string, Job<FlashReport>>>({});
  /** From the press of Write until the last FIBER of it is through. */
  batch = $state(false);
  /** What the writes put on the FIBERs, for the last steps (the form may change after). */
  written = $state.raw<{
    username: string;
    password: string;
    network: Network;
    ssid: string;
    components: string[];
    mqttLan: boolean;
  } | null>(null);
  /** Where each written FIBER was found on the network, by its name. */
  found = $state.raw<Record<string, Found>>({});
  /** How far each written FIBER's first start is, by its name. */
  stages = $state.raw<Record<string, Stage>>({});
  /** A stage that ends the installation is shown once it was read twice in a row: while Avahi
   * registers a changed record it answers nothing for a moment, which can look like the end. */
  private ending: Record<string, Stage> = {};
  searchSince = $state(0);
  /** A newer version of FIBER Setup, when there is one. */
  update = $state.raw<Release | null>(null);
  private dismissed = $state(readDismissed());

  get index(): number {
    return STEPS.indexOf(this.step);
  }

  async init() {
    try {
      this.api = await apiReady;
      this.simulated = this.api.simulated;
      const info = await this.api.appInfo();
      this.info = info;
      prefs.systemLang = info.defaults.language;
      prefs.apply();
      this.form.hostname = info.defaults.hostname;
      this.form.username = info.defaults.username;
      // A region the list does not have would leave the country menu blank.
      this.form.country = COUNTRY_CODES.includes(info.defaults.country) ? info.defaults.country : "CZ";
      this.form.timezone = info.defaults.timezone ?? "";
      this.form.useSshKey = !!info.defaults.ssh_key;
      this.form.components = info.platform?.default ?? [];
      await this.api.onImage(
        (e) => apply(this.image, e),
        (d) => {
          this.image.busy = false;
          this.image.result = d.result;
          this.image.error = d.error;
        },
      );
      await this.api.onFlash(
        (port, e) => apply((this.jobs[port] ??= job<FlashReport>()), e),
        (port, d) => {
          const j = (this.jobs[port] ??= job<FlashReport>());
          j.busy = false;
          j.result = d.result;
          j.error = d.error;
        },
        () => (this.batch = false),
      );
      await this.api.onUpdate((release) => (this.update = release));
      // What a check found before the window was listening.
      this.update = (await this.api.updateAvailable()) ?? this.update;
      const s = await this.api.imageStatus();
      this.image.otherCopy = s.status.other_copy;
      if (s.prepared) this.image.result = s.prepared;
      else if (s.status.ready) this.prepareImage(null);
    } catch (e) {
      this.failed = String(e);
    }
  }

  go(step: StepId) {
    const i = STEPS.indexOf(step);
    if (i > this.reached) return;
    this.step = step;
  }

  next() {
    if (!this.canNext) {
      this.touched = Object.fromEntries(Object.keys(this.errors).map((k) => [k, true]));
      return;
    }
    const i = this.index + 1;
    if (i >= STEPS.length) return;
    this.step = STEPS[i];
    this.reached = Math.max(this.reached, i);
    this.onEnter();
  }

  back() {
    if (this.index > 0 && !this.writing) this.step = STEPS[this.index - 1];
  }

  /** Work a step starts by itself when it opens. */
  private onEnter() {
    if (this.step === "image" && !this.image.result && !this.image.busy) this.prepareImage(null);
    if (this.step === "start") this.searchSince = Date.now();
  }

  get canNext(): boolean {
    switch (this.step) {
      case "image":
        return !!this.image.result && !this.image.busy;
      case "settings":
        return Object.keys(this.errors).length === 0;
      case "connect":
        return this.targets.length > 0 && !!this.access?.rules_installed;
      case "write":
        return this.done.length > 0 && !this.writing;
      default:
        return true;
    }
  }

  /** Whether the chosen settings install the FIBER platform. */
  get installFiber(): boolean {
    return this.form.components.length > 0;
  }

  /** Ticks or unticks a component; what it requires comes with it. */
  tick(id: string, on: boolean) {
    this.form.components = toggle(id, on, this.form.components, this.info?.platform?.components ?? []);
  }

  /** Mirrors `Settings::validate` in Rust, which has the last word. */
  get errors(): Record<string, string> {
    const f = this.form;
    const e: Record<string, string> = {};
    // With the FIBER platform the name comes from the serial number; the field is not shown.
    if (!this.installFiber && !HOST.test(f.hostname)) e.hostname = "hostname";
    if (!USER.test(f.username) || SYSTEM_USERS.includes(f.username)) e.username = "user";
    if (!f.password) e.password = "password";
    else if (f.password !== f.password2) e.password2 = "passwordAgain";
    if (f.network === "wifi") {
      if (!f.ssid || utf8(f.ssid) > 32) e.ssid = "ssid";
      const p = f.wifiPassword;
      const hex64 = /^[0-9a-fA-F]{64}$/.test(p);
      if (p && !hex64 && (utf8(p) < 8 || utf8(p) > 63)) e.wifiPassword = "wifiPassword";
    }
    if (!COUNTRY_CODES.includes(f.country)) e.country = "country";
    if (f.timezone.trim() && !TIMEZONE.test(f.timezone.trim())) e.timezone = "timezone";
    if (f.network === "phone" && !this.installFiber) e.network = "phone";
    return e;
  }

  settings(): Settings {
    const f = this.form;
    return {
      // Only used with the FIBER platform when the serial number is unknown.
      hostname: this.installFiber && !HOST.test(f.hostname) ? "fiber" : f.hostname,
      username: f.username,
      password: f.password,
      ssh_key: f.useSshKey ? (this.info?.defaults.ssh_key ?? null) : null,
      wifi: f.network === "wifi" ? { ssid: f.ssid, password: f.wifiPassword, hidden: f.hidden } : null,
      country: f.country,
      timezone: f.timezone.trim() || null,
      components: f.components,
      // The network listener is Mosquitto's: without it there is nothing to open.
      mqtt_lan: f.mqttLan && f.components.includes("mosquitto"),
      minipcie: this.installFiber ? f.minipcie : "none",
    };
  }

  prepareImage(file: string | null) {
    if (!this.api || this.image.busy) return;
    Object.assign(this.image, { ...job<Prepared>(), busy: true, otherCopy: this.image.otherCopy });
    this.api.imagePrepare(file).catch((e) => {
      this.image.busy = false;
      this.image.error = String(e);
    });
  }

  async pickImage() {
    const file = await this.api?.pickImage();
    if (file) this.prepareImage(file);
  }

  /** Calls `look` now and then again `every` ms after each look has finished, until the returned
   * function is called: a slow look never has a second one started behind it. */
  watchUsb(every: number, looked: () => void = () => {}): () => void {
    let stop = false;
    const loop = async () => {
      while (!stop) {
        try {
          await this.refreshUsb();
          looked();
        } catch {
          // A look that failed is tried again like any other.
        }
        await new Promise((r) => setTimeout(r, every));
      }
    };
    loop();
    return () => (stop = true);
  }

  async refreshUsb() {
    if (!this.api) return;
    this.api.atStep?.(this.step);
    const [boards, access] = await Promise.all([this.api.usbBoards(), this.api.accessStatus()]);
    this.boards = boards;
    this.access = access;
  }

  /** The USB ports the Write button writes: the FIBERs on USB that are not written yet and that
   * the user did not untick (one FIBER alone is always the one), up to the limit. A failed one
   * is among them again, and so is one connected since, or one that was over the limit before. */
  get toWrite(): string[] {
    const alone = this.boards.length === 1;
    return this.boards
      .filter((b) => !this.jobs[b.port]?.result && (alone || !this.skipped.includes(b.port)))
      .slice(0, MAX_DEVICES)
      .map((b) => b.port);
  }

  /** The chosen FIBERs: those to write, and those on USB that are written already. */
  get targets(): UsbBoard[] {
    const next = this.toWrite;
    return this.boards.filter((b) => next.includes(b.port) || !!this.jobs[b.port]?.result);
  }

  /** Whether a FIBER on USB is among those to write. */
  chosen(port: string): boolean {
    return this.targets.some((b) => b.port === port);
  }

  /** A FIBER that is not unticked and still is not written: one more than fit. */
  overLimit(port: string): boolean {
    return !this.skipped.includes(port) && !this.chosen(port);
  }

  /** Ticks or unticks a FIBER in the list. */
  choose(port: string, write: boolean) {
    this.skipped = write ? this.skipped.filter((p) => p !== port) : [...new Set([...this.skipped, port])];
  }

  /** A write is going on. */
  get writing(): boolean {
    return this.batch || Object.values(this.jobs).some((j) => j.busy);
  }

  /** The FIBERs that are written and read back, in the order they were started. */
  get done(): { port: string; report: FlashReport }[] {
    return Object.entries(this.jobs).flatMap(([port, j]) => (j.result ? [{ port, report: j.result }] : []));
  }

  /** The ports the Write step shows a line for: what was written or tried, then what is to come. */
  get ports(): string[] {
    return [...new Set([...Object.keys(this.jobs), ...this.toWrite])];
  }

  /** The name the `n`th FIBER (from 0) gets without the FIBER software: the one from the settings,
   * then the next numbers (fiber, fiber-2, fiber-3; bench-7, bench-8). Two FIBERs with one name
   * would both be <name>.local. */
  nameAt(n: number): string {
    const m = this.form.hostname.match(/^(.*?)(?:-(\d+))?$/);
    if (!m) return this.form.hostname;
    const number = (m[2] ? Number(m[2]) : 1) + n;
    return (number === 1 ? m[1] : `${m[1]}-${number}`).slice(0, 63);
  }

  async allowAccess() {
    if (!this.api) return;
    this.accessBusy = true;
    this.accessError = null;
    try {
      this.access = await this.api.accessInstall();
    } catch (e) {
      this.accessError = String(e);
    } finally {
      this.accessBusy = false;
    }
  }

  /** Writes the chosen FIBERs that are not written yet. */
  startFlash() {
    const ports = this.toWrite;
    if (!this.api || ports.length === 0 || this.writing) return;
    const fail = (error: string) => {
      for (const port of ports) this.jobs[port] = { ...job<FlashReport>(), error };
    };
    // The sidebar can reach this step past a half-edited form; never write unchecked settings.
    if (Object.keys(this.errors).length) return fail(t().write.checkSettings);
    const f = this.form;
    this.written = {
      username: f.username,
      password: f.password,
      network: f.network,
      ssid: f.ssid,
      components: f.components,
      mqttLan: f.mqttLan && f.components.includes("mosquitto"),
    };
    // With the FIBER platform each FIBER is named after its serial number; without it, each
    // gets the next name after those already written.
    const already = this.done.length;
    const devices: FlashDevice[] = ports.map((port, i) => ({
      port,
      hostname: this.installFiber ? null : this.nameAt(already + i),
    }));
    for (const port of ports) this.jobs[port] = { ...job<FlashReport>(), busy: true };
    this.batch = true;
    this.api.flashStart(this.settings(), devices, this.together || devices.length === 1, t().write.macPrompt).catch((e) => {
      this.batch = false;
      fail(String(e));
    });
  }

  cancel() {
    this.api?.cancel();
  }

  /** The newer version to tell about on the Welcome page: not one whose notice was closed. */
  get updateNotice(): Release | null {
    return this.update && this.update.version !== this.dismissed ? this.update : null;
  }

  dismissUpdate() {
    if (!this.update) return;
    this.dismissed = this.update.version;
    try {
      localStorage.setItem(DISMISSED_KEY, this.update.version);
    } catch {
      // Closed for this session only.
    }
  }

  openRelease() {
    this.api?.openRelease().catch(() => {});
  }

  /** Puts a text (the login command) on the clipboard. */
  async copy(text: string) {
    await this.api?.copyText(text);
  }

  /** The name each written FIBER has on the network from its first start: fiber-<serial> with the
   * FIBER platform, else the one it was given. (Before anything is written: the one from the
   * settings.) */
  get names(): string[] {
    const names = this.done.map((d) => d.report.hostname);
    return names.length > 0 ? names : [this.form.hostname];
  }

  /** Looks for every written FIBER under its name, all at the same time; with the FIBER
   * platform also how far its first start is. */
  async search() {
    if (!this.api) return;
    const api = this.api;
    const progress = (this.written?.components ?? this.form.components).length > 0;
    const looked = await Promise.all(this.names.map(async (name) => [name, await api.findDevice(name, progress)] as const));
    this.found = Object.fromEntries(looked.flatMap(([name, found]) => (found ? [[name, found]] : [])));
    if (!progress) return;
    const stages = { ...this.stages };
    for (const [name, found] of looked) {
      if (!found) continue;
      const before = stages[name] ?? null;
      const next = stage(found, before);
      const ends = next.kind === "ready" || next.kind === "partly" || next.kind === "applyFailed";
      const same = (a: Stage | undefined) => !!a && JSON.stringify(a) === JSON.stringify(next);
      if (ends && before && !same(before) && !same(this.ending[name])) {
        this.ending[name] = next;
        continue;
      }
      delete this.ending[name];
      stages[name] = next;
    }
    this.stages = stages;
  }

  /** More FIBERs with the same settings: back to the jumper, the passwords kept for this session. */
  another() {
    // Without the FIBER platform the names go on where they stopped (fiber, fiber-2, ...).
    if (!this.installFiber) this.form.hostname = this.nameAt(Math.max(1, this.done.length));
    this.written = null;
    this.api?.nextBoard?.();
    this.jobs = {};
    this.batch = false;
    this.found = {};
    this.stages = {};
    this.ending = {};
    this.skipped = [];
    this.reached = STEPS.indexOf("jumper");
    this.step = "jumper";
  }
}

export const wizard = new Wizard();
