//! FIBER Setup core: everything the desktop app and the CLI share. No Tauri.
//!
//! The flow: get the operating system image ([`image`]), put the FIBER into USB boot mode and
//! boot it into the mass-storage gadget ([`usb`], [`rpiboot`]), find its eMMC on the same USB
//! port ([`disk`]), write the image with the first-boot settings patched into its boot partition
//! ([`firstboot`], the "image loopback"), and read it all back ([`flash`]).
//!
//! Linux finds the boards and disks in sysfs; macOS asks its I/O Registry (`macos`), Windows its
//! device and storage management (`windows`).

pub mod access;
pub mod catalog;
pub mod check;
pub mod disk;
pub mod event;
pub mod firstboot;
pub mod flash;
pub mod image;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod mbr;
pub mod rpiboot;
pub mod usb;
#[cfg(windows)]
pub mod windows;

pub use event::{Cancel, Event, Phase};

/// The version of FIBER Setup, written into the first-boot files so a unit tells what set it up.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
