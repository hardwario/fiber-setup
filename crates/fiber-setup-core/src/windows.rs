//! Windows: what sysfs, udev and `/proc/mounts` give on Linux.
//!
//! The boards on USB come from Windows' device list (the `nusb` crate), the disks and the USB
//! device each one hangs on from its storage management (PowerShell). The boot ROM needs a
//! driver before rpiboot can talk to it: a WinUSB driver package of ours (`driver/`, next to
//! rpiboot), installed with `pnputil`. Writing a disk needs administrator rights, which the app
//! asks for when it starts.
//!
//! The tools of Windows are started by their full paths under `%SystemRoot%\System32`: this
//! runs as administrator, and must not start whatever else is called `powershell.exe`.

use crate::disk::BlockDevice;
use crate::usb::{BOOT_PIDS, BROADCOM_VID, BoardKind, UsbBoard};
use anyhow::{Context, Result, bail};
use nusb::MaybeFuture;
use serde::Deserialize;
use std::fs::{File, OpenOptions};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

/// No console window flashing up behind the app for each tool it starts.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// The driver package's file, by which it is found in Windows' driver store.
pub const DRIVER_INF: &str = "fiber-usbboot.inf";
/// USB mass storage: the interface class of the gadget's disk.
const MASS_STORAGE: u8 = 0x08;

fn system_tool(relative: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32").join(relative)
}

/// A tool to start without a console window.
pub(crate) fn hidden(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null()).creation_flags(CREATE_NO_WINDOW);
    cmd
}

fn said(out: &std::process::Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr);
    let text = if err.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { err };
    text.trim().to_string()
}

/// Runs a script in Windows PowerShell (5.1, part of every Windows 10 and 11) and gives what it
/// printed. `vars` reach the script as environment variables, so nothing has to be quoted into
/// its text.
fn powershell(script: &str, vars: &[(&str, &str)]) -> Result<String> {
    let mut cmd = hidden(&system_tool(r"WindowsPowerShell\v1.0\powershell.exe"));
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script]);
    for (name, value) in vars {
        cmd.env(name, value);
    }
    let out = cmd.output().context("starting PowerShell")?;
    if !out.status.success() {
        bail!("{}", said(&out));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Every disk with what identifies it, and for a USB disk the USB device it hangs on: up the
/// device tree from the disk to the first node that is a device (`USB\VID_..&PID_..\..`), not
/// one of its interfaces (`..&MI_00\..`). Single quotes only: the script travels as one
/// command line argument.
const PROBE: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$out = @()
foreach ($d in Get-Disk) {
  $usb = $null
  $location = $null
  $bus = [string]$d.BusType
  if ($bus -eq 'USB' -and $d.Path -match '^\\\\\?\\(.+)#\{[0-9A-Fa-f-]+\}$') {
    $id = $Matches[1] -replace '#', '\'
    for ($i = 0; $i -lt 5 -and $id; $i++) {
      $parent = Get-PnpDeviceProperty -InstanceId $id -KeyName 'DEVPKEY_Device_Parent' -ErrorAction SilentlyContinue
      $id = if ($parent) { [string]$parent.Data } else { $null }
      if ($id -match '^USB\\VID_[0-9A-Fa-f]{4}&PID_[0-9A-Fa-f]{4}\\') {
        $usb = $id
        $paths = Get-PnpDeviceProperty -InstanceId $id -KeyName 'DEVPKEY_Device_LocationPaths' -ErrorAction SilentlyContinue
        if ($paths -and $paths.Data) { $location = [string]@($paths.Data)[0] }
        break
      }
    }
  }
  $out += [pscustomobject]@{
    number = [int]$d.Number
    path = [string]$d.Path
    size = [uint64]$d.Size
    bus = $bus
    boot = [bool]$d.IsBoot
    system = [bool]$d.IsSystem
    vendor = ([string]$d.Manufacturer).Trim()
    model = ([string]$d.Model).Trim()
    usb = $usb
    location = $location
  }
}
ConvertTo-Json -InputObject @($out) -Compress
"#;

/// Removes the partition table of the disk at `$env:FIBER_SETUP_DISK` (its device path, which
/// names its vendor and serial number), and with it the volumes Windows mounted from it. The
/// disk is looked up by that path and checked again right here, where its number is read, so
/// the number given to diskpart is that disk's.
const CLEAN: &str = r#"
$ErrorActionPreference = 'Stop'
$d = Get-Disk | Where-Object { $_.Path -eq $env:FIBER_SETUP_DISK }
if (-not $d) { throw 'the disk is gone' }
if (@($d).Count -ne 1) { throw 'more than one disk has that path' }
if ([string]$d.BusType -ne $env:FIBER_SETUP_BUS -or $d.IsBoot -or $d.IsSystem) { throw 'not a disk to clean' }
$script = Join-Path $env:TEMP ('fiber-setup-' + [guid]::NewGuid().ToString() + '.txt')
Set-Content -LiteralPath $script -Encoding ASCII -Value @('select disk ' + $d.Number, 'clean')
try {
  $said = & (Join-Path $env:SystemRoot 'System32\diskpart.exe') /s $script | Out-String
  if ($LASTEXITCODE -ne 0) { throw ('diskpart: ' + $said.Trim()) }
} finally {
  Remove-Item -LiteralPath $script -ErrorAction SilentlyContinue
}
"#;

/// A disk as the probe reports it.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Probe {
    /// N of `\\.\PhysicalDriveN`.
    pub number: u32,
    /// The device path: `\\?\usbstor#disk&ven_..&prod_..&rev_..#<serial>&0#{..}`. It opens the
    /// same disk as its number does, and cannot come to mean another one.
    pub path: String,
    pub size: u64,
    /// `USB`, `NVMe`, `SATA`, `File Backed Virtual`, ...
    pub bus: String,
    pub boot: bool,
    pub system: bool,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub model: String,
    /// The USB device it hangs on: `USB\VID_0A5C&PID_0104\<serial>`.
    #[serde(default)]
    pub usb: Option<String>,
    /// Where that device is plugged in: `PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(2)`.
    #[serde(default)]
    pub location: Option<String>,
}

