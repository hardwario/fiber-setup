/**
 * The backend: Tauri commands and events in the app, a simulated FIBER in a plain browser
 * (`npm run dev`, then http://localhost:1430) and in the app started with `--demo`, so the screens
 * can be shown and checked without hardware.
 */
import { langOf } from "./i18n/index.svelte";
import type {
  AccessStatus,
  AppInfo,
  Done,
  FlashDevice,
  FlashReport,
  Found,
  ImageStatus,
  JobEvent,
  Phase,
  Prepared,
  Release,
  Settings,
  UsbBoard,
} from "./types";

type Unlisten = () => void;

export interface Api {
  simulated: boolean;
  appInfo(): Promise<AppInfo>;
  imageStatus(): Promise<ImageStatus>;
  imagePrepare(file: string | null): Promise<void>;
  pickImage(): Promise<string | null>;
  accessStatus(): Promise<AccessStatus>;
  accessInstall(): Promise<AccessStatus>;
  usbBoards(): Promise<UsbBoard[]>;
  /** Writes the FIBERs of `devices`, all at once (`together`) or one after another. `prompt`: the
   * app's sentence for macOS's password dialog, in the user's language. */
  flashStart(settings: Settings, devices: FlashDevice[], together: boolean, prompt: string): Promise<void>;
  cancel(): Promise<void>;
  findDevice(hostname: string): Promise<Found | null>;
  openLink(url: string): Promise<void>;
  copyText(text: string): Promise<void>;
  onImage(cb: (e: JobEvent) => void, done: (d: Done<Prepared>) => void): Promise<Unlisten>;
  /** What each FIBER of a write does (`cb`) and how it ended (`done`), by its USB port; `finished`
   * when all of them are through. */
  onFlash(
    cb: (port: string, e: JobEvent) => void,
    done: (port: string, d: Done<FlashReport>) => void,
    finished: () => void,
  ): Promise<Unlisten>;
  /** The newer version of FIBER Setup that a check found so far, if any. */
  updateAvailable(): Promise<Release | null>;
  /** Each time a check (after the start, then daily) has its answer. */
  onUpdate(cb: (release: Release | null) => void): Promise<Unlisten>;
  /** Opens the newer version's release page in the browser. */
  openRelease(): Promise<void>;
  /** Simulated FIBER only: the next FIBER ("Set up another FIBER"). */
  nextBoard?(): void;
  /** Simulated FIBER only: the step that is open, so that "RESET" is pressed on the Start step. */
  atStep?(step: string): void;
}

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriApi(): Promise<Api> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const pair = async <T>(event: string, cb: (e: JobEvent) => void, done: (d: Done<T>) => void) => {
    const a = await listen<JobEvent>(`${event}-event`, (e) => cb(e.payload));
    const b = await listen<Done<T>>(`${event}-done`, (e) => done(e.payload));
    return () => {
      a();
      b();
    };
  };
  return {
    simulated: false,
    appInfo: () => invoke("app_info"),
    imageStatus: () => invoke("image_status"),
    imagePrepare: (file) => invoke("image_prepare", { file }),
    pickImage: () => invoke("pick_image"),
    accessStatus: () => invoke("access_status"),
    accessInstall: () => invoke("access_install"),
    usbBoards: () => invoke("usb_boards"),
    flashStart: (settings, devices, together, prompt) => invoke("flash_start", { settings, devices, together, prompt }),
    cancel: () => invoke("cancel"),
    findDevice: (hostname) => invoke("find_device", { hostname }),
    openLink: (url) => invoke("open_link", { url }),
    // The system's clipboard through the app: the page's own is switched off in the Linux web view.
    copyText: async (text) => {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(text);
    },
    onImage: (cb, done) => pair<Prepared>("image", cb, done),
    onFlash: async (cb, done, finished) => {
      const a = await listen<{ port: string; event: JobEvent }>("flash-event", (e) => cb(e.payload.port, e.payload.event));
      const b = await listen<Done<FlashReport> & { port: string }>("flash-done", (e) => done(e.payload.port, e.payload));
      const c = await listen("flash-finished", () => finished());
      return () => {
        a();
        b();
        c();
      };
    },
    updateAvailable: () => invoke("update_available"),
    onUpdate: (cb) => listen<Release | null>("update-available", (e) => cb(e.payload)),
    openRelease: () => invoke("open_release"),
  };
}

/** A FIBER that appears on USB a few seconds after the Connect step opens, stays in boot mode for a
 * few seconds after writing (until "RESET") and is found on the network on the third look.
 * `openLink` and `copyText` are the app's own in demo mode (the browser's in a plain browser).
 * In the address: `?boards=4` for that many FIBERs (up to 9, one more than can be written at
 * once), `?fail=1-2` for a FIBER whose first write fails, `?update=available` for a newer version
 * of FIBER Setup. */
