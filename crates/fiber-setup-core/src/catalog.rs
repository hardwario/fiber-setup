//! The operating system images FIBER Setup knows by checksum.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct OsImage {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub url: &'static str,
    /// sha256 of the compressed file, as Raspberry Pi publishes it next to the image.
    pub sha256: &'static str,
    /// Size of the compressed file in bytes.
    pub size: u64,
    pub file_name: &'static str,
}

/// Raspberry Pi OS Lite (64-bit) of 2026-10-06, the one the FIBER docs and the setup script were
/// tested with (2026-11-04). The FIBER image built in CI replaces it once it exists.
pub const RASPIOS_LITE: OsImage = OsImage {
    id: "raspios-lite-arm64-2026-10-06",
    name: "Raspberry Pi OS Lite (64-bit)",
    version: "2026-10-06, Debian 13 (trixie)",
    url: "https://downloads.raspberrypi.com/raspios_lite_arm64/images/raspios_lite_arm64-2026-10-06/2026-10-06-raspios-trixie-arm64-lite.img.xz",
    sha256: "483db18a48da399b5b7022ffae9a07bc6d99daab30e593aedac09b69ff422843",
    size: 550_466_056,
    file_name: "2026-10-06-raspios-trixie-arm64-lite.img.xz",
};

/// The one-command installer the FIBER runs on its first start (`fiber-install.sh`): it
/// installs the latest fiber-agent release (v6.0.0 on 2026-11-04).
pub const FIBER_INSTALLER: &str = "https://raw.githubusercontent.com/hardwario/fiber-agent/main/bootstrap.sh";

/// Places where another program may already keep the same compressed file, so it need not be
/// downloaded again: Raspberry Pi Imager's cache of its last download. A copy is used only when
/// its size and checksum match.
pub fn known_copies() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    vec![home.join(".cache/Raspberry Pi/Raspberry Pi Imager/lastdownload.cache")]
}
