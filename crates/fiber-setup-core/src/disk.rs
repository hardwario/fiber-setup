//! Block devices: finding the FIBER's eMMC, and writing and reading it back.
//!
//! Only a disk that rpiboot exposed on the FIBER's own USB port, with the CM4 gadget's SCSI
//! identity, is ever written (`flash.rs`). The probe here is the Provisioning Station's
//! (`proximos/provisioning`, `rpiboot.rs`), cut down.

use crate::event::{Cancel, Event, Phase, Sink, Throttle};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const CHUNK: usize = 4 << 20;
const ALIGN: usize = 4096;
/// How much is written between flushes to the eMMC.
const SYNC_EVERY: u64 = 64 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockDevice {
    pub path: String,
    pub size: u64,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub removable: bool,
    /// The USB path of the port the disk hangs on (`3-2`).
    pub usb_port: Option<String>,
    /// For the CM4 gadget: the CM4's serial number.
    pub usb_serial: Option<String>,
}

impl BlockDevice {
    /// The CM4 eMMC as rpiboot exposes it: a removable USB disk with SCSI vendor `mmcblk0` (the
    /// mass-storage gadget) or `RPi-MSD-` model `0001` (the boot ROM's own mass storage).
    pub fn looks_like_cm4_emmc(&self) -> bool {
        let v = self.vendor.as_deref().unwrap_or("").to_lowercase();
        let m = self.model.as_deref().unwrap_or("");
        self.removable && (v.starts_with("mmcblk") || (v.starts_with("rpi-msd") && m.starts_with("0001")))
    }

    pub fn name(&self) -> &str {
        self.path.trim_start_matches("/dev/")
    }
}

pub fn list() -> std::io::Result<Vec<BlockDevice>> {
    list_in(Path::new("/sys/block"))
}

