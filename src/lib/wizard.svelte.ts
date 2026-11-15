/** The wizard: which step is open, what each step found, and the actions behind its buttons. */
import { apiReady, type Api } from "./api";
import { COUNTRY_CODES } from "./countries";
import { t } from "./i18n/index.svelte";
import { prefs } from "./prefs.svelte";
import type { AccessStatus, AppInfo, FlashReport, Found, JobEvent, Phase, Prepared, Settings, UsbBoard } from "./types";

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
export type Network = "wifi" | "ethernet" | "phone";

interface Job<T> {
  busy: boolean;
  phase: Phase | null;
  done: number;
  total: number;
  result: T | null;
  error: string | null;
  log: string[];
}

function job<T>(): Job<T> {
  return { busy: false, phase: null, done: 0, total: 0, result: null, error: null, log: [] };
}

function apply<T>(j: Job<T>, e: JobEvent) {
  if (e.type === "phase") {
    j.phase = e.phase;
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
    password: "",
    password2: "",
    network: "wifi" as Network,
    ssid: "",
    wifiPassword: "",
    hidden: false,
    country: "CZ",
    timezone: "",
    useSshKey: true,
    installFiber: true,
  });
  /** A field shows its error only after the user left it or pressed Next. */
  touched = $state<Record<string, boolean>>({});

  boards = $state.raw<UsbBoard[]>([]);
  access = $state.raw<AccessStatus | null>(null);
  accessBusy = $state(false);
  accessError = $state<string | null>(null);
  port = $state<string | null>(null);

  flash = $state<Job<FlashReport>>(job<FlashReport>());
  /** What the last write put on the FIBER, for the last steps (the form may change after). */
  written = $state.raw<{ hostname: string; username: string; network: Network; ssid: string; installFiber: boolean } | null>(null);
  found = $state.raw<(Found & { name: string }) | null>(null);
  searchSince = $state(0);

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
      await this.api.onImage(
        (e) => apply(this.image, e),
        (d) => {
          this.image.busy = false;
          this.image.result = d.result;
          this.image.error = d.error;
        },
      );
      await this.api.onFlash(
        (e) => apply(this.flash, e),
        (d) => {
          this.flash.busy = false;
          this.flash.result = d.result;
          this.flash.error = d.error;
          // The name it was written with (fiber-<serial> with the FIBER software).
          if (d.result && this.written) this.written = { ...this.written, hostname: d.result.hostname };
        },
      );
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
    if (this.index > 0 && !this.flash.busy) this.step = STEPS[this.index - 1];
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
        return !!this.port && !!this.access?.rules_installed;
      case "write":
        return !!this.flash.result && !this.flash.busy;
      default:
        return true;
    }
  }

  /** Mirrors `Settings::validate` in Rust, which has the last word. */
  get errors(): Record<string, string> {
    const f = this.form;
    const e: Record<string, string> = {};
    // With the FIBER software the name comes from the serial number; the field is not shown.
    if (!f.installFiber && !HOST.test(f.hostname)) e.hostname = "hostname";
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
    if (f.network === "phone" && !f.installFiber) e.network = "phone";
    return e;
  }

  settings(): Settings {
    const f = this.form;
    return {
      // Only used with the FIBER software when the serial number is unknown.
      hostname: f.installFiber && !HOST.test(f.hostname) ? "fiber" : f.hostname,
      username: f.username,
      password: f.password,
      ssh_key: f.useSshKey ? (this.info?.defaults.ssh_key ?? null) : null,
      wifi: f.network === "wifi" ? { ssid: f.ssid, password: f.wifiPassword, hidden: f.hidden } : null,
      country: f.country,
      timezone: f.timezone.trim() || null,
      install_fiber: f.installFiber,
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

  async refreshUsb() {
    if (!this.api) return;
    const [boards, access] = await Promise.all([this.api.usbBoards(), this.api.accessStatus()]);
    this.boards = boards;
    this.access = access;
    // One board: that one. Several: the one the user picked, while it is still there.
    if (boards.length === 1) this.port = boards[0].port;
    else if (!boards.some((b) => b.port === this.port)) this.port = null;
  }

  choosePort(port: string) {
    this.port = port;
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

  startFlash() {
    if (!this.api || !this.port || this.flash.busy) return;
    // The sidebar can reach this step past a half-edited form; never write unchecked settings.
    if (Object.keys(this.errors).length) {
      Object.assign(this.flash, { ...job<FlashReport>(), error: t().write.checkSettings });
      return;
    }
    const f = this.form;
    this.written = { hostname: f.hostname, username: f.username, network: f.network, ssid: f.ssid, installFiber: f.installFiber };
    Object.assign(this.flash, { ...job<FlashReport>(), busy: true });
    this.api.flashStart(this.settings(), this.port).catch((e) => {
      this.flash.busy = false;
      this.flash.error = String(e);
    });
  }

  cancel() {
    this.api?.cancel();
  }

  /** Puts a text (the login command) on the clipboard. */
  async copy(text: string) {
    await this.api?.copyText(text);
  }

  /** The name the FIBER software gives the FIBER, fiber-<serial> (from the write report), which it
   * has from the first start; null without the FIBER software. */
  get hubName(): string | null {
    return this.written?.installFiber ? (this.flash.result?.hub_name ?? null) : null;
  }

  /** Looks for the FIBER under its name: fiber-<serial> with the FIBER software (from the first
   * start), else the one from the settings. */
  async search() {
    if (!this.api) return;
    const names = [...new Set([this.hubName, this.written?.hostname ?? this.form.hostname])].filter(
      (n): n is string => !!n,
    );
    for (const name of names) {
      const found = await this.api.findDevice(name);
      if (found) {
        this.found = { ...found, name };
        return;
      }
    }
    this.found = null;
  }

  /** Another FIBER with the same settings: back to the jumper, the passwords kept for this session. */
  another() {
    // Two FIBERs with one name would both be <name>.local; suggest the next one (fiber, fiber-2, ...).
    // With the FIBER software each is named after its own serial number.
    const m = this.form.hostname.match(/^(.*?)(?:-(\d+))?$/);
    if (m && !this.form.installFiber) this.form.hostname = `${m[1]}-${m[2] ? Number(m[2]) + 1 : 2}`.slice(0, 63);
    this.written = null;
    this.api?.nextBoard?.();
    this.flash = job<FlashReport>();
    this.found = null;
    this.port = null;
    this.reached = STEPS.indexOf("jumper");
    this.step = "jumper";
  }
}

export const wizard = new Wizard();