impl Probe {
    fn is_usb(&self) -> bool {
        // Its name, or its number where Windows gives that (the storage API's BusType 7).
        self.bus.eq_ignore_ascii_case("USB") || self.bus == "7"
    }

    fn block_device(&self) -> BlockDevice {
        // The SCSI identity as usbstor writes it into the path, the form the FIBER is known by;
        // what the disk says of itself otherwise.
        let (vendor, model) = usbstor_identity(&self.path).unwrap_or_else(|| (self.vendor.clone(), self.model.clone()));
        let some = |s: String| Some(s).filter(|s| !s.is_empty());
        BlockDevice {
            path: format!(r"\\.\PhysicalDrive{}", self.number),
            size: self.size,
            vendor: some(vendor),
            model: some(model),
            // Never the disk Windows runs from, whatever it is connected by.
            removable: self.is_usb() && !self.boot && !self.system,
            usb_port: self.location.as_deref().and_then(port_name),
            usb_serial: self.usb.as_deref().and_then(usb_serial),
        }
    }
}

/// Reads what the probe printed. Nothing at all is no disk.
pub(crate) fn parse_probe(json: &str) -> Result<Vec<Probe>> {
    let json = json.trim_start_matches('\u{feff}').trim();
    if json.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(json).context("reading the list of disks")
}

pub(crate) fn probe() -> Result<Vec<Probe>> {
    parse_probe(&powershell(PROBE, &[]).context("listing the disks")?)
}

/// SCSI vendor and model from a USB disk's device path:
/// `\\?\usbstor#disk&ven_mmcblk0&prod_raspberry_pi_mul&rev_0001#..` gives `mmcblk0` and
/// `raspberry pi mul` (usbstor writes spaces as underscores).
fn usbstor_identity(path: &str) -> Option<(String, String)> {
    let lower = path.to_ascii_lowercase();
    let hardware = lower.split('#').skip_while(|part| !part.ends_with("usbstor")).nth(1)?;
    let field = |prefix: &str| {
        hardware.split('&').find_map(|f| f.strip_prefix(prefix)).map(|v| v.replace('_', " ").trim().to_string())
    };
    Some((field("ven_")?, field("prod_").unwrap_or_default()))
}

/// A short name for where a USB device is plugged in, from its location path:
/// `PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(2)#USB(1)` is `1400-2.1` (the controller, then the port
/// on each hub). A board in boot mode and the gadget it becomes are on one port, so they have
/// one name; an interface of a device (`..#USBMI(0)`) has its device's.
pub fn port_name(location: &str) -> Option<String> {
    let mut controller: Vec<&str> = Vec::new();
    let mut ports: Vec<&str> = Vec::new();
    let mut on_usb = false;
    for part in location.split('#') {
        let (name, rest) = part.split_once('(')?;
        let value = rest.strip_suffix(')')?;
        match name.to_ascii_uppercase().as_str() {
            "USBROOT" => on_usb = true,
            "USB" if on_usb => ports.push(value),
            "USBMI" => break,
            "PCIROOT" => {}
            _ if !on_usb => controller.push(value),
            _ => return None,
        }
    }
    if !on_usb || ports.is_empty() {
        return None;
    }
    Some(format!("{}-{}", controller.join("."), ports.join(".")))
}

