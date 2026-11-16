# FIBER Setup

Sets up a HARDWARIO **FIBER** (CM4) from a computer with nothing but a USB cable: no J-Link, no Ethernet
adapter, no terminal. A step-by-step wizard with photos writes the operating system to the FIBER's eMMC over
its USB-B port, together with the first-start settings (name, login, Wi-Fi). On its first start the FIBER
installs the FIBER software by itself and restarts once.

Same structure and look as [hardwario/tower-bridge](https://github.com/hardwario/tower-bridge): Tauri 2,
Rust, Svelte 5; `crates/fiber-setup-core` (no Tauri), `crates/fiber-setup-cli`, `src-tauri`, `src`.

**Status: 1.1.0 (2026-11-15).** Linux, macOS and, new in this version, Windows. Not yet tested on a real
FIBER; the macOS and Windows sides have not had a FIBER connected at all yet.

## How it works

1. **Software.** Raspberry Pi OS Lite (64-bit) 2026-10-06, checked against its published sha256. Downloaded
   once into `~/.cache/fiber-setup`, or taken from Raspberry Pi Imager's cache when it has the same file.
2. **Settings** go into the image's FAT boot partition before it is written (the "image loopback": change the
   image, then flash it): cloud-init `user-data`, `network-config`, `meta-data` and the kernel command line,
   the same files Raspberry Pi Imager 2.x writes. With "Install the FIBER software" on,
   `image/fiber-install.sh` goes there too, and cloud-init runs it once on the first start: it waits for
   the network and the clock, then runs HARDWARIO's one-command installer
   `curl -fsSL https://raw.githubusercontent.com/hardwario/fiber-agent/main/bootstrap.sh | sudo bash`
   (the latest fiber-agent release; v6.0.0 on 2026-11-04).
3. **USB mode.** With the jumper on BOOT the CM4 boot ROM appears on USB (`0a5c:2711`). The app sends it
   Raspberry Pi's boot code and then the mass-storage gadget, the way Raspberry Pi's
   [rpiboot](https://github.com/raspberrypi/usbboot) does (`crates/fiber-setup-core/src/rpiboot.rs`; no
   separate program, no libusb), and the eMMC appears as a disk. Only a disk on that same USB port with the
   CM4 gadget's identity is ever written.
4. **Write and check.** The image is written (partition table last), then read back from the eMMC and
   compared byte for byte.
5. **First start.** The FIBER joins the network, installs the software (about 10 minutes, needs the internet)
   and restarts once. With the software it is `fiber-<last 8 characters of its CM4 serial>` from the very
   first start: the app reads the serial over USB before writing, so the name never changes. Without the
   software it keeps the name from the settings.

## Run it

```sh
npm ci
npm run usbboot             # the gadget's files into src-tauri/resources/usbboot
npm run tauri dev           # the app with hot reload
npm run dev                 # the screens in a browser with a simulated FIBER: http://localhost:1430
```

`npm run usbboot` downloads the gadget's files (`boot.img`, `bootfiles.bin`, `config.txt`) from
raspberrypi/usbboot, at the commit 0.1.0 was made with, and checks each against its pinned SHA-256. Nothing
is built. With `USBBOOT_DIR` set it takes them from a usbboot checkout on this computer instead.

Release build: `npx tauri build` gives `target/release/fiber-setup` and, in `target/release/bundle/`, the
AppImage and .deb on Linux, `FIBER Setup.app` and the .dmg on macOS (with `CI=true` the .dmg is made
without opening a Finder window). The releases on GitHub are built, signed and notarized by the Release
workflow: [docs/release.md](docs/release.md).

On Windows (in PowerShell, as administrator for the app itself):

```powershell
npm ci
npm run usbboot                            # the gadget's files and the USB driver's description
scripts\driver-catalog.ps1                 # the USB driver's catalog (unsigned)
npm run tauri dev
npx tauri build --no-bundle                # target\release\fiber-setup.exe
scripts\windows-package.ps1 -Step stage -Exe target\release\fiber-setup.exe
scripts\windows-package.ps1 -Step uninstaller
scripts\windows-package.ps1 -Step installer   # target\windows-package\out\...-setup.exe (needs NSIS 3)
scripts\windows-package.ps1 -Step zip
```

A driver built by hand is not signed, so Windows does not take it; `scripts\windows-test.ps1` shows how CI
gets around that on its own machine.

Demo: `fiber-setup --demo` (or `FIBER_SETUP_DEMO=1`) runs the app on the same simulated FIBER, all steps
from download to "Set up another FIBER", without touching USB or the network. The title says "(demo)".

## Updates

When it starts, and once a day for as long as it stays open, the app asks GitHub for the newest release (and
sends nothing else). When a newer one is out, the Welcome page says so, with a Download button that opens
the release's page, and a line under the step list keeps saying so on every step. The app does not update
itself: download the new version and install it in place of the old one. Drafts and pre-releases are never
offered. The same as in TOWER Bridge, without its tray item and notification.

| Setting | What it does |
|---|---|
| `FIBER_SETUP_UPDATE_CHECK=0` | The app does not ask (the demo never does) |
| `FIBER_SETUP_UPDATE_URL=http://…` | Ask this address instead of GitHub (a JSON file with `tag_name`), to try the notice |

In the browser preview, `http://localhost:1430/?update=available` shows the notice.

