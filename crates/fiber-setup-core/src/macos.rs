//! macOS: what sysfs, udev and `/proc/mounts` give on Linux.
//!
//! USB boards and the disks behind them come from the I/O Registry (`ioreg`, one call for both).
//! A disk is written through its raw device (`/dev/rdiskN`), which only root may open: macOS asks
//! for the password (`authopen`) and hands the open disk over, so nothing in this app runs as
//! root. macOS mounts every disk it sees, so the FIBER's volumes are unmounted before writing
//! (waiting until macOS has let go of the disk) and the disk is ejected after.

use crate::disk::BlockDevice;
use crate::usb::{BOOT_PIDS, BROADCOM_VID, BoardKind, UsbBoard};
use anyhow::{Context, Result, bail};
use plist::{Dictionary, Value};
use std::fs::{File, OpenOptions};
use std::io::IoSliceMut;
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const IOREG: &str = "/usr/sbin/ioreg";
const DISKUTIL: &str = "/usr/sbin/diskutil";
const AUTHOPEN: &str = "/usr/libexec/authopen";

/// A whole disk (not a partition) behind a USB device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    /// `disk4`
    pub bsd_name: String,
    pub size: u64,
    pub removable: bool,
    /// The disk or one of its partitions is open: a volume is mounted, or macOS is looking at it.
    pub busy: bool,
    /// The device number of its nodes in `/dev`.
    pub major: Option<u64>,
    pub minor: Option<u64>,
    /// The SCSI identity of the unit it sits on.
    pub vendor: Option<String>,
    pub model: Option<String>,
}

/// A USB device with what the storage drivers made of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    /// Where it is plugged in: the bus in the top byte, then one hex digit per hub port.
    pub location_id: u32,
    pub serial: Option<String>,
    /// SCSI vendor and model of each storage unit, with or without a disk in it (an ejected
    /// disk leaves the unit).
    pub units: Vec<(String, String)>,
    pub disks: Vec<Disk>,
}

impl UsbDevice {
    /// The port the way libusb names it on macOS, which is what rpiboot's `-p` compares with:
    /// the bus number, then the port on each hub down to the device (`1-1`, `2-1.3`).
    pub fn port(&self) -> String {
        let mut out = (self.location_id >> 24).to_string();
        let ports = (0..6).map(|i| (self.location_id >> (20 - 4 * i)) & 0xf).take_while(|p| *p != 0);
        for (i, p) in ports.enumerate() {
            out.push(if i == 0 { '-' } else { '.' });
            out.push_str(&p.to_string());
        }
        out
    }

    fn block_device(&self, d: &Disk) -> BlockDevice {
        BlockDevice {
            path: format!("/dev/{}", d.bsd_name),
            size: d.size,
            vendor: d.vendor.clone(),
            model: d.model.clone(),
            removable: d.removable,
            usb_port: Some(self.port()),
            usb_serial: self.serial.clone(),
        }
    }
}

