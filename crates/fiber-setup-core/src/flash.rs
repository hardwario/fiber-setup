//! The whole write: rpiboot on the FIBER's port, its eMMC found on that same port, settings into
//! the boot partition (named after the CM4's serial number with the FIBER software), write, read
//! back.

use crate::disk::{self, BlockDevice, Source, Target};
use crate::event::{Cancel, Event, Phase, Sink};
use crate::firstboot::{self, Settings};
use crate::image::Prepared;
use crate::mbr;
use crate::rpiboot::{self, Tools};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, Instant};

/// How long the gadget may take to show its eMMC after rpiboot handed over (seen: about 10 s).
const DISK_TIMEOUT: Duration = Duration::from_secs(90);
/// rpiboot needs a few seconds for a board that waits in its boot ROM; this long means none answers.
const RPIBOOT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub disk: String,
    pub disk_size: u64,
    /// The CM4's serial number, from the gadget's USB descriptor (the gadget takes it from
    /// /proc/cpuinfo).
    pub serial: Option<String>,
    /// The name the FIBER software gives this FIBER.
    pub hub_name: Option<String>,
    /// The name the FIBER starts with (`<hostname>.local`): `hub_name` with the FIBER software.
    pub hostname: String,
    pub written: u64,
    pub instance_id: String,
    pub seconds: u64,
}

/// The boot partition of the image with the settings written into it, and where it goes.
pub struct Personalised {
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub instance_id: String,
}

pub fn personalise(image: &Prepared, settings: &Settings, events: Sink<'_>) -> Result<Personalised> {
    events(Event::Phase { phase: Phase::Settings });
    let part = mbr::boot_partition(&image.raw)?;
    if part.start + part.size > image.size || part.size > 1 << 30 {
        bail!("the image's boot partition is not where expected");
    }
    let mut bytes = vec![0u8; part.size as usize];
    let mut f = File::open(&image.raw)?;
    f.seek(SeekFrom::Start(part.start))?;
    f.read_exact(&mut bytes).context("reading the image's boot partition")?;
    let (bytes, instance_id) = firstboot::personalise(bytes, settings)?;
    if bytes.len() as u64 != part.size {
        bail!("the boot partition changed size while writing the settings");
    }
    events(Event::Log { line: format!("Settings written for {} (instance {instance_id})", settings.hostname) });
    Ok(Personalised { offset: part.start, bytes, instance_id })
}

enum Emmc {
    NotYet,
    /// There, but smaller than the image; the gadget reports size 0 for a moment while it starts.
    TooSmall(BlockDevice),
    Ready(BlockDevice),
}

/// The eMMC that the board on `port` exposes, if it is there yet.
fn find_emmc(port: &str, image_size: u64) -> Result<Emmc> {
    let disks = disk::list().context("listing disks")?;
    let mut on_port = disks.into_iter().filter(|d| d.usb_port.as_deref() == Some(port) && d.looks_like_cm4_emmc());
    let Some(d) = on_port.next() else { return Ok(Emmc::NotYet) };
    if on_port.next().is_some() {
        bail!("more than one CM4 disk on USB port {port}; not writing either");
    }
    if d.size > 128 << 30 {
        bail!("{} is larger than any CM4 eMMC; not writing it", d.path);
    }
    if d.size < image_size {
        return Ok(Emmc::TooSmall(d));
    }
    Ok(Emmc::Ready(d))
}

