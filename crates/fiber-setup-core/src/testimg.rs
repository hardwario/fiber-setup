//! A small disk image for the tests, laid out as Raspberry Pi OS is: an MBR, a FAT boot
//! partition with `cmdline.txt`, and a second partition whose bytes stand for the root file
//! system. At 12 MiB it takes three chunks to write, and the boot partition lies across all three.

use std::io::{Cursor, Read, Write};

pub const BOOT_START: usize = 1 << 20;
pub const BOOT_SIZE: usize = 8 << 20;
pub const SIZE: usize = 12 << 20;
pub const CMDLINE: &str = "console=serial0,115200 console=tty1 root=PARTUUID=1234abcd-02 rootfstype=ext4 rootwait\n";

/// A FAT volume of `size` bytes holding `files`.
pub fn fat(size: usize, files: &[(&str, &str)]) -> Vec<u8> {
    let mut cursor = Cursor::new(vec![0u8; size]);
    fatfs::format_volume(&mut cursor, fatfs::FormatVolumeOptions::new()).unwrap();
    {
        let fs = fatfs::FileSystem::new(&mut cursor, fatfs::FsOptions::new()).unwrap();
        for (name, text) in files {
            fs.root_dir().create_file(name).unwrap().write_all(text.as_bytes()).unwrap();
        }
        fs.unmount().unwrap();
    }
    cursor.into_inner()
}

/// The disk image around a boot partition: bytes that do not repeat everywhere else, so a
/// chunk written to the wrong place shows.
pub fn disk_around(boot: &[u8]) -> Vec<u8> {
    assert_eq!(boot.len(), BOOT_SIZE);
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut image: Vec<u8> = (0..SIZE)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    image[..512].fill(0);
    let mut entry = |index: usize, kind: u8, start: usize, size: usize| {
        let e = &mut image[446 + index * 16..446 + (index + 1) * 16];
        e[4] = kind;
        e[8..12].copy_from_slice(&((start / 512) as u32).to_le_bytes());
        e[12..16].copy_from_slice(&((size / 512) as u32).to_le_bytes());
    };
    entry(0, 0x0c, BOOT_START, BOOT_SIZE);
    entry(1, 0x83, BOOT_START + BOOT_SIZE, SIZE - BOOT_START - BOOT_SIZE);
    image[510] = 0x55;
    image[511] = 0xAA;
    image[BOOT_START..BOOT_START + BOOT_SIZE].copy_from_slice(boot);
    image
}

/// The image as it comes from Raspberry Pi: `cmdline.txt` and `config.txt` on the boot partition.
pub fn disk() -> Vec<u8> {
    disk_around(&fat(BOOT_SIZE, &[("cmdline.txt", CMDLINE), ("config.txt", "arm_64bit=1\n")]))
}

/// The names of the files on the image's boot partition, sorted.
pub fn boot_files(image: &[u8]) -> Vec<String> {
    let mut cursor = Cursor::new(image[BOOT_START..BOOT_START + BOOT_SIZE].to_vec());
    let fs = fatfs::FileSystem::new(&mut cursor, fatfs::FsOptions::new()).unwrap();
    let mut names: Vec<String> = fs.root_dir().iter().map(|e| e.unwrap().file_name()).collect();
    names.sort();
    names
}

/// A file on the image's boot partition.
pub fn boot_file(image: &[u8], name: &str) -> Option<String> {
    let mut cursor = Cursor::new(image[BOOT_START..BOOT_START + BOOT_SIZE].to_vec());
    let fs = fatfs::FileSystem::new(&mut cursor, fatfs::FsOptions::new()).unwrap();
    let mut text = String::new();
    fs.root_dir().open_file(name).ok()?.read_to_string(&mut text).unwrap();
    Some(text)
}

/// The image packed as Raspberry Pi publishes it.
pub fn xz(data: &[u8]) -> Vec<u8> {
    let mut encoder = liblzma::write::XzEncoder::new(Vec::new(), 1);
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}