/// The serial number in a USB device's instance ID (`USB\VID_0A5C&PID_0104\10000000A1B2C3D4`),
/// in lower case as the CM4 prints it. A device without one gets an ID Windows makes up, which
/// has `&` in it.
fn usb_serial(instance_id: &str) -> Option<String> {
    let last = instance_id.rsplit('\\').next()?;
    (!last.is_empty() && !last.contains('&')).then(|| last.to_ascii_lowercase())
}

/// The disks on this computer, as [`crate::disk::list`] gives them.
pub fn disks() -> Result<Vec<BlockDevice>> {
    let mut out: Vec<BlockDevice> = probe()?.iter().map(Probe::block_device).collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The boards on USB, as [`crate::usb::boards`] gives them. A Broadcom device is a board when it
/// is a boot ROM, or when it carries a disk: the gadget. (Broadcom's Bluetooth adapters carry
/// none.)
pub fn boards() -> Vec<UsbBoard> {
    let Ok(devices) = nusb::list_devices().wait() else { return Vec::new() };
    let mut out: Vec<UsbBoard> = devices
        .filter(|d| format!("{:04x}", d.vendor_id()) == BROADCOM_VID)
        .filter_map(|d| {
            let product_id = format!("{:04x}", d.product_id());
            let storage = d.driver().is_some_and(|driver| driver.eq_ignore_ascii_case("USBSTOR"))
                || d.interfaces().any(|i| i.class() == MASS_STORAGE);
            let kind = if BOOT_PIDS.contains(&product_id.as_str()) {
                BoardKind::Boot
            } else if storage {
                BoardKind::Gadget
            } else {
                return None;
            };
            let port = d.location_paths().iter().find_map(|p| port_name(&p.to_string_lossy()))?;
            Some(UsbBoard { port, kind, product_id, serial: d.serial_number().map(str::to_ascii_lowercase) })
        })
        .collect();
    out.sort_by(|a, b| a.port.cmp(&b.port));
    out
}

/// The disk `\\.\PhysicalDriveN` as the probe sees it now, if it is a CM4's eMMC on USB.
fn cm4_emmc(path: &Path) -> Result<Probe> {
    let text = path.to_string_lossy();
    let number: u32 = text
        .strip_prefix(r"\\.\PhysicalDrive")
        .and_then(|n| n.parse().ok())
        .with_context(|| format!("{text} is not a disk"))?;
    probe()?
        .into_iter()
        .find(|p| p.number == number && p.is_usb() && p.block_device().looks_like_cm4_emmc())
        .with_context(|| format!("{text} is not a FIBER's memory (any more)"))
}

/// Windows mounts the volumes it finds on the eMMC and writes into none it has mounted, so the
/// old partition table goes first, and its volumes with it. (The write wipes it first anyway.)
pub fn clean(dev: &BlockDevice) -> Result<()> {
    let disk = cm4_emmc(Path::new(&dev.path)).with_context(|| format!("{}: not cleaning it", dev.path))?;
    clean_disk(&disk.path, &disk.bus).with_context(|| format!("removing the old partitions of {}", dev.path))
}

/// `bus` is the kind of disk it must still be, as the probe named it: USB for a FIBER.
pub(crate) fn clean_disk(device_path: &str, bus: &str) -> Result<()> {
    powershell(CLEAN, &[("FIBER_SETUP_DISK", device_path), ("FIBER_SETUP_BUS", bus)]).map(drop)
}

/// Opens the FIBER's eMMC for writing and reading back. A disk is no file to Windows: what is
/// written goes to it, what is read comes from it, with no cache in between.
///
/// Whatever the caller checked before, this opens nothing but a CM4's eMMC: the disk is looked
/// up again here, and opened by its device path (which names its vendor and serial number),
/// not by a number that could come to mean another disk.
pub fn open_disk(path: &Path) -> Result<File> {
    let disk = cm4_emmc(path).with_context(|| format!("{}: not writing it", path.display()))?;
    OpenOptions::new().read(true).write(true).open(&disk.path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            anyhow::anyhow!("no permission to write {} (FIBER Setup must run as administrator)", path.display())
        } else {
            anyhow::anyhow!("cannot open {} for writing: {e}", path.display())
        }
    })
}

