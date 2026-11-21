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
///
/// NOT PUBLISHED YET: the sha256 and size are those of the image the platform's release workflow
/// built in its dry run 44913866458 (from fiber-platform commit a005aff), so that everything can be
/// tried before the first release exists (put that file into the cache under `file_name`). Replace
/// them with the published image's (the platform's D24) before this is pushed or released: the
/// address does not exist yet.
pub const FIBER_IMAGE: OsImage = OsImage {
    id: "fiber-platform-0.1.0-image-arm64",
    name: "FIBER platform 0.1.0",
    version: "on Raspberry Pi OS Lite of 2026-10-06",
    url: "https://github.com/hardwario/fiber-platform/releases/download/v0.1.0/fiber-platform-0.1.0-image-arm64.img.xz",
    sha256: "e13658b3a0081c2dd47ce79a3ea80882d3b999d85334b1d5675c1d66278751e4",
    size: 652_539_084,
    file_name: "fiber-platform-0.1.0-image-arm64.img.xz",
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
/// NOT PUBLISHED YET: this is the installer of release 0.1.0 as the platform's release workflow
/// built it in its dry run 44913866458 (fiber-platform commit a005aff), the one inside
/// [`FIBER_IMAGE`], so that everything can be tried before the first release exists. Replace it
/// with the published one (the platform's D24) before this is pushed or released: a FIBER cannot
/// download this installer.
pub const FIBER_PLATFORM: PlatformRelease = PlatformRelease {
    version: "0.1.0",
    installer_sha256: "8852632b3b601c8a25d3644f0d1cd0f39c803b683bca9d35203472c632acf4ef",
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
