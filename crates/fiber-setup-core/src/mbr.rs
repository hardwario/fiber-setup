//! The MBR partition table of a Raspberry Pi OS image: where its FAT boot partition is.

use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub const SECTOR: u64 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Partition {
    pub index: usize,
    pub kind: u8,
    /// Offset from the start of the disk, in bytes.
    pub start: u64,
    /// Length in bytes.
    pub size: u64,
}

/// The four primary partitions of an MBR sector; empty entries are left out.
pub fn parse(sector: &[u8]) -> Result<Vec<Partition>> {
    if sector.len() < 512 || sector[510] != 0x55 || sector[511] != 0xAA {
        bail!("no MBR partition table (the image is not a Raspberry Pi OS disk image)");
    }
    let mut out = Vec::new();
    for index in 0..4 {
        let e = &sector[446 + index * 16..446 + (index + 1) * 16];
        let kind = e[4];
        let lba = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as u64;
        let sectors = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as u64;
        if kind == 0 || sectors == 0 {
            continue;
        }
        if kind == 0xEE {
            bail!("the image has a GPT partition table; only MBR images (Raspberry Pi OS) are supported");
        }
        out.push(Partition { index: index + 1, kind, start: lba * SECTOR, size: sectors * SECTOR });
    }
    Ok(out)
}

/// The first partition, which must be FAT: Raspberry Pi OS keeps `cmdline.txt` and the
/// cloud-init seed files there (`/boot/firmware` on the running system).
pub fn boot_partition(image: &Path) -> Result<Partition> {
    let mut sector = [0u8; 512];
    File::open(image)
        .and_then(|mut f| f.read_exact(&mut sector))
        .with_context(|| format!("reading the partition table of {}", image.display()))?;
    let parts = parse(&sector)?;
    let Some(first) = parts.first().copied() else {
        bail!("the image has no partitions");
    };
    // 0x0b/0x0c FAT32, 0x0e FAT16 LBA, 0x06 FAT16.
    if !matches!(first.kind, 0x0b | 0x0c | 0x0e | 0x06) {
        bail!("the first partition is not FAT (type 0x{:02x})", first.kind);
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sector: &mut [u8], index: usize, kind: u8, lba: u32, sectors: u32) {
        let e = &mut sector[446 + index * 16..446 + (index + 1) * 16];
        e[4] = kind;
        e[8..12].copy_from_slice(&lba.to_le_bytes());
        e[12..16].copy_from_slice(&sectors.to_le_bytes());
    }

    #[test]
    fn reads_raspios_layout() {
        let mut s = vec![0u8; 512];
        s[510] = 0x55;
        s[511] = 0xAA;
        entry(&mut s, 0, 0x0c, 16384, 1_048_576);
        entry(&mut s, 1, 0x83, 1_064_960, 4_000_000);
        let p = parse(&s).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].start, 16384 * 512);
        assert_eq!(p[0].size, 1_048_576 * 512);
        assert_eq!(p[1].kind, 0x83);
    }

    #[test]
    fn refuses_no_signature_and_gpt() {
        assert!(parse(&[0u8; 512]).is_err());
        let mut s = vec![0u8; 512];
        s[510] = 0x55;
        s[511] = 0xAA;
        entry(&mut s, 0, 0xEE, 1, 100);
        assert!(parse(&s).is_err());
    }
}
