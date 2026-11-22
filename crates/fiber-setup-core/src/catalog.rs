//! What FIBER Setup knows by checksum: the image it writes, and the release of the FIBER platform
//! a new FIBER installs.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct OsImage {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub url: &'static str,
    /// sha256 of the compressed file, as the release's `SHA256SUMS` has it.
    pub sha256: &'static str,
    /// Size of the compressed file in bytes.
    pub size: u64,
    pub file_name: &'static str,
}

/// The FIBER platform's ready-made image of the release a new FIBER installs ([`FIBER_PLATFORM`]):
/// Raspberry Pi OS Lite (64-bit) of 2026-10-06 with the release and every component already in
/// it, none turned on. FIBER Setup always writes it, whatever is ticked (the platform's D28), so a
/// first start needs no internet; with nothing ticked the FIBER starts with nothing turned on.
/// The sha256 and size are those of the published release (its `SHA256SUMS`; 3.69 GB unpacked).
pub const FIBER_IMAGE: OsImage = OsImage {
    id: "fiber-platform-1.0.0-image-arm64",
    name: "FIBER platform 1.0.0",
    version: "on Raspberry Pi OS Lite of 2026-10-06",
    url: "https://github.com/hardwario/fiber-platform/releases/download/v1.0.0/fiber-platform-1.0.0-image-arm64.img.xz",
    sha256: "30549b12b143a9b54242cdb5faf34e79dbb62e49095ca6898946544de77ec56b",
    size: 650_635_620,
    file_name: "fiber-platform-1.0.0-image-arm64.img.xz",
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
/// same release's `provision/components.yaml`, and moves with it. Its installer is the one inside
/// [`FIBER_IMAGE`].
pub const FIBER_PLATFORM: PlatformRelease = PlatformRelease {
    version: "1.0.0",
    installer_sha256: "bd9d84e2115e43f9ddfad77f984c126b7abf5246867ac9854e408a3e394f2e3e",
};

#[cfg(test)]
mod tests {
    use super::*;

    /// The image and the installer are one release: a FIBER written with the image runs the
    /// image's own installer only when it is the release the first-start script names.
    #[test]
    fn the_image_is_the_platforms_release() {
        let v = FIBER_PLATFORM.version;
        let file = format!("fiber-platform-{v}-image-arm64.img.xz");
        assert_eq!(FIBER_IMAGE.file_name, file);
        assert_eq!(
            FIBER_IMAGE.url,
            format!("https://github.com/hardwario/fiber-platform/releases/download/v{v}/{file}")
        );
        assert_eq!(FIBER_IMAGE.id, format!("fiber-platform-{v}-image-arm64"));
        assert_eq!(FIBER_IMAGE.name, format!("FIBER platform {v}"));
        assert_eq!(FIBER_IMAGE.sha256.len(), 64);
        assert!(FIBER_IMAGE.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        const { assert!(FIBER_IMAGE.size > 0) };
    }
}
