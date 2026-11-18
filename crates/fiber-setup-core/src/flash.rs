//! The whole write: the FIBER started over USB on its port, its eMMC found on that same port,
//! settings into the boot partition (named after the CM4's serial number with the FIBER
//! software), write, read back.
//!
//! Several FIBERs go as a batch ([`run_many`]): all are started first, their disks are opened
//! together (macOS asks for the password once), and then they are written all at once or one
//! after another. One FIBER ([`run`]) is a batch of one.

use crate::disk::{self, BlockDevice, Patch, Source, Target};
use crate::event::{Cancel, Event, Phase, Sink};
use crate::firstboot::{self, Settings};
use crate::image::Prepared;
use crate::mbr;
use crate::rpiboot::{self, Tools};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long the gadget may take to show its eMMC once it has its files (seen: about 10 s).
const DISK_TIMEOUT: Duration = Duration::from_secs(90);
/// Starting a board that waits in its boot ROM takes a few seconds; this long means none answers.
const RPIBOOT_TIMEOUT: Duration = Duration::from_secs(120);
/// Half-second looks in a row that find the board in boot mode while its memory is waited for
/// (5 s): it was reset. And how often it is started again for that.
const BACK_IN_BOOT_MODE: u32 = 10;
const START_AGAIN: u32 = 2;

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

/// What the settings change in the image: the parts of its boot partition that differ.
pub struct Personalised {
    pub patches: Vec<Patch>,
    pub instance_id: String,
}

pub fn personalise(image: &Prepared, settings: &Settings, events: Sink<'_>) -> Result<Personalised> {
    events(Event::Phase { phase: Phase::Settings });
    // One at a time: the copy of the boot partition is hundreds of megabytes until only what the
    // settings changed is kept of it, and a batch would hold one for every FIBER at once.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
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
    let patches = changed(&mut f, part.start, &bytes)?;
    events(Event::Log { line: format!("Settings written for {} (instance {instance_id})", settings.hostname) });
    Ok(Personalised { patches, instance_id })
}