/// The USB devices on this computer, each with its disks.
pub fn usb_devices() -> Result<Vec<UsbDevice>> {
    let out = Command::new(IOREG)
        .args(["-r", "-c", "IOUSBHostDevice", "-l", "-a"])
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("starting {IOREG}"))?;
    if !out.status.success() {
        bail!("ioreg failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    parse(&out.stdout)
}

/// Reads `ioreg -r -c IOUSBHostDevice -l -a`: a property list of the device trees. Nothing at all
/// means no USB device.
pub fn parse(xml: &[u8]) -> Result<Vec<UsbDevice>> {
    if xml.iter().all(u8::is_ascii_whitespace) {
        return Ok(Vec::new());
    }
    let root = Value::from_reader_xml(xml).context("reading the I/O Registry")?;
    let mut found = Vec::new();
    match &root {
        Value::Array(items) => items.iter().for_each(|v| walk(v, &mut found)),
        other => walk(other, &mut found),
    }
    // A device behind a hub is listed in the hub's tree and again on its own.
    let mut out: Vec<UsbDevice> = Vec::new();
    for dev in found {
        match out.iter_mut().find(|d| d.location_id == dev.location_id) {
            Some(known) if known.disks.len() + known.units.len() < dev.disks.len() + dev.units.len() => *known = dev,
            Some(_) => {}
            None => out.push(dev),
        }
    }
    out.sort_by_key(|d| d.location_id);
    Ok(out)
}

fn children(d: &Dictionary) -> impl Iterator<Item = &Value> {
    d.get("IORegistryEntryChildren").and_then(Value::as_array).into_iter().flatten()
}

fn uint(d: &Dictionary, key: &str) -> Option<u64> {
    let v = d.get(key)?;
    v.as_unsigned_integer().or_else(|| v.as_signed_integer().map(|i| i as u64))
}

fn text(d: &Dictionary, key: &str) -> Option<String> {
    d.get(key).and_then(Value::as_string).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// A device, not one of its interfaces (which repeat its ids).
fn is_usb_device(d: &Dictionary) -> bool {
    uint(d, "idVendor").is_some()
        && uint(d, "idProduct").is_some()
        && uint(d, "locationID").is_some()
        && d.get("bInterfaceNumber").is_none()
}

fn walk(node: &Value, out: &mut Vec<UsbDevice>) {
    let Some(d) = node.as_dictionary() else { return };
    if !is_usb_device(d) {
        children(d).for_each(|c| walk(c, out));
        return;
    }
    let mut dev = UsbDevice {
        vendor_id: uint(d, "idVendor").unwrap_or(0) as u16,
        product_id: uint(d, "idProduct").unwrap_or(0) as u16,
        // The registry keeps it as a signed 32-bit number.
        location_id: uint(d, "locationID").unwrap_or(0) as u32,
        serial: text(d, "USB Serial Number").or_else(|| text(d, "kUSBSerialNumberString")),
        units: Vec::new(),
        disks: Vec::new(),
    };
    for c in children(d) {
        storage(c, None, &mut dev, out);
    }
    out.push(dev);
}

/// Collects the storage units and whole disks below a USB device. `unit` is the SCSI identity
/// seen on the way down.
fn storage(node: &Value, unit: Option<&(String, String)>, dev: &mut UsbDevice, out: &mut Vec<UsbDevice>) {
    let Some(d) = node.as_dictionary() else { return };
    if is_usb_device(d) {
        // Behind a hub: a device of its own.
        walk(node, out);
        return;
    }
    // The SCSI unit says who it is; the block storage driver above it repeats that.
    let here = text(d, "Vendor Identification")
        .map(|v| (v, text(d, "Product Identification").unwrap_or_default()))
        .or_else(|| {
            let c = d.get("Device Characteristics")?.as_dictionary()?;
            Some((text(c, "Vendor Name")?, text(c, "Product Name").unwrap_or_default()))
        });
    if let Some(u) = &here
        && !dev.units.contains(u)
    {
        dev.units.push(u.clone());
    }
    let unit = here.as_ref().or(unit);
    if let Some(bsd_name) = text(d, "BSD Name")
        && d.get("Whole").and_then(Value::as_boolean) == Some(true)
    {
        let flag = |k: &str| d.get(k).and_then(Value::as_boolean) == Some(true);
        dev.disks.push(Disk {
            bsd_name,
            size: uint(d, "Size").unwrap_or(0),
            // A USB disk that says it is fixed is still "ejectable" here.
            removable: flag("Removable") || flag("Ejectable"),
            busy: any_open(d),
            major: uint(d, "BSD Major"),
            minor: uint(d, "BSD Minor"),
            vendor: unit.map(|u| u.0.clone()),
            model: unit.map(|u| u.1.clone()),
        });
        // Below a whole disk are its partitions and the disks macOS makes up from them.
        return;
    }
    for c in children(d) {
        storage(c, unit, dev, out);
    }
}

/// Whether this disk or anything on it (a partition, a disk made up from one) is open.
fn any_open(d: &Dictionary) -> bool {
    d.get("Open").and_then(Value::as_boolean) == Some(true)
        || children(d).filter_map(Value::as_dictionary).any(any_open)
}

/// The CM4's SCSI identity, as `BlockDevice::looks_like_cm4_emmc` knows it.
fn is_cm4_unit(vendor: &str, model: &str) -> bool {
    let v = vendor.to_lowercase();
    v.starts_with("mmcblk") || (v.starts_with("rpi-msd") && model.starts_with("0001"))
}

/// The disks behind USB devices, as [`crate::disk::list`] gives them.
pub fn disks() -> Result<Vec<BlockDevice>> {
    Ok(disks_of(&usb_devices()?))
}

pub fn disks_of(devices: &[UsbDevice]) -> Vec<BlockDevice> {
    let mut out: Vec<BlockDevice> =
        devices.iter().flat_map(|dev| dev.disks.iter().map(|d| dev.block_device(d))).collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// The boards on USB, as [`crate::usb::boards`] gives them.
pub fn boards() -> Vec<UsbBoard> {
    boards_of(&usb_devices().unwrap_or_default())
}

/// A Broadcom device is a board when it is a boot ROM, or when the CM4's eMMC is behind it: the
/// gadget. The gadget still counts after its disk was ejected, by the unit the disk was in.
pub fn boards_of(devices: &[UsbDevice]) -> Vec<UsbBoard> {
    let mut out: Vec<UsbBoard> = devices
        .iter()
        .filter(|d| format!("{:04x}", d.vendor_id) == BROADCOM_VID)
        .filter_map(|d| {
            let product_id = format!("{:04x}", d.product_id);
            let kind = if BOOT_PIDS.contains(&product_id.as_str()) {
                BoardKind::Boot
            } else if d.disks.iter().any(|disk| d.block_device(disk).looks_like_cm4_emmc())
                || d.units.iter().any(|(v, m)| is_cm4_unit(v, m))
            {
                BoardKind::Gadget
            } else {
                return None;
            };
            Some(UsbBoard { port: d.port(), kind, product_id, serial: d.serial.clone() })
        })
        .collect();
    out.sort_by(|a, b| a.port.cmp(&b.port));
    out
}

fn diskutil(args: &[&str]) -> Result<std::process::Output> {
    Command::new(DISKUTIL).args(args).stdin(Stdio::null()).output().with_context(|| format!("starting {DISKUTIL}"))
}

fn said(out: &std::process::Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr);
    let text = if err.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { err };
    text.trim().to_string()
}

/// How long macOS may keep a disk to itself (looking at it, checking a file system, mounting).
const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);
/// The disk must be seen free this many times, this far apart.
const SETTLE_LOOKS: u32 = 3;
const SETTLE_EVERY: Duration = Duration::from_millis(700);

/// Unmounts every volume of the disk and waits until macOS has let go of it: the raw device
/// cannot be opened while anything of the disk is in use.
///
/// One unmount is not enough. macOS mounts a disk a moment after it appears (after looking at
/// its file systems, and checking one that was not unmounted cleanly), so right after the eMMC
/// showed up there may be nothing to unmount yet, and the volume would be mounted just when the
/// password has been typed.
pub fn unmount(dev: &BlockDevice) -> Result<()> {
    let name = dev.name().to_string();
    let busy = || Ok(usb_devices()?.iter().flat_map(|d| &d.disks).find(|d| d.bsd_name == name).map(|d| d.busy));
    let unmount = || {
        let out = diskutil(&["unmountDisk", &dev.path])?;
        Ok(if out.status.success() { None } else { Some(said(&out)) })
    };
    settle(&dev.path, &busy, &unmount, SETTLE_TIMEOUT, SETTLE_EVERY)
}

/// The waiting of [`unmount`]: `busy` looks at the disk (`None`: gone), `unmount` asks macOS to
/// unmount it and says why it would not.
fn settle(
    path: &str,
    busy: &dyn Fn() -> Result<Option<bool>>,
    unmount: &dyn Fn() -> Result<Option<String>>,
    timeout: Duration,
    every: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let mut free = 0;
    let mut refused = None;
    loop {
        match busy()? {
            None => bail!("{path} went away; check the cable"),
            Some(false) => {
                free += 1;
                if free >= SETTLE_LOOKS {
                    return Ok(());
                }
            }
            Some(true) => {
                free = 0;
                refused = unmount()?;
            }
        }
        if Instant::now() > deadline {
            let why = refused.map(|w| format!(": {w}")).unwrap_or_default();
            bail!("{path} is in use on this computer and could not be unmounted{why}");
        }
        std::thread::sleep(every);
    }
}

/// Ejects the disk, so macOS does not mount the new boot partition (and write its own files to
/// it) and does not complain when the FIBER restarts. macOS looks at a disk right after it was
/// written, so the first tries may find it busy.
pub fn eject(dev: &BlockDevice) -> Result<()> {
    let mut last = String::new();
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(1));
        }
        let out = diskutil(&["eject", &dev.path])?;
        if out.status.success() {
            return Ok(());
        }
        last = said(&out);
    }
    bail!("{} could not be ejected: {last}", dev.path)
}

