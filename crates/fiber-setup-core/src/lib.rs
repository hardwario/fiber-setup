//! FIBER Setup core: everything the desktop app and the CLI share. No Tauri.
//!
//! The flow: get the operating system image ([`image`]), put the FIBER into USB boot mode and
//! boot it into the mass-storage gadget ([`usb`], [`rpiboot`]), find its eMMC on the same USB
//! port ([`disk`]), write the image with the first-boot settings patched into its boot partition
//! ([`firstboot`], the "image loopback"), and read it all back ([`flash`]).

pub mod access;
pub mod catalog;
pub mod check;
pub mod disk;
pub mod event;
pub mod firstboot;
pub mod flash;
pub mod image;
pub mod mbr;
pub mod rpiboot;
pub mod usb;

pub use event::{Cancel, Event, Phase};

/// The version of FIBER Setup, written into the first-boot files so a unit tells what set it up.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
