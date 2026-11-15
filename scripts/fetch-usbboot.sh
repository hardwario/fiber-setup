#!/bin/bash
# Puts rpiboot and the mass-storage gadget into src-tauri/resources/usbboot, which the app bundles.
#
# Source: a built raspberrypi/usbboot checkout, USBBOOT_DIR (default ~/github/raspberrypi/usbboot).
# Without one, it fetches and builds the pinned commit below (needs git, make and a C compiler;
# on Linux also libusb's headers, `libusb-1.0-0-dev` on Debian/Ubuntu).
#
# The pin is the commit 0.1.0 was made with: its gadget exposed a FIBER's eMMC with
# `rpiboot -d mass-storage-gadget64` on 2026-11-04. Not the release tag before it
# (20261002-115811): that tag's boot.img is the corrupted one, without kernel modules (usbboot#403),
# fixed in 861a703, which is why the Provisioning Station pins its gadget files to 861a703.
#
# macOS has no libusb of its own, so the bundle carries one next to rpiboot. Built here, both
# come out for Apple silicon and Intel in one file and run from macOS 11 on: libusb from its
# pinned source release, rpiboot against it. (Homebrew's libusb is for the Mac it was installed
# on only; it is taken along only when a prebuilt checkout was linked against it.)
#
# On Windows this runs in Cygwin's bash, the way Raspberry Pi builds rpiboot for Windows (Cygwin
# packages gcc-core, make, git, pkg-config, libusb1.0-devel). rpiboot.exe needs Cygwin's DLLs
# next to it, so they go into the bundle, and so does the USB driver's description
# (packaging/windows/driver); its signed catalog is made when a release is built.
set -euo pipefail

USBBOOT_COMMIT=d3d51bbe13593555f03a0470d8ca750be1518670
LIBUSB_VERSION=1.0.30
LIBUSB_SHA256=fea36f34f9156400209595e300840767ab1a385ede1dc7ee893015aea9c6dbaf
LIBUSB_URL=https://github.com/libusb/libusb/releases/download/v$LIBUSB_VERSION/libusb-$LIBUSB_VERSION.tar.bz2

case "$(uname -s)" in
    CYGWIN*) exe=.exe ;;
    *) exe= ;;
esac
here=$(cd "$(dirname "$0")/.." && pwd)
out="$here/src-tauri/resources/usbboot"
src=${USBBOOT_DIR:-$HOME/github/raspberrypi/usbboot}
libusb_license=
libusb_source=

if [ ! -x "$src/rpiboot$exe" ]; then
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    git init --quiet "$tmp/usbboot"
    git -C "$tmp/usbboot" fetch --quiet --depth 1 https://github.com/raspberrypi/usbboot "$USBBOOT_COMMIT"
    git -C "$tmp/usbboot" checkout --quiet FETCH_HEAD
    if [ "$(uname -s)" = Darwin ]; then
        export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}
        archs="-arch arm64 -arch x86_64"
        curl -fsSL -o "$tmp/libusb.tar.bz2" "$LIBUSB_URL"
        echo "$LIBUSB_SHA256  $tmp/libusb.tar.bz2" | shasum -a 256 -c - > /dev/null \
            || { echo "error: $LIBUSB_URL has another checksum than the pinned one" >&2; exit 1; }
        tar xjf "$tmp/libusb.tar.bz2" -C "$tmp"
        # No dependency tracking: the compiler cannot write it for two architectures at once.
        (cd "$tmp/libusb-$LIBUSB_VERSION" \
            && ./configure --quiet --prefix="$tmp/libusb" --disable-static --disable-dependency-tracking \
                CFLAGS="$archs -O2" \
            && make --quiet -j4 install) > "$tmp/libusb.log" 2>&1 \
            || { cat "$tmp/libusb.log" >&2; echo "error: building libusb failed" >&2; exit 1; }
        # usbboot's Makefile asks pkg-config where libusb is: answer for the one just built,
        # whatever is (or is not) installed on this Mac.
        mkdir "$tmp/bin"
        printf '#!/bin/sh\necho "-I%s/include/libusb-1.0 -L%s/lib -lusb-1.0"\n' "$tmp/libusb" "$tmp/libusb" \
            > "$tmp/bin/pkg-config"
        chmod +x "$tmp/bin/pkg-config"
        PATH="$tmp/bin:$PATH" make -C "$tmp/usbboot" CC="cc $archs"
        libusb_license="$tmp/libusb-$LIBUSB_VERSION/COPYING"
        libusb_source="libusb $LIBUSB_VERSION, built from $LIBUSB_URL"
    else
        make -C "$tmp/usbboot"
    fi
    src="$tmp/usbboot"