/// `/dev/disk4` -> `/dev/rdisk4`. Only a whole disk has such a name; a partition (`disk4s1`) or
/// anything else is refused.
fn raw_path(path: &Path) -> Result<(String, PathBuf)> {
    let name = path.strip_prefix("/dev").ok().and_then(|n| n.to_str()).unwrap_or("");
    let number = name.strip_prefix("disk").unwrap_or("");
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        bail!("{} is not a disk", path.display());
    }
    Ok((name.to_string(), PathBuf::from(format!("/dev/r{name}"))))
}

/// Opens the FIBER's eMMC for writing and reading back, through its raw device: what is written
/// goes to the disk at once and what is read comes from the disk, not from memory.
///
/// Whatever the caller checked before, this opens nothing but a CM4's eMMC: the password dialog
/// would open any disk of this computer, so the disk is looked up again here, and the handle must
/// be that disk's.
pub fn open_disk(path: &Path) -> Result<File> {
    let (name, raw) = raw_path(path)?;
    // Before the password dialog: a disk that is not a FIBER's is not even asked for.
    cm4_emmc(&name).with_context(|| format!("{} is not a FIBER's memory; not writing", path.display()))?;
    let file = match OpenOptions::new().read(true).write(true).open(&raw) {
        Ok(f) => f,
        // The usual case: the raw device belongs to root.
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => authopen(&raw)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!("{} is gone", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::ResourceBusy => {
            bail!("{} is in use on this computer (macOS mounted it again?); try again", path.display())
        }
        Err(e) => bail!("cannot open {} for writing: {e}", path.display()),
    };
    // And with the disk open, when its name can no longer go to another one.
    let emmc =
        cm4_emmc(&name).with_context(|| format!("{} went away while opening it; not writing", path.display()))?;
    let meta = file.metadata()?;
    let rdev = meta.rdev();
    // Darwin's device numbers: the major in the top byte.
    let (major, minor) = ((rdev >> 24) & 0xff, rdev & 0xff_ffff);
    if !meta.file_type().is_char_device() || emmc.major != Some(major) || emmc.minor != Some(minor) {
        bail!("{} changed while opening it; not writing", path.display());
    }
    Ok(file)
}

/// The disk of that name, if it is a CM4's eMMC behind a USB device right now.
fn cm4_emmc(name: &str) -> Result<Disk> {
    usb_devices()?
        .iter()
        .flat_map(|dev| dev.disks.iter().map(move |d| (dev, d)))
        .find(|(dev, d)| d.bsd_name == name && dev.block_device(d).looks_like_cm4_emmc())
        .map(|(_, d)| d.clone())
        .context("no FIBER has such a disk")
}

/// Asks macOS for the disk: `authopen` shows the password dialog, opens the device as root and
/// sends the open descriptor back over a socket (its `-stdoutpipe`).
fn authopen(raw: &Path) -> Result<File> {
    let (ours, theirs) = UnixStream::pair().context("making a socket for authopen")?;
    // The Command (and with it this side's copy of `theirs`) is gone at the end of the statement,
    // so the read below ends when authopen does, with or without a descriptor.
    let child = Command::new(AUTHOPEN)
        .arg("-stdoutpipe")
        .args(["-o", &libc::O_RDWR.to_string()])
        .arg(raw)
        .stdin(Stdio::null())
        .stdout(Stdio::from(OwnedFd::from(theirs)))
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting {AUTHOPEN}"))?;
    let fd = receive_fd(&ours);
    let out = child.wait_with_output().context("waiting for authopen")?;
    match fd? {
        Some(fd) => {
            // Not for the programs this app starts (diskutil).
            rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC).context("closing the disk on exec")?;
            Ok(File::from(fd))
        }
        None => {
            let why = String::from_utf8_lossy(&out.stderr);
            let why = why.trim();
            if why.to_lowercase().contains("busy") {
                bail!("{} is in use on this computer (macOS mounted it again?); try again: {why}", raw.display());
            }
            if why.is_empty() {
                bail!("writing the FIBER was not allowed (the password dialog was closed or refused)");
            }
            bail!("writing the FIBER was not allowed (the password dialog was closed or refused): {why}");
        }
    }
}

