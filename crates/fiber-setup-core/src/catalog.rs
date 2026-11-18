//! What FIBER Setup knows by checksum: the operating system image, and the release of the FIBER
//! platform a new FIBER installs.

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

/// A release of the FIBER platform, named by its installer: a new FIBER installs this one, and
/// only a release of FIBER Setup moves it (the platform's D18). The installer then checks the
/// release's archive by the sha256 written into it.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PlatformRelease {
    pub version: &'static str,
    /// sha256 of the release's `install.sh`, as its `SHA256SUMS` has it.
    pub installer_sha256: &'static str,
}

impl PlatformRelease {
    /// Where the release's installer is: its page on GitHub Releases.
    pub fn installer_url(&self) -> String {
        format!("https://github.com/hardwario/fiber-platform/releases/download/v{}/install.sh", self.version)
    }
}

/// The FIBER platform a new FIBER installs on its first start. `image/components.yaml` is the
/// same release's `provision/components.yaml`, and moves with it.
///
/// NOT PUBLISHED YET: this is the installer of release 0.1.0 as built by hand from
/// fiber-platform commit 0257447 (`just release`), so that everything can be tried before the
/// first release exists. Replace it with the published one (the platform's D24) before this is
/// pushed or released: a FIBER cannot download this installer.
pub const FIBER_PLATFORM: PlatformRelease = PlatformRelease {
    version: "0.1.0",
    installer_sha256: "623784adbdb5b660a51446f9560f253f4416b0beacd65d7d452512d5fc9dd72f",
};

/// Places where another program may already keep the same compressed file, so it need not be
/// downloaded again: Raspberry Pi Imager's cache of its last download. A copy is used only when
/// its size and checksum match.
pub fn known_copies() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    vec![home.join(".cache/Raspberry Pi/Raspberry Pi Imager/lastdownload.cache")]
}
