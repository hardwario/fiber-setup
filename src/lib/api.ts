/**
 * The backend: Tauri commands and events in the app, a simulated FIBER in a plain browser
 * (`npm run dev`, then http://localhost:1430) and in the app started with `--demo`, so the screens
 * can be shown and checked without hardware.
 */
import type {
  AccessStatus,
  AppInfo,
  Done,
  FlashReport,
  Found,
  ImageStatus,
  JobEvent,
  Phase,
  Prepared,
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
  flashStart(settings: Settings, port: string): Promise<void>;
  cancel(): Promise<void>;
  findDevice(hostname: string): Promise<Found | null>;
  openLink(url: string): Promise<void>;
  copyText(text: string): Promise<void>;
  onImage(cb: (e: JobEvent) => void, done: (d: Done<Prepared>) => void): Promise<Unlisten>;
  onFlash(cb: (e: JobEvent) => void, done: (d: Done<FlashReport>) => void): Promise<Unlisten>;
  /** Simulated FIBER only: the next FIBER ("Set up another FIBER"). */
  nextBoard?(): void;
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
    flashStart: (settings, port) => invoke("flash_start", { settings, port }),
    cancel: () => invoke("cancel"),
    findDevice: (hostname) => invoke("find_device", { hostname }),
    openLink: (url) => invoke("open_link", { url }),
    // The system's clipboard through the app: the page's own is switched off in the Linux web view.
    copyText: async (text) => {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(text);
    },
    onImage: (cb, done) => pair<Prepared>("image", cb, done),
    onFlash: (cb, done) => pair<FlashReport>("flash", cb, done),
  };
}

/** A FIBER that appears on USB a few seconds after the Connect step opens, stays in USB mode for a
 * few seconds after writing (until "RESET") and is found on the network on the third look.
 * `openLink` and `copyText` are the app's own in demo mode (the browser's in a plain browser). */
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
  let flashCbs: [(e: JobEvent) => void, (d: Done<FlashReport>) => void] | null = null;
  let access = false;
  let boardsAsked = 0;
  /** Polls since the write finished; null before. */
  let afterWrite: number | null = null;
  let searches = 0;
  let cancelled = false;

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
    appInfo: async () => ({
      version: "1.0.0",
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
        language: navigator.language.startsWith("cs") ? "cs" : "en",
      },
    }),
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
      if (afterWrite !== null) {
        afterWrite++;
        return afterWrite <= 5
          ? [{ port: "3-2", kind: "gadget", product_id: "0104", serial: "10000000a1b2c3d4" }]
          : [];
      }
      boardsAsked++;
      return boardsAsked > 3 ? [{ port: "3-2", kind: "boot", product_id: "2711", serial: null }] : [];
    },
    flashStart: async (settings) => {
      cancelled = false;
      const [cb, done] = flashCbs ?? [() => {}, () => {}];
      const log = (line: string) => cb({ type: "log", line });
      cb({ type: "phase", phase: "boot" });
      for (const line of ["Loading: mass-storage-gadget64/bootfiles.bin", "Sending bootcode.bin", "Second stage boot server done"]) {
        await new Promise((r) => setTimeout(r, 500));
        log(line);
      }
      cb({ type: "phase", phase: "wait_disk" });
      await new Promise((r) => setTimeout(r, 1200));
      log("eMMC /dev/sdb (7.8 GB) on USB port 3-2, CM4 serial 10000000a1b2c3d4");
      cb({ type: "phase", phase: "settings" });
      log(`Settings written for ${settings.install_fiber ? "fiber-a1b2c3d4" : settings.hostname} (instance fiber-setup-1793820007-22517e79)`);
      run(cb, [
        { phase: "write", total: prepared.size, ms: 5000 },
        { phase: "verify", total: prepared.size, ms: 2500 },
      ])
        .then(() => {
          afterWrite = 0;
          cb({ type: "phase", phase: "done" });
          done({
            ok: true,
            result: {
              disk: "/dev/sdb",
              disk_size: 7_818_182_656,
              serial: "10000000a1b2c3d4",
              hub_name: "fiber-a1b2c3d4",
              hostname: settings.install_fiber ? "fiber-a1b2c3d4" : settings.hostname,
              written: prepared.size,
              instance_id: "fiber-setup-1793820007-22517e79",
              seconds: 214,
            },
            error: null,
          });
        })
        .catch((e) => done({ ok: false, result: null, error: String(e.message ?? e) }));
    },
    cancel: async () => {
      cancelled = true;
    },
    findDevice: async () => {
      await new Promise((r) => setTimeout(r, 900));
      return ++searches >= 3 ? { ip: "192.168.1.57", ssh: true } : null;
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
    onFlash: async (cb, done) => {
      flashCbs = [cb, done];
      return () => (flashCbs = null);
    },
    nextBoard: () => {
      boardsAsked = 0;
      afterWrite = null;
      searches = 0;
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
