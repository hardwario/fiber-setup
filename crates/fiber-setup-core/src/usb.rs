//! Raspberry Pi boards on USB: a FIBER with its jumper on BOOT shows up as the BCM2711 boot ROM
//! (`0a5c:2711`), and after rpiboot as the mass-storage gadget. Found in sysfs on Linux, in
//! the I/O Registry on macOS (`macos.rs`) and among Windows' USB devices (`windows.rs`).

use serde::Serialize;
use std::path::Path;

/// Broadcom, the vendor of the boot ROM and of the gadget rpiboot loads.
pub(crate) const BROADCOM_VID: &str = "0a5c";
/// Boot ROM product ids: BCM2710, BCM2711 (two revisions) and BCM2712.
pub(crate) const BOOT_PIDS: [&str; 4] = ["2763", "2764", "2711", "2712"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardKind {
    /// Waiting for rpiboot (jumper on BOOT, powered, USB-B connected).
    Boot,
    /// Running the mass-storage gadget: its eMMC is a disk on this computer.
    Gadget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsbBoard {
    /// The USB path of its port, e.g. `3-2` or `1-1.4`: the name rpiboot's `-p` takes and the
    /// name the gadget gets on the same port. On Linux the kernel's name for the port; on macOS
    /// the same form from the device's location, as libusb (and so rpiboot) builds it there. On
    /// Windows a name of that form from the device's location path, which rpiboot does not know.
    pub port: String,
    pub kind: BoardKind,
    pub product_id: String,
    pub serial: Option<String>,
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn boards() -> Vec<UsbBoard> {
    let emmc_ports: Vec<String> = crate::disk::list()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d.looks_like_cm4_emmc())
        .filter_map(|d| d.usb_port)
        .collect();
    boards_in(Path::new("/sys/bus/usb/devices"), &emmc_ports)
}

#[cfg(target_os = "macos")]
pub fn boards() -> Vec<UsbBoard> {
    crate::macos::boards()
}

#[cfg(windows)]
pub fn boards() -> Vec<UsbBoard> {
    crate::windows::boards()
}

/// The boards in a sysfs USB device directory (Linux).
///
/// Broadcom also makes Bluetooth adapters and fingerprint readers (`0a5c:5834` in many laptops),
/// so a Broadcom device that is not a boot ROM counts only when a CM4 eMMC disk hangs on its port.
pub fn boards_in(root: &Path, emmc_ports: &[String]) -> Vec<UsbBoard> {
    let read = |d: &Path, f: &str| std::fs::read_to_string(d.join(f)).ok().map(|s| s.trim().to_lowercase());
    let mut out: Vec<UsbBoard> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            // Interfaces (`3-2:1.0`) and root hubs (`usb3`) are not boards.
            if name.contains(':') || name.starts_with("usb") {
                return None;
            }
            let dir = e.path();
            if read(&dir, "idVendor")? != BROADCOM_VID {
                return None;
            }
            let pid = read(&dir, "idProduct")?;
            let kind = if BOOT_PIDS.contains(&pid.as_str()) {
                BoardKind::Boot
            } else if emmc_ports.contains(&name) {
                BoardKind::Gadget
            } else {
                return None;
            };
            let serial = std::fs::read_to_string(dir.join("serial"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            Some(UsbBoard { port: name, kind, product_id: pid, serial })
        })
        .collect();
    out.sort_by(|a, b| a.port.cmp(&b.port));
    out
}

// Unix only: sysfs names a USB interface with a colon (`3-2:1.0`), which Windows allows in no
// folder name.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn dev(root: &Path, name: &str, vid: &str, pid: &str) {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("idVendor"), format!("{vid}\n")).unwrap();
        std::fs::write(d.join("idProduct"), format!("{pid}\n")).unwrap();
    }

    #[test]
    fn finds_boot_rom_and_gadget_only() {
        let t = tempfile::tempdir().unwrap();
        dev(t.path(), "3-2", "0a5c", "2711");
        dev(t.path(), "1-1.4", "0a5c", "0104");
        dev(t.path(), "1-3", "0a5c", "5834");
        dev(t.path(), "1-2", "046d", "c52b");
        dev(t.path(), "usb1", "1d6b", "0002");
        std::fs::create_dir_all(t.path().join("3-2:1.0")).unwrap();
        // 1-3 is a Broadcom fingerprint reader: no eMMC on its port, so not a board.
        let b = boards_in(t.path(), &["1-1.4".to_string()]);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].port, "1-1.4");
        assert_eq!(b[0].kind, BoardKind::Gadget);
        assert_eq!(b[1].port, "3-2");
        assert_eq!(b[1].kind, BoardKind::Boot);
    }
}