/// What of the personalised boot partition differs from the image's own, which starts at `start`
/// in `image`: the blocks with a changed byte in them, neighbours joined. The settings change a
/// few files and the tables that list them (about a megabyte of Raspberry Pi OS's 512), and a
/// batch keeps this in memory for every FIBER it writes at a time.
fn changed(image: &mut File, start: u64, patched: &[u8]) -> Result<Vec<Patch>> {
    const BLOCK: usize = 64 << 10;
    image.seek(SeekFrom::Start(start))?;
    let mut original = vec![0u8; BLOCK];
    let mut patches: Vec<Patch> = Vec::new();
    for (i, ours) in patched.chunks(BLOCK).enumerate() {
        image.read_exact(&mut original[..ours.len()]).context("reading the image's boot partition")?;
        if ours == &original[..ours.len()] {
            continue;
        }
        let offset = start + (i * BLOCK) as u64;
        match patches.last_mut() {
            Some(last) if last.offset + last.bytes.len() as u64 == offset => last.bytes.extend_from_slice(ours),
            _ => patches.push(Patch { offset, bytes: ours.to_vec() }),
        }
    }
    Ok(patches)
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
    let log = |line: String| events(Event::Log { line });
    if !matches!(find_emmc(port, min_size)?, Emmc::Ready(_)) {
        rpiboot::run(tools, port, RPIBOOT_TIMEOUT, &log, cancel)?;
    } else {
        log("The FIBER's eMMC is already connected; it need not be started over USB".into());
    }

    events(Event::Phase { phase: Phase::WaitDisk });
    let mut deadline = Instant::now() + DISK_TIMEOUT;
    let mut too_small: Option<BlockDevice> = None;
    // Looks in a row that found the board in boot mode again instead of its memory, and how
    // often it was started again for that.
    let (mut in_boot_mode, mut again) = (0, 0);
    let emmc = loop {
        cancel.check()?;
        match find_emmc(port, min_size)? {
            Emmc::Ready(d) => break d,
            Emmc::TooSmall(d) => too_small = Some(d),
            Emmc::NotYet => {}
        }
        // RESET pressed after the board had its files: it waits in its boot ROM again, and no
        // memory comes of that. (For a moment after its files it is still there too.)
        in_boot_mode = if rpiboot::waiting(port) { in_boot_mode + 1 } else { 0 };
        if in_boot_mode >= BACK_IN_BOOT_MODE && again < START_AGAIN {
            again += 1;
            in_boot_mode = 0;
            log(format!("The FIBER on USB port {port} is in boot mode again (RESET?); starting it again"));
            events(Event::Phase { phase: Phase::Boot });
            rpiboot::run(tools, port, RPIBOOT_TIMEOUT, &log, cancel)?;
            events(Event::Phase { phase: Phase::WaitDisk });
            deadline = Instant::now() + DISK_TIMEOUT;
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

/// How many FIBERs one batch may hold: what a desk, its cables and a person keep track of.
pub const MAX_DEVICES: usize = 8;

/// One FIBER of a batch.
#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    /// The USB port its boot ROM waits on.
    pub port: String,
    /// The name it gets without the FIBER software, where every FIBER of a batch needs its own.
    /// With the FIBER software each is named after its serial number, and this is not used.
    pub hostname: Option<String>,
}

/// How a batch writes: every FIBER at the same time, or one after another. All at once is faster
/// when each FIBER is on its own port of the computer; behind one hub they share its speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Together,
    OneByOne,
}

/// Where a batch sends what happens to the FIBER on a port.
pub type PortSink<'a> = &'a (dyn Fn(&str, Event) + Send + Sync);
/// Where a batch says how the FIBER on a port ended, as soon as it has.
pub type Finished<'a> = &'a (dyn Fn(&str, Result<Report>) + Send + Sync);

/// What a batch needs from the world outside: boards on USB and their disks. The rest is the
/// order of things, which a test runs without a board.
trait Bench: Sync {
    /// Starts the board on `port` over USB and gives its eMMC, free for writing.
    fn start(&self, port: &str, events: Sink<'_>, cancel: &Cancel) -> Result<BlockDevice>;
    /// Whether `emmc` is still the disk on that port, right before it is opened.
    fn still(&self, port: &str, emmc: &BlockDevice) -> Result<()>;
    /// Opens all of them for writing: one result for each, in their order.
    fn open(&self, disks: &[&BlockDevice]) -> Vec<Result<(Target, File)>>;
    /// Lets go of a written disk; a line for the log.
    fn release(&self, emmc: &BlockDevice) -> Option<String>;
}

/// The real bench: FIBERs on this computer's USB.
struct Usb<'a> {
    tools: &'a Tools,
    image_size: u64,
}

impl Bench for Usb<'_> {
    fn start(&self, port: &str, events: Sink<'_>, cancel: &Cancel) -> Result<BlockDevice> {
        let emmc = start_gadget(self.tools, port, self.image_size, events, cancel)?;
        disk::free_for_writing(&emmc)?;
        Ok(emmc)
    }

    fn still(&self, port: &str, emmc: &BlockDevice) -> Result<()> {
        match find_emmc(port, self.image_size)? {
            Emmc::Ready(d) if d.path == emmc.path => Ok(()),
            _ => bail!("the FIBER's eMMC went away before writing; check the cable"),
        }
    }

    fn open(&self, disks: &[&BlockDevice]) -> Vec<Result<(Target, File)>> {
        let paths: Vec<std::path::PathBuf> = disks.iter().map(|d| d.path.clone().into()).collect();
        let opened = disk::open_devices(&paths);
        paths.into_iter().zip(opened).map(|(path, file)| Ok((Target::Device(path), file?))).collect()
    }

    fn release(&self, emmc: &BlockDevice) -> Option<String> {
        // macOS would mount the new boot partition now, and complain when the FIBER restarts.
        #[cfg(target_os = "macos")]
        return Some(match crate::macos::eject(emmc) {
            Ok(()) => format!("{} ejected", emmc.path),
            Err(e) => format!("{e:#} (the FIBER is written all the same)"),
        });
        #[cfg(not(target_os = "macos"))]
        {
            let _ = emmc;
            None
        }
    }
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
    let result: Mutex<Option<Result<Report>>> = Mutex::new(None);
    let one = [Device { port: port.to_string(), hostname: None }];
    let keep = |_: &str, r: Result<Report>| *result.lock().unwrap_or_else(PoisonError::into_inner) = Some(r);
    run_many(tools, image, settings, &one, Order::Together, &|_, e| events(e), &keep, cancel);
    match result.into_inner().unwrap_or_else(PoisonError::into_inner) {
        Some(r) => r,
        None => bail!("the write ended without saying how"),
    }
}