pub fn list_in(root: &Path) -> std::io::Result<Vec<BlockDevice>> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if ["loop", "ram", "dm-", "zram", "sr"].iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let base = entry.path();
        let read = |f: &str| {
            std::fs::read_to_string(base.join(f)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        };
        let Some(sectors) = read("size").and_then(|s| s.parse::<u64>().ok()) else { continue };
        let (usb_serial, usb_port) = usb_identity(&base);
        out.push(BlockDevice {
            path: format!("/dev/{name}"),
            size: sectors.saturating_mul(512),
            vendor: read("device/vendor"),
            model: read("device/model"),
            removable: read("removable").as_deref() == Some("1"),
            usb_port,
            usb_serial,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Walks up from `/sys/block/<dev>/device` to the USB device; its directory is named after the
/// port.
fn usb_identity(block: &Path) -> (Option<String>, Option<String>) {
    let Ok(mut dir) = std::fs::canonicalize(block.join("device")) else { return (None, None) };
    for _ in 0..10 {
        if dir.join("idVendor").is_file() {
            let serial = std::fs::read_to_string(dir.join("serial"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            return (serial, dir.file_name().map(|n| n.to_string_lossy().into_owned()));
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    (None, None)
}

/// Whether the disk or one of its partitions is mounted.
pub fn is_mounted(dev: &BlockDevice) -> bool {
    let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else { return false };
    let prefix = dev.path.as_str();
    mounts.lines().filter_map(|l| l.split_whitespace().next()).any(|src| {
        src == prefix
            || src.strip_prefix(prefix).is_some_and(|rest| {
                let rest = rest.strip_prefix('p').unwrap_or(rest);
                !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
            })
    })
}

/// What goes where: the raw image, with the boot partition's bytes replaced by the personalised
/// copy.
pub struct Source<'a> {
    pub raw: &'a Path,
    pub size: u64,
    pub patch_offset: u64,
    pub patch: &'a [u8],
}

impl Source<'_> {
    /// Fills `buf` with the image bytes at `offset`, patched.
    fn read_at(&self, file: &mut File, offset: u64, buf: &mut [u8]) -> Result<()> {
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buf).with_context(|| format!("reading the image at {offset}"))?;
        let start = offset.max(self.patch_offset);
        let end = (offset + buf.len() as u64).min(self.patch_offset + self.patch.len() as u64);
        if start < end {
            let b = (start - offset) as usize;
            let p = (start - self.patch_offset) as usize;
            let len = (end - start) as usize;
            buf[b..b + len].copy_from_slice(&self.patch[p..p + len]);
        }
        Ok(())
    }

    fn chunks(&self) -> u64 {
        self.size.div_ceil(CHUNK as u64)
    }
}

/// Where to write: the eMMC, or a plain file for testing without a FIBER.
pub enum Target {
    Device(PathBuf),
    File(PathBuf),
}

impl Target {
    pub fn path(&self) -> &Path {
        match self {
            Target::Device(p) | Target::File(p) => p,
        }
    }

    fn open_write(&self) -> Result<File> {
        match self {
            Target::Device(p) => {
                let meta = std::fs::metadata(p).with_context(|| format!("{} is gone", p.display()))?;
                if !meta.file_type().is_block_device() {
                    bail!("{} is not a disk", p.display());
                }
                // O_EXCL on a block device fails while it (or a partition) is mounted or in use.
                let f = OpenOptions::new().write(true).custom_flags(libc::O_EXCL).open(p).map_err(|e| {
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        anyhow::anyhow!("no permission to write {} (allow USB access first)", p.display())
                    } else {
                        anyhow::anyhow!("cannot open {} for writing: {e}", p.display())
                    }
                })?;
                // The device node opened must still be the disk that was checked: compare its
                // major:minor with the kernel's record for that name.
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let sys = std::fs::read_to_string(Path::new("/sys/block").join(&name).join("dev")).unwrap_or_default();
                let rdev = f.metadata()?.rdev();
                if sys.trim() != format!("{}:{}", dev_major(rdev), dev_minor(rdev)) {
                    bail!("{} changed while opening it; not writing", p.display());
                }
                Ok(f)
            }
            Target::File(p) => {
                // The CLI's --to-file must never become a way to write a disk.
                if let Ok(meta) = std::fs::symlink_metadata(p)
                    && !meta.file_type().is_file()
                {
                    bail!("{} exists and is not a regular file", p.display());
                }
                Ok(OpenOptions::new().write(true).create(true).truncate(true).open(p)?)
            }
        }
    }
}

/// glibc's encoding of a device number.
fn dev_major(dev: u64) -> u64 {
    ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)
}

fn dev_minor(dev: u64) -> u64 {
    (dev & 0xff) | ((dev >> 12) & !0xff)
}

/// Writes the image so that a stop half way never leaves a disk that boots: the old partition
/// table is wiped and flushed first, then everything but the first chunk is written, and the
/// first chunk (with the new partition table) goes last. Flushing every [`SYNC_EVERY`] bytes keeps
/// the progress honest and lets Cancel act within seconds instead of after a long final flush.
pub fn write(src: &Source<'_>, target: &Target, events: Sink<'_>, cancel: &Cancel) -> Result<()> {
    events(Event::Phase { phase: Phase::Write });
    if let (Target::File(p), Ok(raw)) = (target, std::fs::canonicalize(src.raw))
        && std::fs::canonicalize(p).is_ok_and(|t| t == raw)
    {
        bail!("{} is the source image; choose another file", p.display());
    }
    let mut input = File::open(src.raw).with_context(|| format!("opening {}", src.raw.display()))?;
    let mut out = target.open_write()?;
    let path = target.path().display().to_string();
    let mut buf = vec![0u8; CHUNK];
    let first = (src.size).min(CHUNK as u64) as usize;
    out.write_all(&buf[..first]).with_context(|| format!("clearing the partition table of {path}"))?;
    out.sync_data().with_context(|| format!("flushing {path}"))?;

    let mut throttle = Throttle::new(Phase::Write, src.size);
    let mut unsynced = 0u64;
    let mut done = 0u64;
    for i in 1..src.chunks() {
        cancel.check()?;
        let offset = i * CHUNK as u64;
        let len = (src.size - offset).min(CHUNK as u64) as usize;
        src.read_at(&mut input, offset, &mut buf[..len])?;
        out.seek(SeekFrom::Start(offset))?;
        out.write_all(&buf[..len]).with_context(|| format!("writing {path} at {offset}"))?;
        unsynced += len as u64;
        if unsynced >= SYNC_EVERY {
            out.sync_data().with_context(|| format!("flushing {path}"))?;
            done += unsynced;
            unsynced = 0;
            throttle.report(events, done);
        }
    }
    out.sync_data().with_context(|| format!("flushing {path}"))?;
    cancel.check()?;
    src.read_at(&mut input, 0, &mut buf[..first])?;
    out.seek(SeekFrom::Start(0))?;
    out.write_all(&buf[..first]).with_context(|| format!("writing the partition table of {path}"))?;
    out.sync_all().with_context(|| format!("flushing {path}"))?;
    throttle.report(events, src.size);
    Ok(())
}

/// A buffer whose start is aligned for O_DIRECT, found inside a slightly larger Vec.
struct Aligned {
    buf: Vec<u8>,
    start: usize,
}

impl Aligned {
    fn new(len: usize) -> Self {
        let buf = vec![0u8; len + ALIGN];
        let addr = buf.as_ptr() as usize;
        let start = (ALIGN - addr % ALIGN) % ALIGN;
        Self { buf, start }
    }

    fn slice(&mut self, len: usize) -> &mut [u8] {
        &mut self.buf[self.start..self.start + len]
    }
}

/// Reads everything back and compares it with what was meant to be written. O_DIRECT reads the
/// device, not the copy the kernel still has in memory from the write.
pub fn verify(src: &Source<'_>, target: &Target, events: Sink<'_>, cancel: &Cancel) -> Result<()> {
    events(Event::Phase { phase: Phase::Verify });
    let mut input = File::open(src.raw)?;
    let path = target.path();
    let mut disk = match (OpenOptions::new().read(true).custom_flags(libc::O_DIRECT).open(path), target) {
        (Ok(f), _) => f,
        // Some file systems (tmpfs) refuse O_DIRECT; a test file there is read normally. A disk
        // never is, or the check would compare the kernel's cached copy with itself.
        (Err(_), Target::File(_)) => {
            File::open(path).with_context(|| format!("opening {} to verify", path.display()))?
        }
        (Err(e), Target::Device(_)) => bail!("cannot read {} back directly: {e}", path.display()),
    };
    let mut expected = vec![0u8; CHUNK];
    let mut aligned = Aligned::new(CHUNK);
    let mut throttle = Throttle::new(Phase::Verify, src.size);
    let mut offset = 0u64;
    while offset < src.size {
        cancel.check()?;
        let len = (src.size - offset).min(CHUNK as u64) as usize;
        src.read_at(&mut input, offset, &mut expected[..len])?;
        // O_DIRECT wants whole sectors; images are whole sectors, but round up to be safe.
        let want = len.div_ceil(512) * 512;
        let got = aligned.slice(want);
        let mut filled = 0;
        while filled < len {
            let n = disk.read(&mut got[filled..]).with_context(|| format!("reading back {}", path.display()))?;
            if n == 0 {
                bail!("{} ended at {} bytes while verifying", path.display(), offset + filled as u64);
            }
            filled += n;
        }
        if got[..len] != expected[..len] {
            let at = got[..len].iter().zip(&expected[..len]).position(|(a, b)| a != b).unwrap_or(0);
            bail!("verify failed: the eMMC differs from the image at byte {} (bad cable or eMMC?)", offset + at as u64);
        }
        offset += len as u64;
        throttle.report(events, offset);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_patched_and_verifies() {
        let t = tempfile::tempdir().unwrap();
        let raw = t.path().join("img.raw");
        let size = (CHUNK * 2 + 4096) as u64;
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        std::fs::write(&raw, &data).unwrap();
        let patch = vec![0xAAu8; 10_000];
        let src = Source { raw: &raw, size, patch_offset: CHUNK as u64 - 5000, patch: &patch };
        let target = Target::File(t.path().join("out.img"));
        let noop = |_e: Event| {};
        write(&src, &target, &noop, &Cancel::new()).unwrap();
        verify(&src, &target, &noop, &Cancel::new()).unwrap();
        let out = std::fs::read(target.path()).unwrap();
        assert_eq!(out.len() as u64, size);
        let p = CHUNK - 5000;
        assert_eq!(&out[..p], &data[..p]);
        assert!(out[p..p + 10_000].iter().all(|&b| b == 0xAA));
        assert_eq!(&out[p + 10_000..], &data[p + 10_000..]);
        // A changed byte must fail the verify.
        let mut bad = out.clone();
        bad[123] ^= 1;
        std::fs::write(target.path(), &bad).unwrap();
        assert!(verify(&src, &target, &noop, &Cancel::new()).is_err());
    }

    #[test]
    fn file_target_never_writes_a_device_or_the_source() {
        let t = tempfile::tempdir().unwrap();
        let raw = t.path().join("img.raw");
        std::fs::write(&raw, vec![1u8; 8192]).unwrap();
        let src = Source { raw: &raw, size: 8192, patch_offset: 0, patch: &[] };
        let noop = |_e: Event| {};
        // /dev/null is a character device: refused before anything is written.
        assert!(write(&src, &Target::File("/dev/null".into()), &noop, &Cancel::new()).is_err());
        assert!(write(&src, &Target::File(raw.clone()), &noop, &Cancel::new()).is_err());
        assert_eq!(std::fs::read(&raw).unwrap(), vec![1u8; 8192]);
    }

    #[test]
    fn device_numbers() {
        // As glibc's makedev encodes them: sdb 8:16, nvme0n1 259:0, and a minor above 255.
        assert_eq!((dev_major(0x810), dev_minor(0x810)), (8, 16));
        assert_eq!((dev_major(0x10300), dev_minor(0x10300)), (259, 0));
        assert_eq!((dev_major(0x10082C), dev_minor(0x10082C)), (8, 300));
    }

    #[test]
    fn cm4_emmc_signature() {
        let d = |vendor: &str, model: &str, removable| BlockDevice {
            path: "/dev/sdb".into(),
            size: 7_818_182_656,
            vendor: Some(vendor.into()),
            model: Some(model.into()),
            removable,
            usb_port: Some("3-2".into()),
            usb_serial: None,
        };
        assert!(d("mmcblk0", "Raspberry Pi multi-function USB device", true).looks_like_cm4_emmc());
        assert!(d("RPi-MSD-", "0001", true).looks_like_cm4_emmc());
        assert!(!d("mmcblk0", "x", false).looks_like_cm4_emmc());
        assert!(!d("Kingston", "DataTraveler", true).looks_like_cm4_emmc());
    }
}