/// Waits for an open file descriptor on the socket; `None` when the other side closed it without
/// sending one.
fn receive_fd(socket: &UnixStream) -> Result<Option<OwnedFd>> {
    use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};
    loop {
        let mut data = [0u8; 64];
        let mut space = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let got = match recvmsg(socket, &mut [IoSliceMut::new(&mut data)], &mut control, RecvFlags::empty()) {
            Ok(got) => got,
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(e).context("waiting for the disk from authopen"),
        };
        let fd = control.drain().find_map(|m| match m {
            RecvAncillaryMessage::ScmRights(mut fds) => fds.next(),
            _ => None,
        });
        if fd.is_some() || got.bytes == 0 {
            return Ok(fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A FIBER running the gadget on a hub's port 3 (hub on bus 1, port 1), its eMMC as disk4;
    /// a boot ROM straight on bus 2, port 1; a memory stick; a Broadcom Bluetooth adapter.
    const TREE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<array>
  <dict>
    <key>IOObjectClass</key><string>IOUSBHostDevice</string>
    <key>IORegistryEntryID</key><integer>4294970000</integer>
    <key>idVendor</key><integer>1507</integer>
    <key>idProduct</key><integer>1552</integer>
    <key>locationID</key><integer>17825792</integer>
    <key>IORegistryEntryChildren</key>
    <array>
      <dict>
        <key>IOObjectClass</key><string>AppleUSB20Hub</string>
        <key>IORegistryEntryChildren</key>
        <array>
          <dict>
            <key>IOObjectClass</key><string>AppleUSB20HubPort</string>
            <key>locationID</key><integer>18022400</integer>
            <key>IORegistryEntryChildren</key>
            <array>
              <dict>
                <key>IOObjectClass</key><string>IOUSBHostDevice</string>
                <key>IORegistryEntryID</key><integer>4294970001</integer>
                <key>idVendor</key><integer>2652</integer>
                <key>idProduct</key><integer>260</integer>
                <key>locationID</key><integer>18022400</integer>
                <key>USB Serial Number</key><string>10000000a1b2c3d4</string>
                <key>IORegistryEntryChildren</key>
                <array>
                  <dict>
                    <key>IOObjectClass</key><string>IOUSBHostInterface</string>
                    <key>idVendor</key><integer>2652</integer>
                    <key>idProduct</key><integer>260</integer>
                    <key>locationID</key><integer>18022400</integer>
                    <key>bInterfaceNumber</key><integer>0</integer>
                    <key>IORegistryEntryChildren</key>
                    <array>
                      <dict>
                        <key>IOObjectClass</key><string>IOSCSILogicalUnitNub</string>
                        <key>Vendor Identification</key><string>mmcblk0 </string>
                        <key>Product Identification</key><string>Raspberry Pi mul</string>
                        <key>IORegistryEntryChildren</key>
                        <array>
                          <dict>
                            <key>IOObjectClass</key><string>IOBlockStorageServices</string>
                            <key>IORegistryEntryChildren</key>
                            <array>
                              <dict>
                                <key>IOObjectClass</key><string>IOMedia</string>
                                <key>BSD Name</key><string>disk4</string>
                                <key>BSD Major</key><integer>1</integer>
                                <key>BSD Minor</key><integer>16</integer>
                                <key>Whole</key><true/>
                                <key>Removable</key><true/>
                                <key>Ejectable</key><true/>
                                <key>Size</key><integer>7818182656</integer>
                                <key>IORegistryEntryChildren</key>
                                <array>
                                  <dict>
                                    <key>IOObjectClass</key><string>IOMedia</string>
                                    <key>BSD Name</key><string>disk4s1</string>
                                    <key>Open</key><true/>
                                    <key>Whole</key><false/>
                                    <key>Size</key><integer>536870912</integer>
                                    <key>IORegistryEntryChildren</key>
                                    <array>
                                      <dict>
                                        <key>IOObjectClass</key><string>AppleAPFSMedia</string>
                                        <key>BSD Name</key><string>disk5</string>
                                        <key>Whole</key><true/>
                                        <key>Size</key><integer>536870912</integer>
                                      </dict>
                                    </array>
                                  </dict>
                                </array>
                              </dict>
                            </array>
                          </dict>
                        </array>
                      </dict>
                    </array>
                  </dict>
                </array>
              </dict>
            </array>
          </dict>
        </array>
      </dict>
    </array>
  </dict>
  <dict>
    <key>IOObjectClass</key><string>IOUSBHostDevice</string>
    <key>IORegistryEntryID</key><integer>4294970001</integer>
    <key>idVendor</key><integer>2652</integer>
    <key>idProduct</key><integer>260</integer>
    <key>locationID</key><integer>18022400</integer>
    <key>USB Serial Number</key><string>10000000a1b2c3d4</string>
  </dict>
  <dict>
    <key>IOObjectClass</key><string>IOUSBHostDevice</string>
    <key>idVendor</key><integer>2652</integer>
    <key>idProduct</key><integer>10001</integer>
    <key>locationID</key><integer>34603008</integer>
  </dict>
  <dict>
    <key>IOObjectClass</key><string>IOUSBHostDevice</string>
    <key>idVendor</key><integer>1921</integer>
    <key>idProduct</key><integer>21889</integer>
    <key>locationID</key><integer>1048576</integer>
    <key>kUSBSerialNumberString</key><string>4C530001</string>
    <key>IORegistryEntryChildren</key>
    <array>
      <dict>
        <key>IOObjectClass</key><string>IOSCSILogicalUnitNub</string>
        <key>Vendor Identification</key><string>SanDisk</string>
        <key>Product Identification</key><string>Cruzer Blade</string>
        <key>IORegistryEntryChildren</key>
        <array>
          <dict>
            <key>IOObjectClass</key><string>IOMedia</string>
            <key>BSD Name</key><string>disk6</string>
            <key>BSD Major</key><integer>1</integer>
            <key>BSD Minor</key><integer>24</integer>
            <key>Open</key><false/>
            <key>Whole</key><true/>
            <key>Removable</key><true/>
            <key>Size</key><integer>15376000000</integer>
          </dict>
        </array>
      </dict>
    </array>
  </dict>
  <dict>
    <key>IOObjectClass</key><string>IOUSBHostDevice</string>
    <key>idVendor</key><integer>2652</integer>
    <key>idProduct</key><integer>22580</integer>
    <key>locationID</key><integer>-2145386496</integer>
  </dict>
</array>
</plist>"#;

    #[test]
    fn reads_the_registry() {
        let devices = parse(TREE.as_bytes()).unwrap();
        // The gadget is listed twice (in the hub's tree and on its own) and counted once, with
        // its disk.
        assert_eq!(devices.len(), 5);
        let gadget = devices.iter().find(|d| d.product_id == 0x0104).unwrap();
        assert_eq!(gadget.port(), "1-1.3");
        assert_eq!(gadget.serial.as_deref(), Some("10000000a1b2c3d4"));
        assert_eq!(gadget.units, vec![("mmcblk0".to_string(), "Raspberry Pi mul".to_string())]);
        // Its eMMC, not the partition and not the disk macOS made up from the partition.
        assert_eq!(gadget.disks.len(), 1);
        assert_eq!(gadget.disks[0].bsd_name, "disk4");
        assert_eq!((gadget.disks[0].major, gadget.disks[0].minor), (Some(1), Some(16)));
        // Its boot partition is mounted, so the disk is busy; the memory stick is not.
        assert!(gadget.disks[0].busy);
        assert!(!devices.iter().find(|d| d.product_id == 0x5581).unwrap().disks[0].busy);
        // The hub itself has no disk: the gadget's belongs to the gadget.
        assert!(devices.iter().find(|d| d.product_id == 0x0610).unwrap().disks.is_empty());
        // A location above 0x7fffffff comes as a negative number.
        assert_eq!(devices.iter().find(|d| d.product_id == 0x5834).unwrap().port(), "128-2");
    }

    #[test]
    fn finds_boards_and_their_disks() {
        let devices = parse(TREE.as_bytes()).unwrap();
        let boards = boards_of(&devices);
        // Not the hub, the memory stick or the Bluetooth adapter.
        assert_eq!(boards.len(), 2);
        assert_eq!((boards[0].port.as_str(), &boards[0].kind), ("1-1.3", &BoardKind::Gadget));
        assert_eq!(boards[0].product_id, "0104");
        assert_eq!((boards[1].port.as_str(), &boards[1].kind), ("2-1", &BoardKind::Boot));
        assert_eq!(boards[1].product_id, "2711");

        let disks = disks_of(&devices);
        assert_eq!(disks.len(), 2);
        assert_eq!(disks[0].path, "/dev/disk4");
        assert_eq!(disks[0].usb_port.as_deref(), Some("1-1.3"));
        assert_eq!(disks[0].usb_serial.as_deref(), Some("10000000a1b2c3d4"));
        assert_eq!(disks[0].size, 7_818_182_656);
        assert!(disks[0].looks_like_cm4_emmc());
        assert_eq!(disks[1].path, "/dev/disk6");
        assert!(!disks[1].looks_like_cm4_emmc());
    }

    #[test]
    fn gadget_stays_a_board_after_its_disk_was_ejected() {
        // The unit stays, with nothing in it.
        let with_media =
            "<string>IOBlockStorageServices</string>\n                            <key>IORegistryEntryChildren</key>";
        let ejected = TREE.replacen(with_media, "<string>IOBlockStorageServices</string>\n<key>Was</key>", 1);
        assert_ne!(ejected, TREE);
        let devices = parse(ejected.as_bytes()).unwrap();
        assert_eq!(disks_of(&devices).len(), 1);
        assert_eq!(disks_of(&devices)[0].path, "/dev/disk6");
        let boards = boards_of(&devices);
        assert_eq!(boards.len(), 2);
        assert_eq!((boards[0].port.as_str(), &boards[0].kind), ("1-1.3", &BoardKind::Gadget));
    }

    #[test]
    fn waits_until_macos_lets_go_of_the_disk() {
        use std::cell::{Cell, RefCell};
        let ms = Duration::from_millis;

        // Free at first (macOS has not started on it), then mounted, then free after the unmount.
        let looks = RefCell::new(vec![Some(false), Some(true), Some(true), Some(false), Some(false), Some(false)]);
        let busy = || Ok(looks.borrow_mut().remove(0));
        let unmounts = Cell::new(0);
        let unmount = || {
            unmounts.set(unmounts.get() + 1);
            Ok(None)
        };
        settle("/dev/disk4", &busy, &unmount, ms(5000), ms(1)).unwrap();
        assert_eq!(unmounts.get(), 2);
        assert!(looks.borrow().is_empty());

        // Never free: gives up, with what macOS said.
        let err = settle("/dev/disk4", &|| Ok(Some(true)), &|| Ok(Some("in use by Finder".into())), ms(20), ms(1));
        assert!(err.unwrap_err().to_string().ends_with("could not be unmounted: in use by Finder"));

        // Unplugged meanwhile.
        assert!(settle("/dev/disk4", &|| Ok(None), &unmount, ms(5000), ms(1)).is_err());
    }

    #[test]
    fn no_usb_device_is_no_output() {
        assert!(parse(b"").unwrap().is_empty());
        assert!(parse(b"\n").unwrap().is_empty());
        assert!(parse(b"not a property list").is_err());
    }

    #[test]
    fn port_names() {
        let at = |location_id| UsbDevice {
            vendor_id: 0x0a5c,
            product_id: 0x2711,
            location_id,
            serial: None,
            units: Vec::new(),
            disks: Vec::new(),
        };
        assert_eq!(at(0x0010_0000).port(), "0-1");
        assert_eq!(at(0x0110_0000).port(), "1-1");
        assert_eq!(at(0x0213_0000).port(), "2-1.3");
        assert_eq!(at(0x1412_3456).port(), "20-1.2.3.4.5.6");
        assert_eq!(at(0x0800_0000).port(), "8");
    }

    #[test]
    fn only_whole_disks_have_a_raw_device() {
        assert_eq!(raw_path(Path::new("/dev/disk4")).unwrap().1, PathBuf::from("/dev/rdisk4"));
        assert_eq!(raw_path(Path::new("/dev/disk12")).unwrap().0, "disk12");
        for bad in ["/dev/disk4s1", "/dev/rdisk4", "/dev/disk", "/dev/null", "disk4", "/tmp/disk4", "/dev/../dev/disk4"]
        {
            assert!(raw_path(Path::new(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn receives_a_descriptor_or_notices_there_is_none() {
        use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
        use std::io::{IoSlice, Read, Seek, SeekFrom, Write};
        use std::os::fd::AsFd;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sent");
        std::fs::write(&path, b"the disk").unwrap();
        let sent = File::open(&path).unwrap();
        let (ours, theirs) = UnixStream::pair().unwrap();
        let fds = [sent.as_fd()];
        let mut space = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        assert!(control.push(SendAncillaryMessage::ScmRights(&fds)));
        sendmsg(&theirs, &[IoSlice::new(b"x")], &mut control, SendFlags::empty()).unwrap();
        let mut got = File::from(receive_fd(&ours).unwrap().unwrap());
        got.seek(SeekFrom::Start(4)).unwrap();
        let mut text = String::new();
        got.read_to_string(&mut text).unwrap();
        assert_eq!(text, "disk");

        // Plain bytes and then the end: no descriptor.
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        theirs.write_all(b"no").unwrap();
        drop(theirs);
        assert!(receive_fd(&ours).unwrap().is_none());
    }
}