The first time on Linux, the app asks to **allow USB access**: two udev rules (the Fiber Provisioning
Station's, with the partition lines fixed so the desktop does not mount the FIBER's partitions) that let the
desktop user talk to the CM4 boot ROM and write the CM4 eMMC, and nothing else. They
are installed with `pkexec` (password dialog).

## On Windows

The steps are the same; these things under them differ.

- **Administrator.** The app asks for administrator rights when it starts (the usual Windows prompt).
  Writing a disk and installing a driver both need them.
- **The USB driver.** Windows has no driver for a FIBER in USB mode. FIBER Setup brings one: a few lines
  (`packaging/windows/driver/fiber-usbboot.inf`) that tell Windows to use its own WinUSB for the boot ROM,
  with a catalog signed by HARDWARIO a.s. The installer installs it; otherwise the Connect step offers
  "Install the USB driver". Windows asks once whether to trust software from HARDWARIO a.s.
- **Finding the FIBER.** The boards come from Windows' list of USB devices, the disks and the USB device
  each hangs on from its storage management (PowerShell's `Get-Disk` and the device tree). A USB port is
  named after its place (`1400-2.1`: the controller, then the port on each hub).
- **Writing.** Windows writes into no volume it has mounted, so the eMMC's old partition table is removed
  first (`diskpart clean`), and its volumes with it. The disk is opened by its device path, which names its
  vendor and serial number, not by its number, and never if Windows boots from it.

The installer (`...-windows-x64-setup.exe`) puts the app into Program Files, installs Microsoft's WebView2
where Windows does not have it yet, and installs the USB driver. The portable zip is the same app in a
folder: unpack it anywhere and start `fiber-setup.exe`. Both are signed by HARDWARIO a.s.
([docs/release.md](docs/release.md)).

## On macOS

The steps are the same; three things under them differ.

- **Finding the FIBER.** The boards and the disks behind them come from the I/O Registry (`ioreg`). The USB
  port is named after its place (the bus, then the port on each hub: `1-1`, `2-1.3`), from the device's
  location.
- **Writing.** There is nothing to allow beforehand. When the write starts, macOS asks for the password
  (`authopen`) and hands the app the open eMMC, so nothing of the app runs as root. Before the dialog and
  again with the disk open, the app checks that the disk is a CM4's eMMC on USB; it opens no other disk.
  The eMMC is written and read back through its raw device (`/dev/rdiskN`), which has no cache in between.
- **Mounting.** macOS mounts every disk it sees. The FIBER's volumes are unmounted before the write, and the
  eMMC is ejected after it, so macOS does not write its own files to the new boot partition.

The app needs nothing installed where it runs, and the bundle holds one program, the app itself. A copy
built by hand is not signed with a Developer ID and not notarized: it starts
on the Mac that built it, and Gatekeeper stops it elsewhere. The released dmg is signed and notarized
([docs/release.md](docs/release.md)).

## Command line

```sh
fiber-setup-cli devices                      # FIBERs on USB and the disks on this computer
fiber-setup-cli image                        # download/check/unpack the image
fiber-setup-cli boot                         # start a FIBER over USB: its eMMC shows as a disk, nothing is written
FIBER_SETUP_PASSWORD=... FIBER_SETUP_WIFI_PASSWORD=... \
  fiber-setup-cli flash --hostname fiber --wifi-ssid MyWifi --country CZ --ssh-key ~/.ssh/id_ed25519.pub
fiber-setup-cli flash ... --to-file out.img  # the same into a file, no FIBER needed
fiber-setup-cli inspect out.img              # the first-boot files on an image or eMMC
fiber-setup-cli find fiber                   # look for fiber.local
```

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check
npm run build
```

## Not in this version

- The ready FIBER image built in CI (software already inside, first start without internet).
- Wi-Fi from the phone over Bluetooth is offered, but the first start still needs the internet (Ethernet)
  to install the software; not tried with the HARDWARIO Manager app yet.
- A plain customer build of the FIBER software: fiber-agent v6.0.0 is the ProXimos Hub stack branded FIBER.
- macOS and Windows: a first write to a real FIBER (see the status above). On Windows also the first
  install of the USB driver with a FIBER connected.
- The app's own start over USB, which took the place of the bundled rpiboot after 1.1.0, has been run against
  a simulated board only. `fiber-setup-cli boot` tries it on a real FIBER without writing anything.
- Windows on ARM: the driver covers it, the app is built for Intel/AMD only (it runs there emulated).

## Pictures

The FIBER logo and product renders are HARDWARIO artworks cleared for public use;
`src/assets/artwork.lock.json` records each one's source file, checksum and version. The board photos
in the steps were taken for this app.

## Related repositories

- [hardwario/fiber-agent](https://github.com/hardwario/fiber-agent): the FIBER software; its `bootstrap.sh`
  is what the FIBER runs on its first start.
- [hardwario/fiber-platform](https://github.com/hardwario/fiber-platform): the FIBER platform setup by hand.
- [hardwario/tower-bridge](https://github.com/hardwario/tower-bridge): the app this one's look and layout
  come from.
- [raspberrypi/usbboot](https://github.com/raspberrypi/usbboot): the mass-storage gadget's files, fetched
  by `npm run usbboot`, and rpiboot, whose way of starting a board over USB the app follows.

## Licence

MIT, with one exception: `crates/fiber-setup-core/src/rpiboot.rs` is derived from Raspberry Pi's rpiboot
and is, like it, under the Apache License 2.0 (`LICENSES/Apache-2.0.txt`). The gadget's files are Raspberry
Pi's, under their own licences, which `npm run usbboot` puts into the bundle next to them.