fi

gadget="$src/mass-storage-gadget64"
for f in boot.img bootfiles.bin config.txt; do
    [ -f "$gadget/$f" ] || { echo "error: $gadget/$f is missing" >&2; exit 1; }
done

rm -rf "$out"
mkdir -p "$out/mass-storage-gadget64"
install -m 0755 "$src/rpiboot$exe" "$out/rpiboot$exe"
install -m 0644 "$gadget/boot.img" "$gadget/bootfiles.bin" "$gadget/config.txt" "$out/mass-storage-gadget64/"
install -m 0644 "$src/LICENSE" "$out/LICENSE"
[ -f "$src/LICENSE.firmware" ] && install -m 0644 "$src/LICENSE.firmware" "$out/LICENSE.firmware"

if [ "$(uname -s)" = Darwin ]; then
    # The libusb rpiboot was linked against, by its full path (not one already next to rpiboot).
    lib=$(otool -L "$out/rpiboot" | awk '$1 ~ /^\/.*libusb-1\.0/ { print $1; exit }')
    if [ -n "$lib" ]; then
        name=$(basename "$lib")
        install -m 0644 "$lib" "$out/$name"
        install_name_tool -id "@executable_path/$name" "$out/$name"
        install_name_tool -change "$lib" "@executable_path/$name" "$out/rpiboot"
        # Changing them broke their signatures, and macOS on Apple silicon runs nothing unsigned.
        # Ad hoc here; a release signs them again with the Developer ID (docs/release.md).
        codesign --force --sign - "$out/$name" "$out/rpiboot"
        if [ -z "$libusb_source" ]; then
            libusb_license="$(dirname "$(dirname "$lib")")/COPYING"
            libusb_source="$name, copied from $lib"
        fi
        [ -f "$libusb_license" ] && install -m 0644 "$libusb_license" "$out/LICENSE.libusb"
    fi
fi

cygwin=
if [ -n "$exe" ]; then
    # The DLLs rpiboot.exe loads that are Cygwin's (Windows' own are everywhere).
    for dll in $(ldd "$out/rpiboot.exe" | awk '$3 ~ /^\/usr\/bin\// { print $3 }'); do
        install -m 0755 "$dll" "$out/"
        cygwin="$cygwin $(basename "$dll")"
    done
    [ -n "$cygwin" ] || { echo "error: rpiboot.exe loads no Cygwin DLL; was it built in Cygwin?" >&2; exit 1; }
    mkdir -p "$out/driver"
    install -m 0644 "$here/packaging/windows/driver/fiber-usbboot.inf" "$out/driver/"
fi

{
    echo "rpiboot and mass-storage-gadget64 from https://github.com/raspberrypi/usbboot"
    echo "commit: $(git -C "$src" rev-parse HEAD 2>/dev/null || echo unknown)"
    echo "boot.img contains a Linux kernel and BusyBox (GPL); their sources: see the usbboot repository."
    if [ -n "$libusb_source" ]; then
        echo "libusb (LGPL-2.1, https://libusb.info): $libusb_source; rpiboot loads it from its own folder."
    fi
    if [ -n "$cygwin" ]; then
        echo "Cygwin's DLLs (https://cygwin.com; cygwin1.dll LGPL-3.0-or-later, cygusb is libusb, LGPL-2.1):$cygwin."
        echo "driver/: the WinUSB driver description for the boot ROM, by HARDWARIO a.s. (this repository)."
    fi
} > "$out/SOURCES.txt"
echo "usbboot bundle ready in $out"
ls -la "$out" "$out/mass-storage-gadget64"
if [ -d "$out/driver" ]; then ls -la "$out/driver"; fi