function simulatedApi(openLink?: (url: string) => Promise<void>, copyText?: (text: string) => Promise<void>): Api {
  const image = {
    id: "raspios-lite-arm64-2026-10-06",
    name: "Raspberry Pi OS Lite (64-bit)",
    version: "2026-10-06, Debian 13 (trixie)",
    url: "https://downloads.raspberrypi.com/",
    sha256: "483db18a48da399b5b7022ffae9a07bc6d99daab30e593aedac09b69ff422843",
    size: 550_466_056,
    file_name: "2026-10-06-raspios-trixie-arm64-lite.img.xz",
  };
  const prepared: Prepared = {
    raw: "~/.cache/fiber-setup/483db18a48da399b.img",
    size: 3_078_619_136,
    source_sha256: image.sha256,
    official: true,
    name: `${image.name} (${image.version})`,
  };
  let imageCbs: [(e: JobEvent) => void, (d: Done<Prepared>) => void] | null = null;
  let flashCbs:
    | [(port: string, e: JobEvent) => void, (port: string, d: Done<FlashReport>) => void, () => void]
    | null = null;
  let access = false;
  const query = new URLSearchParams(typeof location === "undefined" ? "" : location.search);
  const count = Math.min(9, Math.max(1, Number(query.get("boards")) || 1));
  const PORTS = count === 1 ? ["3-2"] : ["1-1", "1-2", "1-3", "1-4", "2-1", "2-2", "2-3", "2-4", "3-1"].slice(0, count);
  const serialOf = (port: string) =>
    count === 1 ? "10000000a1b2c3d4" : `10000000a1b2${port.replace(/\D/g, "").padStart(4, "0")}`;
  /** The FIBER whose first write fails, and whether it has failed already. */
  const failing = query.get("fail");
  let failed = false;
  let boardsAsked = 0;
  /** Looks on the Start step since each port was written. */
  const written = new Map<string, number>();
  let step = "";
  /** How often each name was looked for on the network. */
  const searches = new Map<string, number>();
  let cancelled = false;
  const appInfo = async (): Promise<AppInfo> => ({
    version: "1.1.0",
    os: "linux",
    image,
    tools_found: true,
    defaults: {
      hostname: "fiber",
      username: "fiber",
      country: "CZ",
      timezone: "Europe/Prague",
      ssh_key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExampleKey user@laptop",
      ssh_key_path: "~/.ssh/id_ed25519.pub",
      language: langOf(navigator.language),
    },
  });
  /** `?update=available`: the version after this one is out. */
  const update = async (): Promise<Release | null> => {
    if (new URLSearchParams(location.search).get("update") !== "available") return null;
    const [major, minor] = (await appInfo()).version.split(".").map(Number);
    const version = `${major}.${minor + 1}.0`;
    return { version, url: `https://github.com/hardwario/fiber-setup/releases/tag/v${version}` };
  };

  const run = async (
    cb: (e: JobEvent) => void,
    phases: { phase: Phase; total: number; ms: number }[],
  ) => {
    for (const p of phases) {
      cb({ type: "phase", phase: p.phase });
      const steps = 20;
      for (let i = 1; i <= steps; i++) {
        if (cancelled) throw new Error("cancelled");
        await new Promise((r) => setTimeout(r, p.ms / steps));
        cb({ type: "progress", phase: p.phase, done: (p.total * i) / steps, total: p.total });
      }
    }
  };

  return {
    simulated: true,
    appInfo,
    imageStatus: async () => ({ status: { image, ready: false, other_copy: null }, prepared: null }),
    imagePrepare: async () => {
      cancelled = false;
      const [cb, done] = imageCbs ?? [() => {}, () => {}];
      run(cb, [
        { phase: "download", total: image.size, ms: 2500 },
        { phase: "check", total: image.size, ms: 600 },
        { phase: "unpack", total: image.size, ms: 1500 },
      ])
        .then(() => done({ ok: true, result: prepared, error: null }))
        .catch((e) => done({ ok: false, result: null, error: String(e.message ?? e) }));
    },
    pickImage: async () => null,
    accessStatus: async () => ({ rules_installed: access, pkexec: true }),
    accessInstall: async () => {
      await new Promise((r) => setTimeout(r, 800));
      access = true;
      return { rules_installed: true, pkexec: true };
    },
    usbBoards: async () => {
      boardsAsked++;
      return PORTS.flatMap((port, i): UsbBoard[] => {
        const since = written.get(port);
        if (since === undefined) {
          // They come up one after another, as they are plugged in.
          return boardsAsked > 3 + i ? [{ port, kind: "boot", product_id: "2711", serial: null }] : [];
        }
        // Written: its memory stays a disk until "RESET", pressed on the Start step, one FIBER
        // after another.
        if (step === "restart") written.set(port, since + 1);
        return since <= 4 + i ? [{ port, kind: "gadget", product_id: "0104", serial: serialOf(port) }] : [];
      });
    },
    flashStart: async (settings, devices, together) => {
      cancelled = false;
      const [cb, done, finished] = flashCbs ?? [() => {}, () => {}, () => {}];
      const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));
      const nameOf = (d: FlashDevice) =>
        settings.install_fiber ? `fiber-${serialOf(d.port).slice(-8)}` : (d.hostname ?? settings.hostname);
      /** Started over USB, its memory found: as the app does it, for all of them at once. */
      const start = async (d: FlashDevice) => {
        const log = (line: string) => cb(d.port, { type: "log", line });
        cb(d.port, { type: "phase", phase: "boot" });
        for (const line of [`Waiting for the FIBER on USB port ${d.port}`, "Sending bootcode4.bin", "File read: boot.img", "Second stage boot server done"]) {
          await pause(500);
          if (cancelled) throw new Error("cancelled");
          log(line);
        }
        cb(d.port, { type: "phase", phase: "wait_disk" });
        await pause(1200);
        if (d.port === failing && !failed) {
          failed = true;
          throw new Error(`the FIBER's eMMC did not appear on USB port ${d.port} (check the cable, then try again)`);
        }
        log(`eMMC /dev/sd${String.fromCharCode(98 + PORTS.indexOf(d.port))} (7.8 GB) on USB port ${d.port}, CM4 serial ${serialOf(d.port)}`);
        cb(d.port, { type: "phase", phase: "ready" });
      };
      const write = async (d: FlashDevice) => {
        cb(d.port, { type: "phase", phase: "settings" });
        cb(d.port, { type: "log", line: `Settings written for ${nameOf(d)} (instance fiber-setup-1793820007-22517e79)` });
        await run((e) => cb(d.port, e), [
          { phase: "write", total: prepared.size, ms: 5000 },
          { phase: "verify", total: prepared.size, ms: 2500 },
        ]);
        written.set(d.port, 0);
        cb(d.port, { type: "phase", phase: "done" });
        done(d.port, {
          ok: true,
          result: {
            disk: `/dev/sd${String.fromCharCode(98 + PORTS.indexOf(d.port))}`,
            disk_size: 7_818_182_656,
            serial: serialOf(d.port),
            hub_name: `fiber-${serialOf(d.port).slice(-8)}`,
            hostname: nameOf(d),
            written: prepared.size,
            instance_id: "fiber-setup-1793820007-22517e79",
            seconds: 214,
          },
          error: null,
        });
      };
      const guarded = (job: (d: FlashDevice) => Promise<void>) => async (d: FlashDevice) => {
        try {
          await job(d);
          return true;
        } catch (e) {
          done(d.port, { ok: false, result: null, error: String((e as Error).message ?? e) });
          return false;
        }
      };
      (async () => {
        const started = await Promise.all(devices.map(guarded(start)));
        const ready = devices.filter((_, i) => started[i]);
        if (together) await Promise.all(ready.map(guarded(write)));
        else for (const d of ready) await guarded(write)(d);
        finished();
      })();
    },
    cancel: async () => {
      cancelled = true;
    },
    findDevice: async (hostname) => {
      await new Promise((r) => setTimeout(r, 900));
      // Each on its third look, at an address of its own.
      if (!searches.has(hostname)) searches.set(hostname, 0);
      const looks = (searches.get(hostname) ?? 0) + 1;
      searches.set(hostname, looks);
      const index = [...searches.keys()].indexOf(hostname);
      return looks >= 3 + index ? { ip: `192.168.1.${57 + index}`, ssh: true } : null;
    },
    openLink:
      openLink ??
      (async (url) => {
        window.open(url, "_blank");
      }),
    copyText: copyText ?? browserCopy,
    onImage: async (cb, done) => {
      imageCbs = [cb, done];
      return () => (imageCbs = null);
    },
    onFlash: async (cb, done, finished) => {
      flashCbs = [cb, done, finished];
      return () => (flashCbs = null);
    },
    updateAvailable: update,
    onUpdate: async () => () => {},
    openRelease: async () => {
      const release = await update();
      if (release) window.open(release.url, "_blank");
    },
    nextBoard: () => {
      boardsAsked = 0;
      written.clear();
      searches.clear();
      failed = false;
    },
    atStep: (open) => {
      step = open;
    },
  };
}

/** The browser's clipboard, for the preview: the modern way, or the old one where that is refused. */
async function browserCopy(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.append(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    if (!ok) throw new Error("the clipboard is not available");
  }
}

/** In the app: the Tauri backend, or the simulated FIBER when started with `--demo`. */
async function appApi(): Promise<Api> {
  const api = await tauriApi();
  const { invoke } = await import("@tauri-apps/api/core");
  return (await invoke<boolean>("demo_mode")) ? simulatedApi(api.openLink, api.copyText) : api;
}

export const apiReady: Promise<Api> = inTauri ? appApi() : Promise.resolve(simulatedApi());
