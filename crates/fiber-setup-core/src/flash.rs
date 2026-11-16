//! The whole write: the FIBER started over USB on its port, its eMMC found on that same port,
//! settings into the boot partition (named after the CM4's serial number with the FIBER
//! software), write, read back.

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

/// How long the gadget may take to show its eMMC once it has its files (seen: about 10 s).
const DISK_TIMEOUT: Duration = Duration::from_secs(90);
/// Starting a board that waits in its boot ROM takes a few seconds; this long means none answers.
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

/// Starts the FIBER whose boot ROM waits on USB port `port` into the gadget (unless its eMMC is
/// there already) and waits for the eMMC to show as a disk of at least `min_size` bytes.
/// Nothing is written.
pub fn start_gadget(
    tools: &Tools,
    port: &str,
    min_size: u64,
    events: Sink<'_>,
    cancel: &Cancel,
) -> Result<BlockDevice> {
    events(Event::Phase { phase: Phase::Boot });
    if !matches!(find_emmc(port, min_size)?, Emmc::Ready(_)) {
        let log = |line: String| events(Event::Log { line });
        rpiboot::run(tools, port, RPIBOOT_TIMEOUT, &log, cancel)?;
    } else {
        events(Event::Log { line: "The FIBER's eMMC is already connected; it need not be started over USB".into() });
    }

    events(Event::Phase { phase: Phase::WaitDisk });
    let deadline = Instant::now() + DISK_TIMEOUT;
    let mut too_small: Option<BlockDevice> = None;
    let emmc = loop {
        cancel.check()?;
        match find_emmc(port, min_size)? {
            Emmc::Ready(d) => break d,
            Emmc::TooSmall(d) => too_small = Some(d),
            Emmc::NotYet => {}
        }
        if Instant::now() > deadline {
            if let Some(d) = too_small {
                bail!("the FIBER's eMMC ({} MB) is smaller than the image ({} MB)", d.size >> 20, min_size >> 20);
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
    Ok(emmc)
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
    let emmc = start_gadget(tools, port, image.size, events, cancel)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::firstboot::Wifi;
    use crate::testimg;
    use std::sync::Mutex;

    fn settings() -> Settings {
        Settings {
            hostname: "fiber".into(),
            username: "marek".into(),
            password: "correct horse".into(),
            ssh_key: Some("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample marek@laptop".into()),
            wifi: Some(Wifi { ssid: "Dílna \"5G\"".into(), password: "pa:ss;word\\x".into(), hidden: true }),
            country: "CZ".into(),
            timezone: Some("Europe/Prague".into()),
            install_fiber: true,
        }
    }

    fn prepared(raw: &std::path::Path) -> Prepared {
        Prepared {
            raw: raw.to_path_buf(),
            size: std::fs::metadata(raw).unwrap().len(),
            source_sha256: String::new(),
            official: false,
            name: "test".into(),
        }
    }

    /// The stages a run went through, in order.
    fn phases(events: &Mutex<Vec<Event>>) -> Vec<Phase> {
        events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| if let Event::Phase { phase } = e { Some(*phase) } else { None })
            .collect()
    }

    #[test]
    fn writes_the_image_with_the_settings_into_a_file() {
        let t = tempfile::tempdir().unwrap();
        let raw = t.path().join("os.img");
        let original = testimg::disk();
        std::fs::write(&raw, &original).unwrap();
        let out = t.path().join("out.img");
        let events = Mutex::new(Vec::new());
        let report =
            run_to_file(&prepared(&raw), &settings(), &out, &|e| events.lock().unwrap().push(e), &Cancel::new())
                .unwrap();
        assert_eq!(phases(&events), [Phase::Settings, Phase::Write, Phase::Verify, Phase::Done]);
        assert_eq!(report.written, testimg::SIZE as u64);
        assert_eq!(report.hostname, "fiber");
        assert_eq!(report.serial, None);

        // The image itself is never changed; the copy differs only in its boot partition.
        assert_eq!(std::fs::read(&raw).unwrap(), original);
        let written = std::fs::read(&out).unwrap();
        assert_eq!(written.len(), original.len());
        let (start, end) = (testimg::BOOT_START, testimg::BOOT_START + testimg::BOOT_SIZE);
        assert!(written[..start] == original[..start]);
        assert!(written[end..] == original[end..]);
        assert!(written[start..end] != original[start..end]);

        assert_eq!(
            testimg::boot_files(&written),
            ["cmdline.txt", "config.txt", "fiber-install.sh", "meta-data", "network-config", "user-data"]
        );
        let id = &report.instance_id;
        assert_eq!(
            testimg::boot_file(&written, "cmdline.txt").unwrap(),
            format!("{} ds=nocloud;i={id} cfg80211.ieee80211_regdom=CZ\n", testimg::CMDLINE.trim_end())
        );
        assert_eq!(testimg::boot_file(&written, "meta-data").unwrap(), format!("instance-id: {id}\n"));
        assert_eq!(testimg::boot_file(&written, "config.txt").unwrap(), "arm_64bit=1\n");
        assert_eq!(testimg::boot_file(&written, "fiber-install.sh").unwrap(), firstboot::INSTALL_SCRIPT);
        let user = testimg::boot_file(&written, "user-data").unwrap();
        assert!(user.starts_with("#cloud-config\n"), "{user}");
        assert!(user.contains("hostname: \"fiber\"\n"), "{user}");
        assert!(user.contains("  name: \"marek\"\n"), "{user}");
        assert!(user.contains("  passwd: \"$6$"), "{user}");
        assert!(!user.contains("correct horse"), "the password itself must not be on the FIBER");
        let network = testimg::boot_file(&written, "network-config").unwrap();
        assert!(network.contains("        \"Dílna \\\"5G\\\"\":\n"), "{network}");
        assert!(network.contains("          hidden: true\n"), "{network}");
    }

    #[test]
    fn new_settings_replace_the_earlier_ones() {
        let t = tempfile::tempdir().unwrap();
        // An image that was set up before, and has started once: the files of an older FIBER
        // Setup and the log of its first start are on it.
        let boot = testimg::fat(
            testimg::BOOT_SIZE,
            &[
                ("cmdline.txt", "console=tty1 rootwait ds=nocloud;i=old cfg80211.ieee80211_regdom=GB\n"),
                ("user-data", "#cloud-config\nhostname: old\n"),
                ("fiber-install.sh", "old script\n"),
                ("fiber-cm4-setup.sh", "older script\n"),
                ("fiber-setup.log", "old log\n"),
            ],
        );
        let raw = t.path().join("os.img");
        std::fs::write(&raw, testimg::disk_around(&boot)).unwrap();
        let out = t.path().join("out.img");
        let mut plain = settings();
        plain.install_fiber = false;
        plain.wifi = None;
        plain.hostname = "bench-7".into();
        let report = run_to_file(&prepared(&raw), &plain, &out, &|_| {}, &Cancel::new()).unwrap();
        let written = std::fs::read(&out).unwrap();
        assert_eq!(testimg::boot_files(&written), ["cmdline.txt", "meta-data", "network-config", "user-data"]);
        assert_eq!(
            testimg::boot_file(&written, "cmdline.txt").unwrap(),
            format!("console=tty1 rootwait ds=nocloud;i={} cfg80211.ieee80211_regdom=CZ\n", report.instance_id)
        );
        let user = testimg::boot_file(&written, "user-data").unwrap();
        assert!(user.contains("hostname: \"bench-7\"\n") && !user.contains("old"), "{user}");
        assert!(!testimg::boot_file(&written, "network-config").unwrap().contains("wifis"));
    }

    #[test]
    fn refuses_what_it_cannot_set_up_and_writes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let out = t.path().join("out.img");
        let try_image = |image: Vec<u8>, settings: &Settings| {
            let raw = t.path().join("os.img");
            std::fs::write(&raw, image).unwrap();
            let error = run_to_file(&prepared(&raw), settings, &out, &|_| {}, &Cancel::new()).unwrap_err();
            assert!(!out.exists(), "{error:#}");
            format!("{error:#}")
        };

        let mut bad = settings();
        bad.username = "root".into();
        assert!(try_image(testimg::disk(), &bad).contains("user name"));

        // A boot partition that is no FAT, one without cmdline.txt, one that ends after the image.
        let error = try_image(testimg::disk_around(&vec![0u8; testimg::BOOT_SIZE]), &settings());
        assert!(error.contains("boot partition (FAT)"), "{error}");
        let error = try_image(testimg::disk_around(&testimg::fat(testimg::BOOT_SIZE, &[])), &settings());
        assert!(error.contains("cmdline.txt"), "{error}");
        let mut short = testimg::disk();
        short.truncate(4 << 20);
        let error = try_image(short, &settings());
        assert!(error.contains("not where expected"), "{error}");
        let mut no_table = testimg::disk();
        no_table[510] = 0;
        let error = try_image(no_table, &settings());
        assert!(error.contains("no MBR partition table"), "{error}");
    }

    #[test]
    fn a_stopped_write_leaves_no_partition_table() {
        let t = tempfile::tempdir().unwrap();
        let raw = t.path().join("os.img");
        std::fs::write(&raw, testimg::disk()).unwrap();
        let out = t.path().join("out.img");
        // What is there from an earlier write must not survive as a disk that starts.
        std::fs::write(&out, testimg::disk()).unwrap();
        let cancel = Cancel::new();
        let stop = cancel.clone();
        let error = run_to_file(
            &prepared(&raw),
            &settings(),
            &out,
            &|e| {
                if matches!(e, Event::Phase { phase: Phase::Write }) {
                    stop.cancel();
                }
            },
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "cancelled");
        let left = std::fs::read(&out).unwrap();
        assert!(left.len() >= 512 && left[..512].iter().all(|&b| b == 0), "{} bytes", left.len());
    }
}
