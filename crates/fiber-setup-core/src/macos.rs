//! macOS: what sysfs, udev and `/proc/mounts` give on Linux.
//!
//! USB boards and the disks behind them come from the I/O Registry (`ioreg`, one call for both).
//! That call is heavy (it prints every USB device with all it carries: megabytes on a desk with
//! a dock), so the question the wizard asks every second, "is a board there?", goes to the USB
//! devices themselves first ([`boards`]), and to the registry only when a disk decides.
//! A disk is written through its raw device (`/dev/rdiskN`), which only root may open: the app
//! asks macOS for the right to open that one disk (the password or Touch ID dialog, in the app's
//! name), and macOS's `authopen` opens it with that right and hands the open disk over, so
//! nothing in this app runs as root. macOS mounts every disk it sees, so the FIBER's volumes are unmounted before writing
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
use std::sync::{Mutex, PoisonError};
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

/// A USB port's name from a device's location id, the way libusb names ports on macOS: the bus
/// number, then the port on each hub down to the device (`1-1`, `2-1.3`). A board in boot mode
/// and the gadget it becomes are on one port, so they have one name.
pub fn port_of(location_id: u32) -> String {
    let mut out = (location_id >> 24).to_string();
    let ports = (0..6).map(|i| (location_id >> (20 - 4 * i)) & 0xf).take_while(|p| *p != 0);
    for (i, p) in ports.enumerate() {
        out.push(if i == 0 { '-' } else { '.' });
        out.push_str(&p.to_string());
    }
    out
}

