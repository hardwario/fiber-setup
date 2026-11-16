#!/usr/bin/env node
// Puts the mass-storage gadget into src-tauri/resources/usbboot, which the app bundles: the files
// a FIBER's CM4 is started with over USB (crates/fiber-setup-core/src/rpiboot.rs sends them).
// Nothing is built: they are Raspberry Pi's files, as they are in raspberrypi/usbboot.
//
// Source: the commit pinned below, straight from GitHub, each file checked against its SHA-256.
// With USBBOOT_DIR set, a usbboot checkout on this computer instead (for trying another gadget).
//
// The pin is the commit 0.1.0 was made with: its gadget exposed a FIBER's eMMC on 2026-11-04.
// Not the release tag before it (20261002-115811): that tag's boot.img is the corrupted one,
// without kernel modules (usbboot#403), fixed in 861a703, which is why the Provisioning Station
// pins its gadget files to 861a703.
//
// On Windows the USB driver's description (packaging/windows/driver) goes into the bundle too;
// its signed catalog is made when a release is built.
import { createHash } from "node:crypto";
import { copyFile, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const COMMIT = "d3d51bbe13593555f03a0470d8ca750be1518670";
const GADGET = "mass-storage-gadget64";
/** Where each file is in usbboot, where it goes in the bundle, and what it must be. */
const FILES = [
  {
    from: `${GADGET}/boot.img`,
    to: `${GADGET}/boot.img`,
    sha256: "e68d4614a0cbdb8404670e350b2ab261f4233dfba5e05e6d7dd691fd33bf254f",
  },
  {
    // The gadget's folder has a link to it.
    from: "firmware/bootfiles.bin",
    to: `${GADGET}/bootfiles.bin`,
    sha256: "c51ef4d816bc2968ca5eef215ca58ce8e2cd4d68d75eae58b4d450f1b5d3d2a6",
  },
  {
    from: `${GADGET}/config.txt`,
    to: `${GADGET}/config.txt`,
    sha256: "f74a9db07cd32f418e25754134840b138486e39cda2a4e536042b86c5c67b687",
  },
  { from: "LICENSE", to: "LICENSE", sha256: "b40930bbcf80744c86c46a12bc9da056641d722716c378f5659b9e555ef833e1" },
  {
    from: "LICENSE.firmware",
    to: "LICENSE.firmware",
    sha256: "654f7876fd9e4cc1b93b071baf6da58df1f263f7591948c69e1978e2ee43d367",
  },
];

const here = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(here, "src-tauri", "resources", "usbboot");
const checkout = process.env.USBBOOT_DIR;

async function download(path) {
  const url = `https://raw.githubusercontent.com/raspberrypi/usbboot/${COMMIT}/${path}`;
  let last;
  for (let attempt = 1; attempt <= 3; attempt++) {
    try {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`${url}: ${response.status} ${response.statusText}`);
      return Buffer.from(await response.arrayBuffer());
    } catch (e) {
      last = e;
      await new Promise((r) => setTimeout(r, attempt * 2000));
    }
  }
  throw last;
}

const sha256 = (data) => createHash("sha256").update(data).digest("hex");

await rm(out, { recursive: true, force: true });
await mkdir(join(out, GADGET), { recursive: true });
let pinned = true;
for (const file of FILES) {
  const data = checkout ? await readFile(join(checkout, file.from)) : await download(file.from);
  const sum = sha256(data);
  if (sum !== file.sha256) {
    if (!checkout) throw new Error(`${file.from} has the checksum ${sum}, not the pinned ${file.sha256}`);
    pinned = false;
  }
  await writeFile(join(out, file.to), data);
  console.log(`${file.to}  ${data.length} bytes  sha256 ${sum.slice(0, 16)}`);
}
const lines = [
  "mass-storage-gadget64 from https://github.com/raspberrypi/usbboot",
  checkout
    ? `from the checkout in ${checkout}${pinned ? ` (the same files as commit ${COMMIT})` : " (not the pinned files)"}`
    : `commit: ${COMMIT}`,
  "boot.img contains a Linux kernel and BusyBox (GPL); their sources: see the usbboot repository.",
  "The app starts a board over USB the way rpiboot of the same repository does; that part of it is derived",
  "from rpiboot and is under the Apache License 2.0, which is the LICENSE here.",
];
if (process.platform === "win32") {
  await mkdir(join(out, "driver"));
  await copyFile(
    join(here, "packaging", "windows", "driver", "fiber-usbboot.inf"),
    join(out, "driver", "fiber-usbboot.inf"),
  );
  lines.push("driver/: the WinUSB driver description for the boot ROM, by HARDWARIO a.s. (this repository).");
  console.log("driver/fiber-usbboot.inf");
}
await writeFile(join(out, "SOURCES.txt"), lines.join("\n") + "\n");
if (!pinned) console.warn(`warning: the files in ${checkout} are not the pinned ones`);
console.log(`usbboot bundle ready in ${out}`);
