#!/bin/bash
# Puts rpiboot and the mass-storage gadget into src-tauri/resources/usbboot, which the app bundles.
#
# Source: a built raspberrypi/usbboot checkout, USBBOOT_DIR (default ~/github/raspberrypi/usbboot).
# Without one, it clones and builds the pinned tag below (needs git, make, gcc, libusb-1.0-0-dev).
# The Provisioning Station pins the gadget files to commit 861a703, because the tag's boot.img had no
# kernel modules (usbboot#403). The local checkout used for 0.1.0 (d3d51bb) exposed a FIBER's eMMC with
# `rpiboot -d mass-storage-gadget64` on 2026-11-04, so its gadget works.
set -euo pipefail

USBBOOT_TAG=20261002-115811
here=$(cd "$(dirname "$0")/.." && pwd)
out="$here/src-tauri/resources/usbboot"
src=${USBBOOT_DIR:-$HOME/github/raspberrypi/usbboot}

if [ ! -x "$src/rpiboot" ]; then
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    git clone --depth 1 --branch "$USBBOOT_TAG" https://github.com/raspberrypi/usbboot "$tmp/usbboot"
    make -C "$tmp/usbboot"
    src="$tmp/usbboot"
fi

gadget="$src/mass-storage-gadget64"
for f in boot.img bootfiles.bin config.txt; do
    [ -f "$gadget/$f" ] || { echo "error: $gadget/$f is missing" >&2; exit 1; }
done

rm -rf "$out"
mkdir -p "$out/mass-storage-gadget64"
install -m 0755 "$src/rpiboot" "$out/rpiboot"
install -m 0644 "$gadget/boot.img" "$gadget/bootfiles.bin" "$gadget/config.txt" "$out/mass-storage-gadget64/"
install -m 0644 "$src/LICENSE" "$out/LICENSE"
[ -f "$src/LICENSE.firmware" ] && install -m 0644 "$src/LICENSE.firmware" "$out/LICENSE.firmware"
{
    echo "rpiboot and mass-storage-gadget64 from https://github.com/raspberrypi/usbboot"
    echo "commit: $(git -C "$src" rev-parse HEAD 2>/dev/null || echo unknown)"
    echo "boot.img contains a Linux kernel and BusyBox (GPL); their sources: see the usbboot repository."
} > "$out/SOURCES.txt"
echo "usbboot bundle ready in $out"
ls -la "$out" "$out/mass-storage-gadget64"