/// What this computer is set to, for the settings the wizard starts with.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Regional {
    /// The language of Windows itself: `cs-CZ`.
    #[serde(default)]
    pub language: Option<String>,
    /// The country, two letters: `CZ`.
    #[serde(default)]
    pub region: Option<String>,
    /// The time zone by its IANA name (`Europe/Prague`), which is what the FIBER wants.
    #[serde(default)]
    pub timezone: Option<String>,
}

const REGIONAL: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$timezone = $null
try {
  [void][Windows.Globalization.Calendar, Windows.Globalization, ContentType = WindowsRuntime]
  $timezone = (New-Object Windows.Globalization.Calendar).GetTimeZone()
} catch {}
ConvertTo-Json -Compress -InputObject @{
  language = (Get-UICulture).Name
  region = [System.Globalization.RegionInfo]::CurrentRegion.TwoLetterISORegionName
  timezone = $timezone
}
"#;

/// Language, country and time zone of this computer; whatever cannot be found out is left out.
pub fn regional() -> Regional {
    powershell(REGIONAL, &[])
        .ok()
        .and_then(|json| serde_json::from_str(json.trim_start_matches('\u{feff}').trim()).ok())
        .unwrap_or_default()
}

/// Whether the driver package for the boot ROM is in Windows' driver store. Once it is, it
/// stays; before that it is looked for each time.
pub fn driver_installed() -> bool {
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    if INSTALLED.load(Ordering::Relaxed) {
        return true;
    }
    // The list's labels are in Windows' language; the file name is not.
    let found = hidden(&system_tool("pnputil.exe"))
        .arg("/enum-drivers")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).to_ascii_lowercase().contains(DRIVER_INF));
    if found {
        INSTALLED.store(true, Ordering::Relaxed);
    }
    found
}

