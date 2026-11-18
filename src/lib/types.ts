import type { Lang } from "./i18n/index.svelte";

/** What the Rust side sends; snake_case, as Rust names them. */

export interface OsImage {
  id: string;
  name: string;
  version: string;
  url: string;
  sha256: string;
  size: number;
  file_name: string;
}

/** The release of the FIBER platform a new FIBER installs (`catalog.rs`). */
export interface PlatformRelease {
  version: string;
  installer_sha256: string;
}

/** Where a component listens: `loopback` is the FIBER itself only. */
export interface Port {
  port: number;
  listen: "loopback" | "network";
  /** The setting that opens it, as in `mqtt.lan`. */
  when: string | null;
}

/** A component of the FIBER platform, from the copy of its `components.yaml`. */
export interface Component {
  id: string;
  /** In English, as the release has them; the app shows its own translation. */
  title: string;
  about: string;
  requires: string[];
  suggests: string[];
  ports: Port[];
  /** Download and installed size in MB, once measured on a FIBER. */
  size: { download: number | null; installed: number | null } | null;
}

/** What a new FIBER can install, and what is ticked to begin with. */
export interface Offer {
  release: PlatformRelease;
  components: Component[];
  default: string[];
}

/** The card in the mini-PCIe slot (the platform's `hardware.minipcie`). */
export type MiniPcie = "none" | "lte" | "rak5146";

export interface AppInfo {
  version: string;
  os: string;
  image: OsImage;
  platform: Offer | null;
  tools_found: boolean;
  defaults: {
    hostname: string;
    username: string;
    country: string;
    timezone: string | null;
    ssh_key: string | null;
    ssh_key_path: string | null;
    language: Lang;
  };
}

export interface Prepared {
  raw: string;
  size: number;
  source_sha256: string;
  official: boolean;
  name: string;
}

export interface ImageStatus {
  status: { image: OsImage; ready: boolean; other_copy: string | null };
  prepared: Prepared | null;
}

export type Phase =
  | "download"
  | "check"
  | "unpack"
  | "settings"
  | "boot"
  | "wait_disk"
  | "ready"
  | "write"
  | "verify"
  | "done";

export type JobEvent =
  | { type: "phase"; phase: Phase }
  | { type: "progress"; phase: Phase; done: number; total: number }
  | { type: "log"; line: string };

export interface Done<T> {
  ok: boolean;
  result: T | null;
  error: string | null;
}

export interface UsbBoard {
  port: string;
  kind: "boot" | "gadget";
  product_id: string;
  serial: string | null;
}

export interface AccessStatus {
  rules_installed: boolean;
  pkexec: boolean;
}

export interface Settings {
  hostname: string;
  username: string;
  password: string;
  ssh_key: string | null;
  wifi: { ssid: string; password: string; hidden: boolean } | null;
  country: string;
  timezone: string | null;
  /** The components to install, with what they require; none: a plain Raspberry Pi OS. */
  components: string[];
  mqtt_lan: boolean;
  minipcie: MiniPcie;
}

/** One FIBER of a batch: its USB port and, without the FIBER software, the name it gets. */
export interface FlashDevice {
  port: string;
  hostname: string | null;
}

export interface FlashReport {
  disk: string;
  disk_size: number;
  serial: string | null;
  hub_name: string | null;
  hostname: string;
  written: number;
  instance_id: string;
  seconds: number;
}

/** A FIBER on the network, and its two Avahi records while it installs (`check.rs`). */
export interface Found {
  ip: string;
  ssh: boolean;
  /** The first-start script's: `state`, `release`, and `error` after a failure. */
  setup: Record<string, string> | null;
  /** The FIBER platform's: `state`, `step` ("2/5"), `component`, `release`. */
  platform: Record<string, string> | null;
}

/** A version of FIBER Setup newer than the running one. */
export interface Release {
  /** "1.2.0" */
  version: string;
  /** Its page on GitHub, with the downloads. */
  url: string;
}