/// Flashes the FIBER whose boot ROM waits on USB port `port`.
pub fn run(
    tools: &Tools,
    image: &Prepared,
    settings: &Settings,
    port: &str,
    events: Sink<'_>,
    cancel: &Cancel,
) -> Result<Report> {
    let started = Instant::now();
    settings.validate()?;

    events(Event::Phase { phase: Phase::Boot });
    if !matches!(find_emmc(port, image.size)?, Emmc::Ready(_)) {
        // rpiboot cannot be told which board to take on Windows (rpiboot.rs).
        #[cfg(windows)]
        if crate::usb::boards().iter().filter(|b| b.kind == crate::usb::BoardKind::Boot).count() > 1 {
            bail!("several boards are in USB mode; on Windows connect one FIBER at a time");
        }
        let log = |line: String| events(Event::Log { line });
        rpiboot::run(tools, port, RPIBOOT_TIMEOUT, &log, cancel)?;
    } else {
        events(Event::Log { line: "The FIBER's eMMC is already connected; rpiboot not needed".into() });
    }

    events(Event::Phase { phase: Phase::WaitDisk });
    let deadline = Instant::now() + DISK_TIMEOUT;
    let mut too_small: Option<BlockDevice> = None;
    let emmc = loop {
        cancel.check()?;
        match find_emmc(port, image.size)? {
            Emmc::Ready(d) => break d,
            Emmc::TooSmall(d) => too_small = Some(d),
            Emmc::NotYet => {}
        }
        if Instant::now() > deadline {
            if let Some(d) = too_small {
                bail!("the FIBER's eMMC ({} MB) is smaller than the image ({} MB)", d.size >> 20, image.size >> 20);
            }
            bail!("the FIBER's eMMC did not appear on USB port {port} (check the cable, then try again)");
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    events(Event::Log {
        line: format!(
            "eMMC {} ({:.1} GB) on USB port {port}{}",
            emmc.path,
            emmc.size as f64 / 1e9,
            emmc.usb_serial.as_deref().map(|s| format!(", CM4 serial {s}")).unwrap_or_default()
        ),
    });
    disk::free_for_writing(&emmc)?;

    // The settings go in only now: the serial number names the FIBER.
    let settings =
        Settings { hostname: firstboot::start_name(settings, emmc.usb_serial.as_deref()), ..settings.clone() };
    settings.validate()?;
    let p = personalise(image, &settings, events)?;
    let src = Source { raw: &image.raw, size: image.size, patch_offset: p.offset, patch: &p.bytes };
    let target = Target::Device(emmc.path.clone().into());
    // Same disk, still on the same port, right before writing.
    match find_emmc(port, image.size)? {
        Emmc::Ready(d) if d.path == emmc.path => {}
        _ => bail!("the FIBER's eMMC went away before writing; check the cable"),
    }
    disk::write_and_verify(&src, &target, events, cancel)?;
    // macOS would mount the new boot partition now, and complain when the FIBER restarts.
    #[cfg(target_os = "macos")]
    events(Event::Log {
        line: match crate::macos::eject(&emmc) {
            Ok(()) => format!("{} ejected", emmc.path),
            Err(e) => format!("{e:#} (the FIBER is written all the same)"),
        },
    });
    events(Event::Phase { phase: Phase::Done });
    Ok(Report {
        disk: emmc.path.clone(),
        disk_size: emmc.size,
        serial: emmc.usb_serial.clone(),
        hub_name: emmc.usb_serial.as_deref().and_then(firstboot::hub_name),
        hostname: settings.hostname,
        written: image.size,
        instance_id: p.instance_id,
        seconds: started.elapsed().as_secs(),
    })
}

/// The same write into a plain file instead of a FIBER: for testing, and for preparing an image
/// file another tool will write.
pub fn run_to_file(
    image: &Prepared,
    settings: &Settings,
    out: &std::path::Path,
    events: Sink<'_>,
    cancel: &Cancel,
) -> Result<Report> {
    let started = Instant::now();
    let p = personalise(image, settings, events)?;
    let src = Source { raw: &image.raw, size: image.size, patch_offset: p.offset, patch: &p.bytes };
    let target = Target::File(out.to_path_buf());
    disk::write_and_verify(&src, &target, events, cancel)?;
    events(Event::Phase { phase: Phase::Done });
    Ok(Report {
        disk: out.display().to_string(),
        disk_size: image.size,
        serial: None,
        hub_name: None,
        hostname: settings.hostname.clone(),
        written: image.size,
        instance_id: p.instance_id,
        seconds: started.elapsed().as_secs(),
    })
}