impl UsbDevice {
    /// The port this device is on ([`port_of`]).
    pub fn port(&self) -> String {
        port_of(self.location_id)
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

/// The disks behind USB devices, as [`crate::disk::list`] gives them. A batch has every FIBER
/// of it asking twice a second while its memory comes up, and the registry takes a fifth of a
/// second each time: an answer that young serves them all. (The checks before a disk is opened
/// read the registry themselves.)
pub fn disks() -> Result<Vec<BlockDevice>> {
    static LAST: Mutex<Option<(Instant, Vec<BlockDevice>)>> = Mutex::new(None);
    const YOUNG: Duration = Duration::from_millis(300);
    let mut last = LAST.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((at, disks)) = last.as_ref()
        && at.elapsed() < YOUNG
    {
        return Ok(disks.clone());
    }
    let disks = disks_of(&usb_devices()?);
    *last = Some((Instant::now(), disks.clone()));
    Ok(disks)
}

pub fn disks_of(devices: &[UsbDevice]) -> Vec<BlockDevice> {
    let mut out: Vec<BlockDevice> =
        devices.iter().flat_map(|dev| dev.disks.iter().map(|d| dev.block_device(d))).collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// A USB device as a look at the devices themselves gives it: cheap (well under a millisecond
/// for all of them, where the registry takes a fifth of a second), but without the disks.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    /// The I/O Registry's number for this connection: a new one each time it is plugged in.
    entry: u64,
    vendor_id: u16,
    product_id: u16,
    location_id: u32,
    serial: Option<String>,
}

fn seen() -> Option<Vec<Seen>> {
    use nusb::MaybeFuture;
    let devices = nusb::list_devices().wait().ok()?;
    Some(
        devices
            .map(|d| Seen {
                entry: d.registry_entry_id(),
                vendor_id: d.vendor_id(),
                product_id: d.product_id(),
                location_id: d.location_id(),
                serial: d.serial_number().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            })
            .collect(),
    )
}

/// How long "this Broadcom device is no board" stands before the registry is asked again: a
/// gadget's disk shows up a moment after the gadget itself.
const ASK_AGAIN: Duration = Duration::from_secs(2);

/// What the registry said about the Broadcom devices that are no boot ROM, kept for as long as
/// the same ones are connected.
struct Asked {
    /// Their connections ([`Seen::entry`]), in order.
    about: Vec<u64>,
    at: Instant,
    gadgets: Vec<UsbBoard>,
}

/// Answers "which boards are on USB?" from a look at the devices, asking the registry only when
/// it must: a boot ROM is known by its ids alone, and only a Broadcom device that is none needs
/// its disk looked at, once per connection.
struct Look {
    asked: Option<Asked>,
}

impl Look {
    fn boards(&mut self, seen: &[Seen], now: Instant, registry: impl FnOnce() -> Vec<UsbDevice>) -> Vec<UsbBoard> {
        let broadcom = seen.iter().filter(|d| format!("{:04x}", d.vendor_id) == BROADCOM_VID);
        let (roms, others): (Vec<&Seen>, Vec<&Seen>) =
            broadcom.partition(|d| BOOT_PIDS.contains(&format!("{:04x}", d.product_id).as_str()));
        let mut out: Vec<UsbBoard> = roms
            .iter()
            .map(|d| UsbBoard {
                port: port_of(d.location_id),
                kind: BoardKind::Boot,
                product_id: format!("{:04x}", d.product_id),
                serial: d.serial.clone(),
            })
            .collect();
        if others.is_empty() {
            self.asked = None;
        } else {
            let mut about: Vec<u64> = others.iter().map(|d| d.entry).collect();
            about.sort_unstable();
            // Every one of them a gadget: it stays one while it is connected (also with its disk
            // ejected). Otherwise the disk may still be on its way.
            let stands = self.asked.as_ref().is_some_and(|a| {
                a.about == about && (a.gadgets.len() == about.len() || now.duration_since(a.at) < ASK_AGAIN)
            });
            if !stands {
                let gadgets = boards_of(&registry()).into_iter().filter(|b| b.kind == BoardKind::Gadget).collect();
                self.asked = Some(Asked { about, at: now, gadgets });
            }
            out.extend(self.asked.iter().flat_map(|a| a.gadgets.iter().cloned()));
        }
        out.sort_by(|a, b| a.port.cmp(&b.port));
        out
    }
}

/// The boards on USB, as [`crate::usb::boards`] gives them. Cheap enough to ask every second:
/// see [`Look`].
pub fn boards() -> Vec<UsbBoard> {
    static LOOK: Mutex<Look> = Mutex::new(Look { asked: None });
    let registry = || usb_devices().unwrap_or_default();
    match seen() {
        Some(seen) => LOOK.lock().unwrap_or_else(PoisonError::into_inner).boards(&seen, Instant::now(), registry),
        // The devices could not be listed: the registry alone answers.
        None => boards_of(&registry()),
    }
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
    match open_disks(&[path.to_path_buf()]).pop() {
        Some(opened) => opened,
        None => bail!("{} was not opened", path.display()),
    }
}

/// [`open_disk`] for several FIBERs at once, with one password dialog for all of them: one
/// result for each path, in their order. A disk that cannot be opened does not keep the others
/// closed.
pub fn open_disks(paths: &[PathBuf]) -> Vec<Result<File>> {
    /// How far one disk got.
    enum Step {
        Failed(anyhow::Error),
        /// Checked to be a FIBER's, and to be asked for: its name and raw device.
        ToAsk(String, PathBuf),
        Open(String, File),
    }
    let mut steps: Vec<Step> = paths
        .iter()
        .map(|path| {
            let (name, raw) = match raw_path(path) {
                Ok(found) => found,
                Err(e) => return Step::Failed(e),
            };
            // Before the password dialog: a disk that is not a FIBER's is not even asked for.
            if let Err(e) = cm4_emmc(&name) {
                return Step::Failed(e.context(format!("{} is not a FIBER's memory; not writing", path.display())));
            }
            match OpenOptions::new().read(true).write(true).open(&raw) {
                Ok(f) => Step::Open(name, f),
                // The usual case: the raw device belongs to root.
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Step::ToAsk(name, raw),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    Step::Failed(anyhow::anyhow!("{} is gone", path.display()))
                }
                Err(e) if e.kind() == std::io::ErrorKind::ResourceBusy => Step::Failed(anyhow::anyhow!(
                    "{} is in use on this computer (macOS mounted it again?); try again",
                    path.display()
                )),
                Err(e) => Step::Failed(anyhow::anyhow!("cannot open {} for writing: {e}", path.display())),
            }
        })
        .collect();

    let to_ask: Vec<PathBuf> =
        steps.iter().filter_map(|s| if let Step::ToAsk(_, raw) = s { Some(raw.clone()) } else { None }).collect();
    if !to_ask.is_empty() {
        // Kept until every disk is open: the right handed to authopen is good only that long.
        let permission = ask(&to_ask);
        for step in &mut steps {
            let Step::ToAsk(name, raw) = step else { continue };
            *step = match &permission {
                Permission::Refused => Step::Failed(anyhow::anyhow!(
                    "writing the FIBER was not allowed (the password dialog was closed or refused)"
                )),
                Permission::Allowed(granted) => match authopen(raw, Some(&granted.form)) {
                    Ok(f) => Step::Open(std::mem::take(name), f),
                    Err(e) => Step::Failed(e),
                },
                Permission::Unavailable => match authopen(raw, None) {
                    Ok(f) => Step::Open(std::mem::take(name), f),
                    Err(e) => Step::Failed(e),
                },
            };
        }
    }

    steps
        .into_iter()
        .zip(paths)
        .map(|(step, path)| {
            let (name, file) = match step {
                Step::Open(name, file) => (name, file),
                Step::Failed(e) => return Err(e),
                Step::ToAsk(..) => bail!("{} was not opened", path.display()),
            };
            // And with the disk open, when its name can no longer go to another one.
            let emmc = cm4_emmc(&name)
                .with_context(|| format!("{} went away while opening it; not writing", path.display()))?;
            let meta = file.metadata()?;
            let rdev = meta.rdev();
            // Darwin's device numbers: the major in the top byte.
            let (major, minor) = ((rdev >> 24) & 0xff, rdev & 0xff_ffff);
            if !meta.file_type().is_char_device() || emmc.major != Some(major) || emmc.minor != Some(minor) {
                bail!("{} changed while opening it; not writing", path.display());
            }
            Ok(file)
        })
        .collect()
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

/// What the password dialog says of this app's own, above macOS's words ("FIBER Setup wants to
/// make changes"). The window sets it in the user's language ([`set_password_prompt`]); the
/// command line keeps the English one.
static PROMPT: Mutex<String> = Mutex::new(String::new());
const PROMPT_EN: &str = "FIBER Setup needs to write the FIBER's memory.";

/// Sets the app's own sentence in the password dialog, in the user's language.
pub fn set_password_prompt(text: &str) {
    *PROMPT.lock().unwrap_or_else(PoisonError::into_inner) = text.trim().to_string();
}

/// What came of asking macOS for the right to open the disks.
enum Permission {
    Allowed(Granted),
    /// The dialog was closed, or the password was not an administrator's.
    Refused,
    /// The right could not be asked for here at all (no session with a screen, for one).
    Unavailable,
}

/// The right to open the disks that were asked for.
struct Granted {
    /// The right in the form `authopen` takes on its standard input.
    form: [u8; 32],
    /// The right itself: the form is good only for as long as this lives.
    _authorization: security_framework::authorization::Authorization,
}

/// `errAuthorizationDenied` and `errAuthorizationCanceled` of Authorization Services.
const REFUSED: [i32; 2] = [-60005, -60006];

/// Asks macOS, as this app, for the right to open every one of `raws` for reading and writing:
/// one dialog for all of them. Asked by the app that is in front, the dialog carries its name
/// and icon, comes up in front of its window and takes Touch ID at once; asked by `authopen` it
/// would be "authopen" that wants to make changes, in a dialog that stays behind (what
/// `authopen(1)` names `-extauth` for).
fn ask(raws: &[PathBuf]) -> Permission {
    use security_framework::authorization::{Authorization, AuthorizationItemSetBuilder, Flags};
    let prompt = {
        let set = PROMPT.lock().unwrap_or_else(PoisonError::into_inner);
        if set.is_empty() { PROMPT_EN.to_string() } else { set.clone() }
    };
    let asked = || -> std::result::Result<Granted, security_framework::base::Error> {
        let mut rights = AuthorizationItemSetBuilder::new();
        for raw in raws {
            rights = rights.add_right(format!("sys.openfile.readwrite.{}", raw.display()))?;
        }
        let dialog = AuthorizationItemSetBuilder::new().add_string("prompt", prompt)?.build();
        let flags = Flags::INTERACTION_ALLOWED | Flags::EXTEND_RIGHTS | Flags::PREAUTHORIZE;
        let authorization = Authorization::new(Some(rights.build()), Some(dialog), flags)?;
        Ok(Granted { form: hand_over(&authorization)?, _authorization: authorization })
    };
    match asked() {
        Ok(granted) => Permission::Allowed(granted),
        Err(e) if REFUSED.contains(&e.code()) => Permission::Refused,
        Err(_) => Permission::Unavailable,
    }
}

/// An authorization in the form another program takes on its standard input (32 bytes).
fn hand_over(
    authorization: &security_framework::authorization::Authorization,
) -> std::result::Result<[u8; 32], security_framework::base::Error> {
    Ok(authorization.make_external_form()?.bytes.map(|b| b.to_ne_bytes()[0]))
}

/// Gets one open disk from macOS: `authopen` opens the device as root, with the `right` this app
/// asked for ([`ask`]), and sends the open descriptor back over a socket (its `-stdoutpipe`).
/// Without a right, `authopen` asks by itself.
fn authopen(raw: &Path, right: Option<&[u8; 32]>) -> Result<File> {
    let (ours, theirs) = UnixStream::pair().context("making a socket for authopen")?;
    let mut command = Command::new(AUTHOPEN);
    command.arg("-stdoutpipe");
    if right.is_some() {
        command.arg("-extauth");
    }
    // The Command (and with it this side's copy of `theirs`) is gone at the end of the statement,
    // so the read below ends when authopen does, with or without a descriptor.
    let mut child = command
        .args(["-o", &libc::O_RDWR.to_string()])
        .arg(raw)
        .stdin(if right.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::from(OwnedFd::from(theirs)))
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting {AUTHOPEN}"))?;
    drop(command);
    if let (Some(form), Some(mut stdin)) = (right, child.stdin.take()) {
        use std::io::Write;
        // authopen reads the right first; a failure here shows as "no descriptor" below.
        let _ = stdin.write_all(form);
    }
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

    /// No dialog: an authorization with no rights in it is made and put into the form `authopen`
    /// reads, which shows that Authorization Services answer here and what the form looks like.
    #[test]
    fn an_authorization_can_be_handed_to_another_program() {
        let empty = security_framework::authorization::Authorization::default().unwrap();
        let form = hand_over(&empty).unwrap();
        assert_ne!(form, [0u8; 32]);
        assert_ne!(form, hand_over(&security_framework::authorization::Authorization::default().unwrap()).unwrap());
    }

    #[test]
    fn the_dialog_says_the_sentence_the_window_set() {
        assert!(PROMPT.lock().unwrap().is_empty(), "the command line's sentence is the English one");
        set_password_prompt("  FIBER Setup potřebuje zapsat paměť FIBER.  ");
        assert_eq!(*PROMPT.lock().unwrap(), "FIBER Setup potřebuje zapsat paměť FIBER.");
        set_password_prompt("");
    }

    fn seen_at(entry: u64, product_id: u16, location_id: u32) -> Seen {
        Seen { entry, vendor_id: 0x0a5c, product_id, location_id, serial: None }
    }

    /// The gadget as the registry gives it: its storage unit, with or without a disk in it.
    fn gadget_at(location_id: u32, with_unit: bool) -> UsbDevice {
        UsbDevice {
            vendor_id: 0x0a5c,
            product_id: 0x0104,
            location_id,
            serial: Some("10000000a1b2c3d4".into()),
            units: if with_unit { vec![("mmcblk0".into(), "Raspberry Pi mul".into())] } else { Vec::new() },
            disks: Vec::new(),
        }
    }

    /// Waiting for a board, and a board in boot mode: the wizard asks every second, and the
    /// registry (a fifth of a second of work each time) must not be read for it.
    #[test]
    fn a_boot_rom_is_found_without_reading_the_registry() {
        let mut look = Look { asked: None };
        let never = || -> Vec<UsbDevice> { panic!("the registry was read") };
        let now = Instant::now();
        // Nothing of Broadcom's: a keyboard.
        let keyboard = Seen { entry: 1, vendor_id: 0x05ac, product_id: 0x0342, location_id: 0x0120_0000, serial: None };
        assert_eq!(look.boards(std::slice::from_ref(&keyboard), now, never), []);
        let rom = Seen { serial: Some("a1b2c3d4".into()), ..seen_at(2, 0x2711, 0x0210_0000) };
        let boards = look.boards(&[keyboard, rom], now, never);
        assert_eq!(
            boards,
            [UsbBoard {
                port: "2-1".into(),
                kind: BoardKind::Boot,
                product_id: "2711".into(),
                serial: Some("a1b2c3d4".into())
            }]
        );
    }

    #[test]
    fn the_registry_is_read_once_for_a_gadget_and_again_when_it_is_plugged_in_anew() {
        let mut look = Look { asked: None };
        let reads = std::cell::Cell::new(0);
        let registry = || {
            reads.set(reads.get() + 1);
            vec![gadget_at(0x0113_0000, true)]
        };
        let start = Instant::now();
        for second in 0..60 {
            let boards = look.boards(&[seen_at(7, 0x0104, 0x0113_0000)], start + Duration::from_secs(second), registry);
            assert_eq!(boards.len(), 1);
            assert_eq!((boards[0].port.as_str(), &boards[0].kind), ("1-1.3", &BoardKind::Gadget));
        }
        assert_eq!(reads.get(), 1);
        // RESET with the jumper still on BOOT: the gadget goes, the boot ROM comes on the same port.
        let boards = look.boards(&[seen_at(8, 0x2711, 0x0113_0000)], start + Duration::from_secs(60), registry);
        assert_eq!((boards.len(), &boards[0].kind), (1, &BoardKind::Boot));
        assert_eq!(reads.get(), 1);
        // Started over USB again: a new connection, looked at anew.
        let boards = look.boards(&[seen_at(9, 0x0104, 0x0113_0000)], start + Duration::from_secs(61), registry);
        assert_eq!((boards.len(), &boards[0].kind), (1, &BoardKind::Gadget));
        assert_eq!(reads.get(), 2);
        // Both at once: a second FIBER in boot mode beside the gadget.
        let both = [seen_at(9, 0x0104, 0x0113_0000), seen_at(10, 0x2711, 0x0210_0000)];
        let boards = look.boards(&both, start + Duration::from_secs(62), registry);
        assert_eq!(
            boards.iter().map(|b| (b.port.as_str(), &b.kind)).collect::<Vec<_>>(),
            [("1-1.3", &BoardKind::Gadget), ("2-1", &BoardKind::Boot)]
        );
        assert_eq!(reads.get(), 2);
    }

    /// A gadget's disk comes a moment after the gadget; a Broadcom device that never gets one
    /// (a Bluetooth adapter) must not cost a reading every second.
    #[test]
    fn a_broadcom_device_without_a_disk_is_asked_about_again_only_after_a_while() {
        let mut look = Look { asked: None };
        let reads = std::cell::Cell::new(0);
        let has_unit = std::cell::Cell::new(false);
        let registry = || {
            reads.set(reads.get() + 1);
            vec![gadget_at(0x0113_0000, has_unit.get())]
        };
        let seen = [seen_at(7, 0x0104, 0x0113_0000)];
        let start = Instant::now();
        assert_eq!(look.boards(&seen, start, registry), []);
        assert_eq!(look.boards(&seen, start + Duration::from_secs(1), registry), []);
        assert_eq!(reads.get(), 1);
        has_unit.set(true);
        assert_eq!(look.boards(&seen, start + ASK_AGAIN, registry).len(), 1);
        assert_eq!(reads.get(), 2);
        assert_eq!(look.boards(&seen, start + ASK_AGAIN * 10, registry).len(), 1);
        assert_eq!(reads.get(), 2);
    }

    /// The cheap look and the registry must tell the same boards, with whatever is connected to
    /// this computer now.
    #[test]
    fn the_cheap_look_and_the_registry_tell_the_same_boards() {
        assert_eq!(boards(), boards_of(&usb_devices().unwrap()));
    }

    /// The two ways this app looks at USB must name a port alike: the board list comes from
    /// the I/O Registry, the device to start is found through `nusb`. With whatever is
    /// connected to this computer now.
    #[test]
    fn both_views_of_usb_name_the_ports_alike() {
        use nusb::MaybeFuture;
        let ours: Vec<(u16, u16, String)> =
            usb_devices().unwrap().iter().map(|d| (d.vendor_id, d.product_id, d.port())).collect();
        for device in nusb::list_devices().wait().unwrap() {
            let port = crate::usb::port_of(&device).unwrap();
            let seen = (device.vendor_id(), device.product_id(), port);
            assert!(ours.contains(&seen), "{seen:x?} is not among {ours:x?}");
        }
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