/// Adds the driver package to Windows' driver store and installs it on a boot ROM that is
/// connected. Windows asks once whether to trust software from its publisher.
pub fn install_driver(inf: &Path) -> Result<()> {
    if !inf.is_file() {
        bail!("the USB driver is missing from this installation ({})", inf.display());
    }
    let out = hidden(&system_tool("pnputil.exe"))
        .arg("/add-driver")
        .arg(inf)
        .arg("/install")
        .output()
        .context("starting pnputil")?;
    match out.status.code() {
        // Done; done, with no board connected to install it on; done, wants a restart.
        Some(0) | Some(259) | Some(3010) => Ok(()),
        _ => bail!("installing the USB driver failed: {}", said(&out)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A laptop's own disk, a FIBER running the gadget, a memory stick.
    const DISKS: &str = r#"[
      {"number":0,"path":"\\\\?\\scsi#disk&ven_nvme&prod_samsung_ssd_980#5&2a8b&0&000000#{53f56307-b6bf-11d0-94f2-00a0c91efb8b}",
       "size":512110190592,"bus":"NVMe","boot":true,"system":true,"vendor":"NVMe","model":"Samsung SSD 980","usb":null,"location":null},
      {"number":2,"path":"\\\\?\\usbstor#disk&ven_mmcblk0&prod_raspberry_pi_mul&rev_0001#10000000a1b2c3d4&0#{53f56307-b6bf-11d0-94f2-00a0c91efb8b}",
       "size":7818182656,"bus":"USB","boot":false,"system":false,"vendor":"mmcblk0","model":"Raspberry Pi mul",
       "usb":"USB\\VID_0A5C&PID_0104\\10000000A1B2C3D4","location":"PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(2)#USB(1)"},
      {"number":1,"path":"\\\\?\\usbstor#disk&ven_sandisk&prod_cruzer_blade&rev_1.00#4c530001&0#{53f56307-b6bf-11d0-94f2-00a0c91efb8b}",
       "size":15376000000,"bus":"USB","boot":false,"system":false,"vendor":"SanDisk","model":"Cruzer Blade",
       "usb":"USB\\VID_0781&PID_5567\\4C530001","location":"PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(5)"}
    ]"#;

    #[test]
    fn reads_the_disks() {
        let disks: Vec<BlockDevice> = parse_probe(DISKS).unwrap().iter().map(Probe::block_device).collect();
        assert_eq!(disks.len(), 3);
        let emmc = disks.iter().find(|d| d.path == r"\\.\PhysicalDrive2").unwrap();
        assert!(emmc.looks_like_cm4_emmc());
        assert_eq!(emmc.vendor.as_deref(), Some("mmcblk0"));
        assert_eq!(emmc.usb_port.as_deref(), Some("1400-2.1"));
        assert_eq!(emmc.usb_serial.as_deref(), Some("10000000a1b2c3d4"));
        assert_eq!(emmc.size, 7_818_182_656);
        // Not the memory stick, and not the disk Windows runs from.
        assert!(!disks.iter().find(|d| d.path == r"\\.\PhysicalDrive1").unwrap().looks_like_cm4_emmc());
        let own = disks.iter().find(|d| d.path == r"\\.\PhysicalDrive0").unwrap();
        assert!(!own.removable && !own.looks_like_cm4_emmc());
        assert!(own.usb_port.is_none());
    }

    #[test]
    fn a_boot_or_system_disk_is_never_a_fiber() {
        let fake = DISKS.replace(
            r#""bus":"USB","boot":false,"system":false,"vendor":"mmcblk0""#,
            r#""bus":"USB","boot":true,"system":false,"vendor":"mmcblk0""#,
        );
        assert_ne!(fake, DISKS);
        let disks: Vec<BlockDevice> = parse_probe(&fake).unwrap().iter().map(Probe::block_device).collect();
        assert!(disks.iter().all(|d| !d.looks_like_cm4_emmc()));
    }

    #[test]
    fn no_output_is_no_disk() {
        assert!(parse_probe("").unwrap().is_empty());
        assert!(parse_probe("\u{feff}[]\r\n").unwrap().is_empty());
        assert!(parse_probe("[\r\n\r\n]").unwrap().is_empty());
        assert!(parse_probe("Get-Disk : Access denied").is_err());
    }

    #[test]
    fn identity_from_the_device_path() {
        let id = |p: &str| usbstor_identity(p);
        assert_eq!(
            id(
                r"\\?\usbstor#disk&ven_mmcblk0&prod_raspberry_pi_mul&rev_0001#10000000a1b2c3d4&0#{53f56307-b6bf-11d0-94f2-00a0c91efb8b}"
            ),
            Some(("mmcblk0".into(), "raspberry pi mul".into()))
        );
        assert_eq!(
            id(r"\\?\USBSTOR#Disk&Ven_RPi-MSD-&Prod_0001&Rev_#0001&0#{53f56307-b6bf-11d0-94f2-00a0c91efb8b}"),
            Some(("rpi-msd-".into(), "0001".into()))
        );
        assert_eq!(id(r"\\?\scsi#disk&ven_nvme&prod_samsung#5&2a8b&0#{53f56307-b6bf-11d0-94f2-00a0c91efb8b}"), None);
    }

    #[test]
    fn port_names() {
        assert_eq!(port_name("PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(2)").as_deref(), Some("1400-2"));
        assert_eq!(port_name("PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(2)#USB(1)").as_deref(), Some("1400-2.1"));
        // An interface of a device on that port is on that port.
        assert_eq!(port_name("PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(2)#USBMI(0)").as_deref(), Some("1400-2"));
        assert_eq!(port_name("PCIROOT(0)#PCI(1C04)#PCI(0000)#USBROOT(0)#USB(7)").as_deref(), Some("1C04.0000-7"));
        assert_eq!(port_name("ACPI(_SB_)#ACPI(USB0)#USBROOT(0)#USB(1)").as_deref(), Some("_SB_.USB0-1"));
        assert_eq!(port_name("PCIROOT(0)#PCI(1400)#USBROOT(0)"), None);
        assert_eq!(port_name("PCIROOT(0)#PCI(1700)"), None);
        assert_eq!(port_name(""), None);
    }

    #[test]
    fn serial_numbers() {
        assert_eq!(usb_serial(r"USB\VID_0A5C&PID_0104\10000000A1B2C3D4").as_deref(), Some("10000000a1b2c3d4"));
        assert_eq!(usb_serial(r"USB\VID_0A5C&PID_2711\5&2a9d6f4&0&2"), None);
    }

    /// The probe as Windows really answers it: its own disk is there, and is told apart.
    #[test]
    fn lists_this_computers_disks() {
        let disks = probe().unwrap();
        assert!(!disks.is_empty());
        assert!(disks.iter().any(|d| d.boot || d.system), "{disks:?}");
        assert!(disks.iter().all(|d| d.size > 0 && d.path.starts_with(r"\\?\")), "{disks:?}");
        // No FIBER is connected to a test machine.
        assert!(super::disks().unwrap().iter().all(|d| !d.looks_like_cm4_emmc()));
        // Listing the boards must work with none there.
        assert!(boards().iter().all(|b| !b.port.is_empty()));
        // And asking for the driver must not fail, whatever the answer.
        let _ = driver_installed();
        // Windows knows its own language and country.
        let regional = regional();
        assert!(regional.language.is_some_and(|l| l.len() >= 2), "no language");
        assert!(regional.region.is_some_and(|r| r.len() == 2), "no region");
    }
}
