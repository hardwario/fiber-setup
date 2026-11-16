/** What the Rust side sends; snake_case as in tower-bridge. */

export interface OsImage {
  id: string;
  name: string;
  version: string;
  url: string;
  sha256: string;
  size: number;
  file_name: string;
}

export interface AppInfo {
  version: string;
  os: string;
  image: OsImage;
  tools_found: boolean;
  defaults: {
    hostname: string;
    username: string;
    country: string;
    timezone: string | null;
    ssh_key: string | null;
    ssh_key_path: string | null;
    language: "en" | "cs";
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

export type Phase = "download" | "check" | "unpack" | "settings" | "boot" | "wait_disk" | "write" | "verify" | "done";

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
  install_fiber: boolean;
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

export interface Found {
  ip: string;
  ssh: boolean;
}

/** A version of FIBER Setup newer than the running one. */
export interface Release {
  /** "1.2.0" */
  version: string;
  /** Its page on GitHub, with the downloads. */
  url: string;
}