/// Flashes the FIBERs whose boot ROMs wait on the USB ports of `devices`, with the same settings.
/// `finished` hears of each one as soon as it is done or has failed; one that fails does not
/// stop the others.
#[allow(clippy::too_many_arguments)]
pub fn run_many(
    tools: &Tools,
    image: &Prepared,
    settings: &Settings,
    devices: &[Device],
    order: Order,
    events: PortSink<'_>,
    finished: Finished<'_>,
    cancel: &Cancel,
) {
    batch(&Usb { tools, image_size: image.size }, image, settings, devices, order, events, finished, cancel);
}

/// A FIBER of a batch that is started: its disk, its own settings, and how long that took.
struct Ready {
    emmc: BlockDevice,
    settings: Settings,
    start: Duration,
}

#[allow(clippy::too_many_arguments)]
fn batch(
    bench: &dyn Bench,
    image: &Prepared,
    settings: &Settings,
    devices: &[Device],
    order: Order,
    events: PortSink<'_>,
    finished: Finished<'_>,
    cancel: &Cancel,
) {
    let refuse = |why: String| devices.iter().for_each(|d| finished(&d.port, Err(anyhow::anyhow!("{why}"))));
    if devices.len() > MAX_DEVICES {
        return refuse(format!("at most {MAX_DEVICES} FIBERs can be written at once"));
    }
    if (1..devices.len()).any(|i| devices[..i].iter().any(|d| d.port == devices[i].port)) {
        return refuse("a USB port is named twice".into());
    }

    // 1. Every board is started and its memory found, all at once (it takes seconds and little
    //    of the USB's speed): what is wrong with a cable or a jumper shows now, not minutes in.
    let start = |device: &Device| -> Result<Ready> {
        let began = Instant::now();
        let sink = |e: Event| events(&device.port, e);
        let mut own = settings.clone();
        if let Some(name) = &device.hostname {
            own.hostname = name.clone();
        }
        own.validate()?;
        let emmc = bench.start(&device.port, &sink, cancel)?;
        // The settings are made only now: the serial number names the FIBER.
        let own = Settings { hostname: firstboot::start_name(&own, emmc.usb_serial.as_deref()), ..own };
        own.validate()?;
        sink(Event::Phase { phase: Phase::Ready });
        Ok(Ready { emmc, settings: own, start: began.elapsed() })
    };
    let started: Vec<Result<Ready>> = std::thread::scope(|scope| {
        let running: Vec<_> = devices.iter().map(|device| scope.spawn(|| start(device))).collect();
        running
            .into_iter()
            .map(|r| r.join().unwrap_or_else(|_| Err(anyhow::anyhow!("starting this FIBER broke off"))))
            .collect()
    });
    let mut ready = Vec::new();
    for (device, started) in devices.iter().zip(started) {
        // Same disk, still on the same port, right before it is opened.
        match started.and_then(|r| cancel.check().and_then(|()| bench.still(&device.port, &r.emmc)).map(|()| r)) {
            Ok(r) => ready.push((device, r)),
            Err(e) => finished(&device.port, Err(e)),
        }
    }
    if ready.is_empty() {
        return;
    }

    // 2. All their disks are opened together: macOS asks for the password once, not again for
    //    every FIBER, minutes apart. A disk stays open until its turn.
    let opened = bench.open(&ready.iter().map(|(_, r)| &r.emmc).collect::<Vec<_>>());
    let mut jobs = Vec::new();
    for ((device, r), opened) in ready.into_iter().zip(opened) {
        match opened {
            Ok((target, file)) => jobs.push((device, r, target, file)),
            Err(e) => finished(&device.port, Err(e)),
        }
    }

    // 3. Each is written and read back.
    let write = |(device, r, target, file): (&Device, Ready, Target, File)| {
        let began = Instant::now();
        let sink = |e: Event| events(&device.port, e);
        let written = || -> Result<Report> {
            cancel.check()?;
            let p = personalise(image, &r.settings, &sink)?;
            let src = Source { raw: &image.raw, size: image.size, patches: &p.patches };
            disk::write_and_verify_open(&src, &target, file, &sink, cancel)?;
            if let Some(line) = bench.release(&r.emmc) {
                sink(Event::Log { line });
            }
            sink(Event::Phase { phase: Phase::Done });
            Ok(Report {
                disk: r.emmc.path.clone(),
                disk_size: r.emmc.size,
                serial: r.emmc.usb_serial.clone(),
                hub_name: r.emmc.usb_serial.as_deref().and_then(firstboot::hub_name),
                hostname: r.settings.hostname.clone(),
                written: image.size,
                instance_id: p.instance_id,
                // Its own work, without the wait for its turn.
                seconds: (r.start + began.elapsed()).as_secs(),
            })
        };
        finished(&device.port, written());
    };
    match order {
        Order::Together => std::thread::scope(|scope| {
            let write = &write;
            for job in jobs {
                scope.spawn(move || write(job));
            }
        }),
        Order::OneByOne => jobs.into_iter().for_each(write),
    }
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
    let src = Source { raw: &image.raw, size: image.size, patches: &p.patches };
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
            components: vec!["agent".into(), "node-red".into()],
            mqtt_lan: false,
            minipcie: Default::default(),
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
            [
                "cmdline.txt",
                "config.txt",
                "fiber-install.sh",
                "fiber-seed.yaml",
                "meta-data",
                "network-config",
                "user-data"
            ]
        );
        let id = &report.instance_id;
        assert_eq!(
            testimg::boot_file(&written, "cmdline.txt").unwrap(),
            format!("{} ds=nocloud;i={id} cfg80211.ieee80211_regdom=CZ\n", testimg::CMDLINE.trim_end())
        );
        assert_eq!(testimg::boot_file(&written, "meta-data").unwrap(), format!("instance-id: {id}\n"));
        assert_eq!(testimg::boot_file(&written, "config.txt").unwrap(), "arm_64bit=1\n");
        assert_eq!(
            testimg::boot_file(&written, "fiber-install.sh").unwrap(),
            firstboot::install_script(&crate::catalog::FIBER_PLATFORM)
        );
        let seed = testimg::boot_file(&written, "fiber-seed.yaml").unwrap();
        assert!(seed.contains("components: {base: true, mosquitto: true, agent: true, node-red: true}\n"), "{seed}");
        assert!(seed.contains("login: {user: \"marek\", password: \"correct horse\"}\n"), "{seed}");
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

    /// A desk without boards: every port has a "disk" that is a file in a folder, named after
    /// the port. It counts how often the disks were asked for (the password dialogs).
    struct Desk {
        dir: std::path::PathBuf,
        /// Ports where no board answers.
        dead: Vec<&'static str>,
        /// The password dialog is refused.
        refused: bool,
        dialogs: Mutex<Vec<usize>>,
    }

    impl Desk {
        fn new(dir: &std::path::Path) -> Self {
            Desk { dir: dir.to_path_buf(), dead: Vec::new(), refused: false, dialogs: Mutex::new(Vec::new()) }
        }

        fn disk(&self, port: &str) -> std::path::PathBuf {
            self.dir.join(format!("emmc-{port}"))
        }
    }

    impl Bench for Desk {
        fn start(&self, port: &str, events: Sink<'_>, cancel: &Cancel) -> Result<BlockDevice> {
            events(Event::Phase { phase: Phase::Boot });
            cancel.check()?;
            if self.dead.contains(&port) {
                bail!("no board answers on USB port {port}");
            }
            events(Event::Phase { phase: Phase::WaitDisk });
            // 1-2 -> serial ...0102, so each has its own name with the FIBER software.
            let digits: String = port.chars().filter(char::is_ascii_digit).map(|c| format!("0{c}")).collect();
            Ok(BlockDevice {
                path: self.disk(port).display().to_string(),
                size: 8 << 30,
                vendor: None,
                model: None,
                removable: true,
                usb_port: Some(port.to_string()),
                usb_serial: Some(format!("10000000a1b2{digits}")),
            })
        }

        fn still(&self, _port: &str, _emmc: &BlockDevice) -> Result<()> {
            Ok(())
        }

        fn open(&self, disks: &[&BlockDevice]) -> Vec<Result<(Target, File)>> {
            self.dialogs.lock().unwrap().push(disks.len());
            disks
                .iter()
                .map(|d| {
                    if self.refused {
                        bail!("writing the FIBER was not allowed (the password dialog was closed or refused)");
                    }
                    let path = std::path::PathBuf::from(&d.path);
                    let file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&path)?;
                    Ok((Target::File(path), file))
                })
                .collect()
        }

        fn release(&self, emmc: &BlockDevice) -> Option<String> {
            Some(format!("{} let go", emmc.path))
        }
    }

    /// What a batch said: every event with its port, and how each port ended.
    #[derive(Default)]
    struct Heard {
        events: Mutex<Vec<(String, Event)>>,
        ended: Mutex<Vec<(String, std::result::Result<Report, String>)>>,
    }

    impl Heard {
        fn phases(&self, port: &str) -> Vec<Phase> {
            let events = self.events.lock().unwrap();
            events
                .iter()
                .filter_map(|(p, e)| match e {
                    Event::Phase { phase } if p == port => Some(*phase),
                    _ => None,
                })
                .collect()
        }

        fn ended(&self, port: &str) -> std::result::Result<Report, String> {
            let ended = self.ended.lock().unwrap();
            let mine: Vec<_> = ended.iter().filter(|(p, _)| p == port).collect();
            assert_eq!(mine.len(), 1, "{port} ends once");
            mine[0].1.clone()
        }
    }

    fn on(ports: &[&str]) -> Vec<Device> {
        ports.iter().map(|p| Device { port: p.to_string(), hostname: None }).collect()
    }

    /// Runs a batch on a desk with the test image and gives what it said.
    fn run_batch(desk: &Desk, s: &Settings, devices: &[Device], order: Order, cancel: &Cancel) -> Heard {
        let raw = desk.dir.join("os.img");
        std::fs::write(&raw, testimg::disk()).unwrap();
        let heard = Heard::default();
        batch(
            desk,
            &prepared(&raw),
            s,
            devices,
            order,
            &|port, e| heard.events.lock().unwrap().push((port.to_string(), e)),
            &|port, r| heard.ended.lock().unwrap().push((port.to_string(), r.map_err(|e| format!("{e:#}")))),
            cancel,
        );
        heard
    }

    const WHOLE: [Phase; 7] =
        [Phase::Boot, Phase::WaitDisk, Phase::Ready, Phase::Settings, Phase::Write, Phase::Verify, Phase::Done];

    #[test]
    fn writes_every_fiber_of_a_batch_with_one_password_dialog() {
        for order in [Order::Together, Order::OneByOne] {
            let t = tempfile::tempdir().unwrap();
            let desk = Desk::new(t.path());
            let heard = run_batch(&desk, &settings(), &on(&["1-1", "1-2", "2-3"]), order, &Cancel::new());
            assert_eq!(*desk.dialogs.lock().unwrap(), [3], "one dialog for the three disks");
            for (port, name) in [("1-1", "fiber-a1b20101"), ("1-2", "fiber-a1b20102"), ("2-3", "fiber-a1b20203")] {
                assert_eq!(heard.phases(port), WHOLE, "{port}, {order:?}");
                let report = heard.ended(port).unwrap();
                // With the FIBER software each is named after its own serial number.
                assert_eq!(report.hostname, name);
                assert_eq!(report.hub_name.as_deref(), Some(name));
                let written = std::fs::read(desk.disk(port)).unwrap();
                assert_eq!(written.len(), testimg::SIZE);
                let user = testimg::boot_file(&written, "user-data").unwrap();
                assert!(user.contains(&format!("hostname: \"{name}\"\n")), "{user}");
            }
        }
    }

    #[test]
    fn one_after_another_writes_one_at_a_time_and_in_their_order() {
        let t = tempfile::tempdir().unwrap();
        let desk = Desk::new(t.path());
        let heard = run_batch(&desk, &settings(), &on(&["2-1", "1-4", "1-2"]), Order::OneByOne, &Cancel::new());
        // All are started before the first is written; then each from its settings to its end.
        let events = heard.events.lock().unwrap();
        let turns: Vec<(&str, Phase)> = events
            .iter()
            .filter_map(|(port, e)| match e {
                Event::Phase { phase } if matches!(phase, Phase::Ready | Phase::Settings | Phase::Done) => {
                    Some((port.as_str(), *phase))
                }
                _ => None,
            })
            .collect();
        let ready: Vec<&str> = turns.iter().filter(|(_, p)| *p == Phase::Ready).map(|(port, _)| *port).collect();
        assert_eq!(ready.len(), 3);
        let after: Vec<(&str, Phase)> = turns.into_iter().filter(|(_, p)| *p != Phase::Ready).collect();
        assert_eq!(
            after,
            [
                ("2-1", Phase::Settings),
                ("2-1", Phase::Done),
                ("1-4", Phase::Settings),
                ("1-4", Phase::Done),
                ("1-2", Phase::Settings),
                ("1-2", Phase::Done)
            ]
        );
    }

    #[test]
    fn a_fiber_that_fails_does_not_stop_the_others() {
        let t = tempfile::tempdir().unwrap();
        let desk = Desk { dead: vec!["1-2"], ..Desk::new(t.path()) };
        let heard = run_batch(&desk, &settings(), &on(&["1-1", "1-2", "1-3"]), Order::Together, &Cancel::new());
        assert!(heard.ended("1-1").is_ok());
        assert_eq!(heard.ended("1-2").unwrap_err(), "no board answers on USB port 1-2");
        assert!(heard.ended("1-3").is_ok());
        // Only the two that started were asked for.
        assert_eq!(*desk.dialogs.lock().unwrap(), [2]);
        assert_eq!(heard.phases("1-2"), [Phase::Boot]);
        assert!(!desk.disk("1-2").exists());
    }

    #[test]
    fn a_refused_password_dialog_writes_none() {
        let t = tempfile::tempdir().unwrap();
        let desk = Desk { refused: true, ..Desk::new(t.path()) };
        let heard = run_batch(&desk, &settings(), &on(&["1-1", "1-2"]), Order::OneByOne, &Cancel::new());
        for port in ["1-1", "1-2"] {
            assert!(heard.ended(port).unwrap_err().contains("not allowed"));
            assert_eq!(heard.phases(port), [Phase::Boot, Phase::WaitDisk, Phase::Ready]);
        }
    }

    #[test]
    fn without_the_fiber_software_each_gets_the_name_it_was_given() {
        let t = tempfile::tempdir().unwrap();
        let desk = Desk::new(t.path());
        let mut plain = settings();
        plain.components.clear();
        let devices = [
            Device { port: "1-1".into(), hostname: Some("bench".into()) },
            Device { port: "1-2".into(), hostname: Some("bench-2".into()) },
            // A name that no computer may have fails that FIBER alone.
            Device { port: "1-3".into(), hostname: Some("-bench".into()) },
        ];
        let heard = run_batch(&desk, &plain, &devices, Order::Together, &Cancel::new());
        for (port, name) in [("1-1", "bench"), ("1-2", "bench-2")] {
            assert_eq!(heard.ended(port).unwrap().hostname, name);
            let user = testimg::boot_file(&std::fs::read(desk.disk(port)).unwrap(), "user-data").unwrap();
            assert!(user.contains(&format!("hostname: \"{name}\"\n")), "{user}");
        }
        assert!(heard.ended("1-3").is_err());
        assert!(heard.phases("1-3").is_empty(), "not even started");
    }

    #[test]
    fn refuses_too_many_fibers_and_a_port_named_twice() {
        let t = tempfile::tempdir().unwrap();
        let desk = Desk::new(t.path());
        let nine = ["1-1", "1-2", "1-3", "1-4", "2-1", "2-2", "2-3", "2-4", "3-1"];
        let heard = run_batch(&desk, &settings(), &on(&nine), Order::Together, &Cancel::new());
        for port in nine {
            assert!(heard.ended(port).unwrap_err().contains("at most 8"));
        }
        let heard = run_batch(&desk, &settings(), &on(&["1-1", "1-1"]), Order::Together, &Cancel::new());
        assert_eq!(heard.ended.lock().unwrap().len(), 2);
        assert!(desk.dialogs.lock().unwrap().is_empty());
        assert!(heard.events.lock().unwrap().is_empty());
        // Eight is fine.
        let heard = run_batch(&desk, &settings(), &on(&nine[..8]), Order::Together, &Cancel::new());
        assert!(nine[..8].iter().all(|port| heard.ended(port).is_ok()));
        assert_eq!(*desk.dialogs.lock().unwrap(), [8]);
    }

    #[test]
    fn a_cancelled_batch_writes_nothing_more() {
        let t = tempfile::tempdir().unwrap();
        let desk = Desk::new(t.path());
        let cancel = Cancel::new();
        cancel.cancel();
        let heard = run_batch(&desk, &settings(), &on(&["1-1", "1-2"]), Order::OneByOne, &cancel);
        for port in ["1-1", "1-2"] {
            assert_eq!(heard.ended(port).unwrap_err(), "cancelled");
        }
        assert!(desk.dialogs.lock().unwrap().is_empty());
    }

    /// The settings change a few files of the boot partition: what a batch keeps in memory for
    /// each FIBER is those parts, not the whole partition.
    #[test]
    fn keeps_only_what_the_settings_changed() {
        let t = tempfile::tempdir().unwrap();
        let raw = t.path().join("os.img");
        let original = testimg::disk();
        std::fs::write(&raw, &original).unwrap();
        let p = personalise(&prepared(&raw), &settings(), &|_| {}).unwrap();
        let (start, end) = (testimg::BOOT_START as u64, (testimg::BOOT_START + testimg::BOOT_SIZE) as u64);
        let kept: usize = p.patches.iter().map(|patch| patch.bytes.len()).sum();
        assert!(kept > 0 && kept < testimg::BOOT_SIZE / 8, "{kept} bytes kept in {} parts", p.patches.len());
        let mut after = start;
        for patch in &p.patches {
            // In the partition, in order, apart from each other, and really different.
            assert!(patch.offset >= after && patch.offset + patch.bytes.len() as u64 <= end);
            after = patch.offset + patch.bytes.len() as u64 + 1;
            let at = patch.offset as usize;
            assert_ne!(patch.bytes[..], original[at..at + patch.bytes.len()]);
        }
        // Laid over the image, they give the boot partition with the settings in it.
        let mut whole = original.clone();
        for patch in &p.patches {
            let at = patch.offset as usize;
            whole[at..at + patch.bytes.len()].copy_from_slice(&patch.bytes);
        }
        assert!(testimg::boot_file(&whole, "user-data").unwrap().contains("hostname: \"fiber\"\n"));
        assert_eq!(testimg::boot_file(&whole, "config.txt").unwrap(), "arm_64bit=1\n");

        // Nothing changed is nothing kept.
        let mut image = File::open(&raw).unwrap();
        let same = &original[testimg::BOOT_START..testimg::BOOT_START + testimg::BOOT_SIZE];
        assert_eq!(changed(&mut image, start, same).unwrap(), []);
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
                ("fiber-seed.yaml", "old seed\n"),
                ("fiber-cm4-setup.sh", "older script\n"),
                ("fiber-setup.log", "old log\n"),
            ],
        );
        let raw = t.path().join("os.img");
        std::fs::write(&raw, testimg::disk_around(&boot)).unwrap();
        let out = t.path().join("out.img");
        let mut plain = settings();
        plain.components.clear();
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
