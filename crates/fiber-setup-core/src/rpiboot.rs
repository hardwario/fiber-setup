// SPDX-License-Identifier: Apache-2.0
//
// Derived from rpiboot, https://github.com/raspberrypi/usbboot (main.c and bootfiles.c at commit
// d3d51bbe13593555f03a0470d8ca750be1518670), which is licensed under the Apache License,
// Version 2.0. Changed by HARDWARIO a.s. in 2026: written anew in Rust on the nusb crate, for the
// boards and the one way of starting them that FIBER Setup needs. This file is under the same
// license (LICENSES/Apache-2.0.txt in this repository); the rest of FIBER Setup is MIT.

//! Boots the FIBER's CM4 over USB into the mass-storage gadget, which shows its eMMC as a disk on
//! this computer.
//!
//! This is the work of Raspberry Pi's `rpiboot` (github.com/raspberrypi/usbboot), done here in
//! Rust over the `nusb` crate: the app carries no program of its own for it and no libusb. The
//! steps, their order and their timing are rpiboot's:
//!
//! 1. The boot ROM (`0a5c:2711`; the index of its serial-number string is 0 or 3) is sent the
//!    boot code, `bootcode4.bin`: a header of 24 bytes with the length, then the file. It
//!    answers with a status, or just leaves.
//! 2. The boot code runs and the board comes back on the same port as a new USB device. Now it
//!    asks: for the size of a file, for the file, until it has all it needs (the last is
//!    `boot.img`, the Linux that is the gadget). It is answered from the gadget's folder and
//!    from `bootfiles.bin` in it, a tar archive of the firmware's files.
//!
//! Everything goes through vendor requests on the control endpoint that carry a length in
//! `wValue` and `wIndex`. An outgoing one announces so many bytes, which follow on the bulk
//! endpoint; an incoming one fetches what the board has to say.

use crate::event::Cancel;
use anyhow::{Context, Result, anyhow, bail};
use nusb::MaybeFuture;
use nusb::transfer::{Bulk, ControlIn, ControlOut, ControlType, Out, Recipient, TransferError};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
pub struct Tools {
    /// The `mass-storage-gadget64` directory (`boot.img`, `bootfiles.bin`, `config.txt`).
    pub gadget: PathBuf,
}

/// The gadget needs these; a directory without them is someone else's.
fn is_gadget(dir: &Path) -> bool {
    dir.join("boot.img").is_file() && (dir.join("bootfiles.bin").is_file() || dir.join("bootcode4.bin").is_file())
}

/// Finds the gadget's files: `FIBER_SETUP_USBBOOT` (a usbboot checkout or bundle), the
/// directories the app passes (its bundled resources), a usbboot checkout in
/// `~/github/raspberrypi/usbboot`, then where a system's `rpiboot` package keeps them.
pub fn find_tools(extra: &[PathBuf]) -> Option<Tools> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("FIBER_SETUP_USBBOOT") {
        dirs.push(PathBuf::from(d));
    }
    dirs.extend(extra.iter().cloned());
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("github/raspberrypi/usbboot"));
    }
    if !cfg!(windows) {
        dirs.extend(["/usr/share/rpiboot", "/usr/local/share/rpiboot"].map(PathBuf::from));
    }
    dirs.iter()
        .map(|dir| dir.join("mass-storage-gadget64"))
        .find(|gadget| is_gadget(gadget))
        .map(|gadget| Tools { gadget })
}

/// The processor whose boot ROM answers, told by the USB product id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chip {
    /// The Compute Modules before the CM4.
    Bcm2710,
    /// The CM4, which a FIBER has.
    Bcm2711,
    /// The CM5.
    Bcm2712,
}

impl Chip {
    fn of(product_id: u16) -> Chip {
        match product_id {
            0x2711 => Chip::Bcm2711,
            0x2712 => Chip::Bcm2712,
            _ => Chip::Bcm2710,
        }
    }

    /// Where this processor's files are, in the gadget's folder and in `bootfiles.bin`.
    fn folder(self) -> &'static str {
        match self {
            Chip::Bcm2710 => "2710",
            Chip::Bcm2711 => "2711",
            Chip::Bcm2712 => "2712",
        }
    }

    /// The file its boot ROM is sent.
    fn boot_code(self) -> &'static str {
        match self {
            Chip::Bcm2710 => "bootcode.bin",
            Chip::Bcm2711 => "bootcode4.bin",
            Chip::Bcm2712 => "bootcode5.bin",
        }
    }
}

/// The files a board may ask for: the gadget's folder, and the firmware archive in it.
struct Files {
    dir: PathBuf,
    /// `bootfiles.bin`, a tar archive; a folder may also do without one.
    archive: Option<Vec<u8>>,
}

impl Files {
    fn open(dir: &Path) -> Result<Files> {
        let path = dir.join("bootfiles.bin");
        let archive = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Ok(Files { dir: dir.to_path_buf(), archive })
    }

    /// A file for the board, looked up the way rpiboot does: `<folder>/<chip>/<name>` first (a
    /// file put there replaces the archive's), then `<chip>/<name>` in the archive, then
    /// `<folder>/<name>`.
    fn get(&self, chip: Chip, name: &str) -> Option<Vec<u8>> {
        let path = inside(name)?;
        if let Ok(data) = std::fs::read(self.dir.join(chip.folder()).join(&path)) {
            return Some(data);
        }
        if let Some(archive) = &self.archive
            && let Some(data) = tar_entry(archive, &format!("{}/{name}", chip.folder()))
        {
            return Some(data.to_vec());
        }
        std::fs::read(self.dir.join(&path)).ok()
    }
}

/// The name a board asked for as a path inside the gadget's folder; `None` for a name that
/// would lead out of it. The name comes from the board, so it is not trusted.
fn inside(name: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for part in name.split('/').filter(|part| !part.is_empty() && *part != ".") {
        if part == ".." || part.contains(['\\', ':', '\0']) {
            return None;
        }
        path.push(part);
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

/// A file in a tar archive, its name compared without regard to case (as rpiboot does).
fn tar_entry<'a>(archive: &'a [u8], name: &str) -> Option<&'a [u8]> {
    const BLOCK: usize = 512;
    let mut at = 0;
    while at + BLOCK <= archive.len() {
        let header = &archive[at..at + BLOCK];
        let entry = &header[..header[..99].iter().position(|&b| b == 0).unwrap_or(99)];
        // The size in octal digits, after spaces and up to whatever ends them.
        let size = header[124..136]
            .iter()
            .skip_while(|b| **b == b' ')
            .take_while(|b| (b'0'..=b'7').contains(b))
            .fold(0usize, |size, b| size * 8 + usize::from(b - b'0'));
        let data = at + BLOCK;
        if data + size > archive.len() {
            return None;
        }
        if entry.eq_ignore_ascii_case(name.as_bytes()) {
            return Some(&archive[data..data + size]);
        }
        at = data + size.div_ceil(BLOCK) * BLOCK;
    }
    None
}

/// Why a transfer did not happen.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LinkError {
    /// The board left the bus. It does so to start what it was sent, so this is often good news.
    Gone,
    /// No answer in time.
    Timeout,
    Other(String),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::Gone => f.write_str("the board left the USB port"),
            LinkError::Timeout => f.write_str("no answer in time"),
            LinkError::Other(text) => f.write_str(text),
        }
    }
}

/// The USB connection to a board in boot mode. The real one is [`UsbLink`]; the tests have a
/// board of their own.
pub(crate) trait Link {
    /// Tells the board that `len` bytes follow (or, with 0, that nothing does).
    fn announce(&mut self, len: u32) -> Result<(), LinkError>;
    /// The announced bytes.
    fn send(&mut self, data: &[u8]) -> Result<(), LinkError>;
    /// What the board has to say, `len` bytes of it.
    fn ask(&mut self, len: u16) -> Result<Vec<u8>, LinkError>;
}

/// A board in boot mode, opened.
pub(crate) struct Board<L> {
    pub product_id: u16,
    /// The index of the serial-number string in its descriptor: 0 or 3 while the boot ROM
    /// answers, another once the boot code runs.
    pub serial_index: u8,
    pub link: L,
}

pub(crate) enum OpenError {
    /// Not there, or not ready: worth another look.
    NotYet(String),
    /// This user may not use it. Right after a board appears that can be a matter of a moment
    /// (Linux sets its permissions a little later), so it counts only when it stays so.
    Denied,
}

/// So many looks in a row at a board this user may not use, and it is taken for a fact.
const DENIED_LOOKS: u32 = 5;

/// What the thread that does the work reports.
enum Note {
    /// A line for the log.
    Line(String),
    /// Why the board cannot be opened yet.
    NotReady(String),
}

/// Where the boards are. The real one is [`UsbBus`].
pub(crate) trait Bus {
    type Link: Link;
    /// Whether a board in boot mode is on the port now.
    fn present(&self, port: &str) -> bool;
    fn open(&self, port: &str) -> Result<Board<Self::Link>, OpenError>;
}

/// The waits between the steps. rpiboot's are these; the tests do not wait.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pace {
    /// Between two looks for the board.
    look: Duration,
    /// For the board to settle: before it is opened, before the boot ROM is asked how it went,
    /// and after each of the two steps.
    settle: Duration,
}

const PACE: Pace = Pace { look: Duration::from_millis(100), settle: Duration::from_secs(1) };

/// The board's messages while it asks for files: a number and a name, 260 bytes.
const MESSAGE: u16 = 4 + 256;
const GET_FILE_SIZE: i32 = 0;
const READ_FILE: i32 = 1;
const DONE: i32 = 2;

/// Announces the bytes and sends them.
fn write(link: &mut impl Link, data: &[u8]) -> Result<(), LinkError> {
    let len = u32::try_from(data.len()).map_err(|_| LinkError::Other("the file is too large".into()))?;
    link.announce(len)?;
    if data.is_empty() { Ok(()) } else { link.send(data) }
}

/// Step 1: the boot code to the boot ROM.
fn send_boot_code(link: &mut impl Link, code: &[u8], pace: Pace) -> Result<(), LinkError> {
    // The length, and room for a signature that only the oldest boards check.
    let mut header = [0u8; 24];
    let len = i32::try_from(code.len()).map_err(|_| LinkError::Other("the boot code is too large".into()))?;
    header[..4].copy_from_slice(&len.to_le_bytes());
    write(link, &header)?;
    write(link, code)?;
    std::thread::sleep(pace.settle);
    match link.ask(4) {
        // The ROM may leave before it answers; it comes back running the boot code.
        Err(LinkError::Gone) => Ok(()),
        Err(e) => Err(e),
        Ok(answer) => match <[u8; 4]>::try_from(answer.as_slice()).map(i32::from_le_bytes) {
            Ok(0) => Ok(()),
            Ok(status) => Err(LinkError::Other(format!("the boot ROM answered 0x{status:x}"))),
            Err(_) => Err(LinkError::Other(format!("the boot ROM's answer had {} bytes", answer.len()))),
        },
    }
}

/// How serving the boot code's files ended.
#[derive(Debug, PartialEq, Eq)]
enum Served {
    /// The boot code has what it asked for: it said so, or left with its files read.
    Done,
    /// It left, or a file did not go through, before it had what it needs: it was reset, or
    /// the link broke. It has to be started again, from its boot ROM.
    Unserved,
}

/// What the FIBER is told to do when its boot code does not take part: only RESET brings it
/// back to its boot ROM.
const PRESS_RESET: &str = "it does not ask for its files; press RESET on it";

/// Step 2: the files the boot code asks for, until it says it is done or leaves.
///
/// A boot code that says nothing is one left over from a start that broke off (the app was
/// closed, a cable moved): it waits for the rest of a file nobody sends any more. It is waited
/// with, and said so, until RESET takes it away; then it is [`Served::Unserved`].
fn serve_files(
    link: &mut impl Link,
    files: &Files,
    chip: Chip,
    pace: Pace,
    note: &dyn Fn(Note),
    stop: &Cancel,
    still_there: &dyn Fn() -> bool,
) -> Result<Served> {
    let log = &|line: String| note(Note::Line(line));
    // The file whose size was asked for last: the one the next read gets.
    let mut open: Option<Vec<u8>> = None;
    // Files it has read, whether it has said anything at all, and whether RESET was asked for.
    let (mut read, mut spoke, mut told) = (0, false, false);
    loop {
        stop.check()?;
        let message = match link.ask(MESSAGE) {
            Ok(message) => message,
            Err(e) => {
                let gone = matches!(e, LinkError::Gone) || (!matches!(e, LinkError::Timeout) && !still_there());
                // Left with files read: gone to start the gadget. Left with none: it was reset.
                if gone && read > 0 {
                    break;
                }
                if gone {
                    return Ok(Served::Unserved);
                }
                if !spoke && !told {
                    note(Note::NotReady(PRESS_RESET.into()));
                    told = true;
                }
                std::thread::sleep(pace.settle);
                continue;
            }
        };
        spoke = true;
        if message.len() != usize::from(MESSAGE) {
            bail!("the FIBER sent an incomplete request ({} bytes)", message.len());
        }
        let command = i32::from_le_bytes([message[0], message[1], message[2], message[3]]);
        let name = &message[4..];
        // The last byte ends the name whatever it is.
        let name = String::from_utf8_lossy(&name[..name[..255].iter().position(|&b| b == 0).unwrap_or(255)]);
        if !(GET_FILE_SIZE..=DONE).contains(&command) {
            bail!("the FIBER sent a request that is not known ({command})");
        }
        // No name is another way to say done.
        if name.is_empty() {
            let _ = write(link, &[]);
            break;
        }
        // Not a file: something the board tells about itself (`*NAME*value`).
        if let Some(fact) = name.strip_prefix('*')
            && command != DONE
        {
            if let Some((key, value)) = fact.split_once('*') {
                log(format!("{key}: {value}"));
            }
            let _ = write(link, &[]);
            continue;
        }
        match command {
            GET_FILE_SIZE => {
                open = files.get(chip, &name);
                match &open {
                    Some(data) => {
                        let len = u32::try_from(data.len()).with_context(|| format!("{name} is too large"))?;
                        if let Err(e) = link.announce(len) {
                            log(format!("Telling the FIBER the size of {name} did not go through: {e}"));
                            return Ok(Served::Unserved);
                        }
                    }
                    None => {
                        let _ = write(link, &[]);
                        log(format!("Cannot open file {name}"));
                    }
                }
            }
            READ_FILE => match open.take() {
                Some(data) => {
                    log(format!("File read: {name}"));
                    // Not the end: the board is waited for again, and RESET starts it anew.
                    if let Err(e) = write(link, &data) {
                        log(format!("Sending {name} to the FIBER did not go through: {e}"));
                        return Ok(Served::Unserved);
                    }
                    read += 1;
                }
                None => {
                    let _ = write(link, &[]);
                }
            },
            _ => break,
        }
    }
    log("Second stage boot server done".into());
    Ok(Served::Done)
}

/// Both steps for the board on `port`, however long that takes: the caller sets the limit.
fn boot<B: Bus>(bus: &B, files: &Files, port: &str, pace: Pace, note: &dyn Fn(Note), stop: &Cancel) -> Result<()> {
    let log = &|line: String| note(Note::Line(line));
    log(format!("Waiting for the FIBER on USB port {port}"));
    let mut waiting_for = String::new();
    let mut denied = 0;
    loop {
        let mut board = loop {
            stop.check()?;
            if bus.present(port) {
                std::thread::sleep(pace.settle);
                let why = match bus.open(port) {
                    Ok(board) => break board,
                    Err(OpenError::Denied) => {
                        denied += 1;
                        if denied >= DENIED_LOOKS {
                            bail!("no permission to use the USB device; allow USB access first");
                        }
                        "no permission to use it".to_string()
                    }
                    Err(OpenError::NotYet(why)) => {
                        denied = 0;
                        why
                    }
                };
                // Said once, not at every look.
                if why != waiting_for {
                    note(Note::NotReady(why.clone()));
                    waiting_for = why;
                }
            }
            std::thread::sleep(pace.look);
        };
        denied = 0;
        let chip = Chip::of(board.product_id);
        if board.serial_index == 0 || board.serial_index == 3 {
            let name = chip.boot_code();
            let code = files.get(chip, name).with_context(|| {
                format!(
                    "this board (0a5c:{:04x}) needs {name}, which is not among the gadget's files",
                    board.product_id
                )
            })?;
            log(format!("Sending {name}"));
            // Not the end if it fails: a ROM that is still there, or back, gets it again.
            if let Err(e) = send_boot_code(&mut board.link, &code, pace) {
                log(format!("The boot ROM did not confirm {name}: {e}"));
            }
        } else {
            log("Second stage boot server".into());
            if serve_files(&mut board.link, files, chip, pace, note, stop, &|| bus.present(port))? == Served::Done {
                return Ok(());
            }
            // Reset, or cut off: back to waiting for it, and its boot ROM gets the boot code again.
            log(format!("The FIBER did not get all its files; waiting for it on USB port {port} again"));
            waiting_for.clear();
        }
        drop(board);
        std::thread::sleep(pace.settle);
    }
}

/// Whether a board waits in boot mode on `port`: in its boot ROM, or in the boot code it was sent.
pub fn waiting(port: &str) -> bool {
    UsbBus.present(port)
}

/// Boots the board on `port` into the gadget and returns when the board has all its files.
/// What happens on the way goes to `on_line`, line by line.
pub fn run(
    tools: &Tools,
    port: &str,
    timeout: Duration,
    on_line: &(dyn Fn(String) + Send + Sync),
    cancel: &Cancel,
) -> Result<()> {
    run_on(UsbBus, &tools.gadget, port, timeout, PACE, on_line, cancel)
}

/// The work is done on a thread of its own: a transfer may wait for the board for seconds, and
/// neither Cancel nor the time limit should wait with it.
fn run_on<B: Bus + Send + 'static>(
    bus: B,
    gadget: &Path,
    port: &str,
    timeout: Duration,
    pace: Pace,
    on_line: &(dyn Fn(String) + Send + Sync),
    cancel: &Cancel,
) -> Result<()> {
    let files = Files::open(gadget)?;
    let deadline = Instant::now() + timeout;
    // Tells the thread to stop when this function has given up on it.
    let stop = Cancel::new();
    let (notes, incoming) = mpsc::channel::<Note>();
    let worker = {
        let (stop, port) = (stop.clone(), port.to_string());
        std::thread::spawn(move || boot(&bus, &files, &port, pace, &|note| drop(notes.send(note)), &stop))
    };
    // Why the board could not be opened, when that is what the time went on.
    let mut not_ready: Option<String> = None;
    loop {
        match incoming.recv_timeout(Duration::from_millis(100)) {
            Ok(Note::Line(line)) => {
                not_ready = None;
                on_line(line);
            }
            Ok(Note::NotReady(why)) => {
                on_line(format!("The FIBER is not ready yet: {why}"));
                not_ready = Some(why);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // The thread is through, and so are its notes.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if cancel.is_cancelled() {
            stop.cancel();
            bail!("cancelled");
        }
        if Instant::now() > deadline {
            stop.cancel();
            let secs = timeout.as_secs();
            match not_ready {
                Some(why) => bail!(
                    "the FIBER on USB port {port} could not be started within {secs} s ({why}); press RESET and try again"
                ),
                None => bail!(
                    "the FIBER did not answer on USB port {port} within {secs} s; check that the jumper is on BOOT, press RESET and try again"
                ),
            }
        }
    }
    worker.join().map_err(|_| anyhow!("starting the FIBER over USB failed unexpectedly"))?
}

/// The USB vendor of the boot ROM (Broadcom) and the products it shows as: BCM2710 (two), BCM2711
/// and BCM2712. The same as `usb::BROADCOM_VID` and `usb::BOOT_PIDS`, as numbers.
const VENDOR: u16 = 0x0a5c;
const PRODUCTS: [u16; 4] = [0x2763, 0x2764, 0x2711, 0x2712];

/// rpiboot's limits: a second for an announcement, five for each piece of a file, twenty for
/// the board to say something.
const ANNOUNCE_TIMEOUT: Duration = Duration::from_secs(1);
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const ASK_TIMEOUT: Duration = Duration::from_secs(20);
/// A file goes out in pieces of this size.
const PIECE: usize = 16 << 10;

impl From<TransferError> for LinkError {
    fn from(e: TransferError) -> LinkError {
        match e {
            TransferError::Cancelled => LinkError::Timeout,
            // What a system reports when a device goes away in the middle of a transfer differs,
            // and much of it has no name of its own here.
            TransferError::Disconnected | TransferError::Fault | TransferError::Unknown(_) => LinkError::Gone,
            other => LinkError::Other(other.to_string()),
        }
    }
}

/// The boards on this computer's USB.
struct UsbBus;

impl UsbBus {
    fn find(port: &str) -> Option<nusb::DeviceInfo> {
        nusb::list_devices().wait().ok()?.find(|d| {
            d.vendor_id() == VENDOR
                && PRODUCTS.contains(&d.product_id())
                && crate::usb::port_of(d).as_deref() == Some(port)
        })
    }
}

impl Bus for UsbBus {
    type Link = UsbLink;

    fn present(&self, port: &str) -> bool {
        Self::find(port).is_some()
    }

    fn open(&self, port: &str) -> Result<Board<UsbLink>, OpenError> {
        let not_yet = |what: &str, e: nusb::Error| OpenError::NotYet(format!("{what}: {e}"));
        let info = Self::find(port).ok_or_else(|| OpenError::NotYet("it left the USB port".into()))?;
        let device = info.open().wait().map_err(|e| match e.kind() {
            nusb::ErrorKind::PermissionDenied => OpenError::Denied,
            // Windows gives a board that just appeared its driver a moment later. (Without the
            // driver at all, the Connect step does not let the user come this far.)
            nusb::ErrorKind::Unsupported if cfg!(windows) => {
                OpenError::NotYet("Windows has no USB driver for it (yet)".into())
            }
            _ => not_yet("opening it", e),
        })?;
        let serial_index = device.device_descriptor().serial_number_string_index().map_or(0, |i| i.get());
        let interfaces = |device: &nusb::Device| device.active_configuration().ok().map(|c| c.num_interfaces());
        let interfaces = match interfaces(&device) {
            Some(n) => n,
            // macOS leaves a device alone that no driver of its own wants: it has to be told
            // to take its (only) configuration.
            None => {
                let first = device.configurations().next().map(|c| c.configuration_value());
                let first = first.ok_or_else(|| OpenError::NotYet("it has no USB configuration".into()))?;
                device.set_configuration(first).wait().map_err(|e| not_yet("configuring it", e))?;
                interfaces(&device).ok_or_else(|| OpenError::NotYet("it did not take its USB configuration".into()))?
            }
        };
        // The oldest boards have a disk of their own in front of the interface to talk to.
        let (interface, endpoint) = if interfaces == 1 { (0, 0x01) } else { (1, 0x03) };
        let interface = device.claim_interface(interface).wait().map_err(|e| not_yet("claiming its interface", e))?;
        let out = interface.endpoint::<Bulk, Out>(endpoint).map_err(|e| not_yet("opening its endpoint", e))?;
        Ok(Board { product_id: info.product_id(), serial_index, link: UsbLink { interface, out } })
    }
}

/// A board on USB.
struct UsbLink {
    interface: nusb::Interface,
    out: nusb::Endpoint<Bulk, Out>,
}

impl Link for UsbLink {
    fn announce(&mut self, len: u32) -> Result<(), LinkError> {
        let request = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: 0,
            value: (len & 0xffff) as u16,
            index: (len >> 16) as u16,
            data: &[],
        };
        Ok(self.interface.control_out(request, ANNOUNCE_TIMEOUT).wait()?)
    }

    fn send(&mut self, data: &[u8]) -> Result<(), LinkError> {
        let mut rest = data;
        while !rest.is_empty() {
            let piece = &rest[..rest.len().min(PIECE)];
            let done = self.out.transfer_blocking(piece.to_vec().into(), SEND_TIMEOUT);
            done.status?;
            if done.actual_len == 0 {
                return Err(LinkError::Other("the board took no more of the file".into()));
            }
            rest = &rest[done.actual_len.min(piece.len())..];
        }
        Ok(())
    }

    fn ask(&mut self, len: u16) -> Result<Vec<u8>, LinkError> {
        let request = ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: 0,
            value: len,
            index: 0,
            length: len,
        };
        Ok(self.interface.control_in(request, ASK_TIMEOUT).wait()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// What the board's boot code says next, when asked.
    #[derive(Debug, Clone)]
    enum Says {
        /// How large is this file?
        Size(&'static str),
        /// Send this file.
        Read(&'static str),
        /// Something about itself, `*NAME*value`.
        Fact(&'static str),
        Done,
        /// A request with no name: also done.
        NoName,
        /// A request with this number, which no board sends.
        Number(i32),
        /// A message of the wrong length.
        Short,
        /// Nothing for now.
        Silence,
        /// A stalled request, with the board still there.
        Stall,
        /// It leaves the bus: it has all it needs.
        Leaves,
        /// RESET is pressed: it is back in its boot ROM.
        Reset,
    }

    /// What the boot ROM does once it has the boot code.
    #[derive(Debug, Clone, Copy)]
    enum Rom {
        /// Answers with this status; 0 starts the boot code.
        Status(i32),
        /// Starts the boot code without answering.
        Leaves,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Stage {
        Rom,
        Code,
        /// Running the gadget: no longer a board in boot mode.
        Gadget,
    }

    /// A board as the tests have it: the boot ROM, then the boot code, seen from the computer.
    struct Sim {
        port: &'static str,
        product_id: u16,
        rom_serial: u8,
        stage: Stage,
        rom: VecDeque<Rom>,
        code: VecDeque<Says>,
        /// Looks that do not find the board yet, and opens that fail for a moment.
        absent: usize,
        busy: usize,
        /// Opens that this user has no permission for.
        denied: usize,
        /// How long an `ask` takes when the board has nothing to say.
        silence: Duration,
        /// Files whose sending breaks off, from the next one on.
        lost: usize,
        /// What arrived, in order: announcements and data.
        arrived: Vec<Arrived>,
        /// The boot codes the ROM was sent.
        boot_codes: Vec<Vec<u8>>,
        /// The answers to the boot code's requests: the announced length and the data, by request.
        answers: Vec<(String, u32, Option<Vec<u8>>)>,
        opens: usize,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Arrived {
        Announced(u32),
        Data(Vec<u8>),
    }

    impl Sim {
        fn new(code: Vec<Says>) -> Sim {
            Sim {
                port: "1-1",
                product_id: 0x2711,
                rom_serial: 0,
                stage: Stage::Rom,
                rom: VecDeque::from([Rom::Status(0)]),
                code: code.into(),
                absent: 0,
                busy: 0,
                denied: 0,
                silence: Duration::ZERO,
                lost: 0,
                arrived: Vec::new(),
                boot_codes: Vec::new(),
                answers: Vec::new(),
                opens: 0,
            }
        }
    }

    #[derive(Clone)]
    struct SimBus(Arc<Mutex<Sim>>);

    struct SimLink(Arc<Mutex<Sim>>);

    fn message(number: i32, name: &str) -> Vec<u8> {
        let mut m = vec![0u8; 260];
        m[..4].copy_from_slice(&number.to_le_bytes());
        m[4..4 + name.len()].copy_from_slice(name.as_bytes());
        m
    }

    impl Bus for SimBus {
        type Link = SimLink;

        fn present(&self, port: &str) -> bool {
            let mut sim = self.0.lock().unwrap();
            if sim.absent > 0 {
                sim.absent -= 1;
                return false;
            }
            port == sim.port && sim.stage != Stage::Gadget
        }

        fn open(&self, port: &str) -> Result<Board<SimLink>, OpenError> {
            let mut sim = self.0.lock().unwrap();
            assert_eq!(port, sim.port);
            sim.opens += 1;
            if sim.denied > 0 {
                sim.denied -= 1;
                return Err(OpenError::Denied);
            }
            if sim.busy > 0 {
                sim.busy -= 1;
                return Err(OpenError::NotYet("opening it: busy".into()));
            }
            if sim.stage == Stage::Gadget {
                return Err(OpenError::NotYet("it left the USB port".into()));
            }
            let serial_index = if sim.stage == Stage::Rom { sim.rom_serial } else { 1 };
            Ok(Board { product_id: sim.product_id, serial_index, link: SimLink(self.0.clone()) })
        }
    }

    impl Link for SimLink {
        fn announce(&mut self, len: u32) -> Result<(), LinkError> {
            let mut sim = self.0.lock().unwrap();
            match sim.stage {
                Stage::Rom => sim.arrived.push(Arrived::Announced(len)),
                Stage::Code => match sim.answers.last_mut() {
                    Some(answer) => answer.1 = len,
                    None => panic!("an announcement nobody asked for"),
                },
                Stage::Gadget => return Err(LinkError::Gone),
            }
            Ok(())
        }

        fn send(&mut self, data: &[u8]) -> Result<(), LinkError> {
            let mut sim = self.0.lock().unwrap();
            match sim.stage {
                Stage::Rom => sim.arrived.push(Arrived::Data(data.to_vec())),
                Stage::Code => {
                    if sim.lost > 0 {
                        sim.lost -= 1;
                        return Err(LinkError::Timeout);
                    }
                    match sim.answers.last_mut() {
                        Some(answer) => {
                            assert_eq!(answer.1 as usize, data.len(), "{}: other than announced", answer.0);
                            answer.2 = Some(data.to_vec());
                        }
                        None => panic!("data nobody asked for"),
                    }
                }
                Stage::Gadget => return Err(LinkError::Gone),
            }
            Ok(())
        }

        fn ask(&mut self, len: u16) -> Result<Vec<u8>, LinkError> {
            let mut sim = self.0.lock().unwrap();
            match sim.stage {
                Stage::Rom => {
                    assert_eq!(len, 4);
                    // The boot ROM takes a header that says how long the boot code is, then
                    // exactly that many bytes, each announced.
                    let arrived = std::mem::take(&mut sim.arrived);
                    let [Arrived::Announced(24), Arrived::Data(header), Arrived::Announced(n), Arrived::Data(code)] =
                        arrived.as_slice()
                    else {
                        panic!("the boot ROM got {arrived:?}");
                    };
                    assert_eq!(header.len(), 24);
                    assert_eq!(header[..4], (code.len() as i32).to_le_bytes());
                    assert!(header[4..].iter().all(|&b| b == 0));
                    assert_eq!(*n as usize, code.len());
                    sim.boot_codes.push(code.clone());
                    match sim.rom.pop_front().unwrap_or(Rom::Status(0)) {
                        Rom::Status(0) => {
                            sim.stage = Stage::Code;
                            Ok(0i32.to_le_bytes().to_vec())
                        }
                        Rom::Status(status) => Ok(status.to_le_bytes().to_vec()),
                        Rom::Leaves => {
                            sim.stage = Stage::Code;
                            Err(LinkError::Gone)
                        }
                    }
                }
                Stage::Code => {
                    assert_eq!(len, 260);
                    let says = sim.code.pop_front().unwrap_or(Says::Leaves);
                    let ask = |sim: &mut Sim, number: i32, name: &str| {
                        sim.answers.push((name.to_string(), u32::MAX, None));
                        Ok(message(number, name))
                    };
                    match says {
                        Says::Size(name) => ask(&mut sim, 0, name),
                        Says::Read(name) => ask(&mut sim, 1, name),
                        Says::Fact(fact) => ask(&mut sim, 0, fact),
                        Says::NoName => ask(&mut sim, 0, ""),
                        Says::Number(number) => ask(&mut sim, number, "file"),
                        Says::Done => {
                            sim.stage = Stage::Gadget;
                            Ok(message(2, "done"))
                        }
                        Says::Short => Ok(vec![0u8; 12]),
                        Says::Silence => {
                            let silence = sim.silence;
                            drop(sim);
                            std::thread::sleep(silence);
                            Err(LinkError::Timeout)
                        }
                        Says::Stall => Err(LinkError::Other("endpoint stalled".into())),
                        Says::Leaves => {
                            sim.stage = Stage::Gadget;
                            Err(LinkError::Gone)
                        }
                        Says::Reset => {
                            sim.stage = Stage::Rom;
                            Err(LinkError::Gone)
                        }
                    }
                }
                Stage::Gadget => Err(LinkError::Gone),
            }
        }
    }

    /// A tar archive of these files, as `bootfiles.bin` is one.
    fn tar(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, data) in files {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            header[100..107].copy_from_slice(b"0000644");
            header[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
            out.extend_from_slice(&header);
            out.extend_from_slice(data);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.resize(out.len() + 1024, 0);
        out
    }

    /// A gadget's folder: the firmware archive, the gadget itself, its settings, and one file
    /// that takes the place of the archive's.
    fn gadget() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let archive = tar(&[
            ("2711/", b""),
            ("2711/bootcode4.bin", &[4u8; 50_930]),
            ("2711/bootmain", &[0xB0; 2000]),
            ("2711/mcb.bin", b"from the archive"),
            ("2712/", b""),
            ("2712/bootcode5.bin", &[5u8; 700]),
            ("2712/bootmain", &[0xB5; 100]),
        ]);
        std::fs::write(dir.path().join("bootfiles.bin"), archive).unwrap();
        std::fs::write(dir.path().join("boot.img"), vec![0x1Bu8; 100_000]).unwrap();
        std::fs::write(dir.path().join("config.txt"), "boot_ramdisk=1\n").unwrap();
        std::fs::create_dir(dir.path().join("2711")).unwrap();
        std::fs::write(dir.path().join("2711/mcb.bin"), "from the folder").unwrap();
        dir
    }

    const QUICK: Pace = Pace { look: Duration::from_millis(1), settle: Duration::ZERO };

    /// Runs the whole of it against the board and gives back what happened and what was said.
    fn start(
        sim: Sim,
        gadget: &Path,
        timeout: Duration,
        cancel: &Cancel,
    ) -> (Result<()>, Vec<String>, Arc<Mutex<Sim>>) {
        let sim = Arc::new(Mutex::new(sim));
        let lines = Mutex::new(Vec::new());
        let result =
            run_on(SimBus(sim.clone()), gadget, "1-1", timeout, QUICK, &|l| lines.lock().unwrap().push(l), cancel);
        (result, lines.into_inner().unwrap(), sim)
    }

    fn boots(sim: Sim) -> (Vec<String>, Arc<Mutex<Sim>>) {
        let dir = gadget();
        let (result, lines, sim) = start(sim, dir.path(), Duration::from_secs(20), &Cancel::new());
        result.unwrap_or_else(|e| panic!("{e:#}\n{lines:#?}"));
        (lines, sim)
    }

    /// The answers the board got, as (name, announced length, data).
    fn answers(sim: &Arc<Mutex<Sim>>) -> Vec<(String, u32, Option<Vec<u8>>)> {
        std::mem::take(&mut sim.lock().unwrap().answers)
    }

    #[test]
    fn boots_a_cm4_into_the_gadget() {
        let (lines, sim) = boots(Sim::new(vec![
            Says::Fact("*USER_SERIAL_NUM*10000000a1b2c3d4"),
            Says::Size("config.txt"),
            Says::Read("config.txt"),
            Says::Size("pieeprom.sig"),
            Says::Size("bootmain"),
            Says::Read("bootmain"),
            Says::Size("mcb.bin"),
            Says::Read("mcb.bin"),
            Says::Size("boot.img"),
            Says::Read("boot.img"),
            Says::Read("boot.img"),
            Says::Done,
        ]));
        // The boot ROM got the CM4's boot code out of the archive, once.
        assert_eq!(sim.lock().unwrap().boot_codes, [vec![4u8; 50_930]]);
        // The ROM, then the boot code: opened twice.
        assert_eq!(sim.lock().unwrap().opens, 2);
        let size = |name: &str, len: usize| (name.to_string(), len as u32, None);
        let file = |name: &str, data: &[u8]| (name.to_string(), data.len() as u32, Some(data.to_vec()));
        assert_eq!(
            answers(&sim),
            [
                // A fact about the board is taken note of and answered with nothing.
                size("*USER_SERIAL_NUM*10000000a1b2c3d4", 0),
                size("config.txt", 15),
                file("config.txt", b"boot_ramdisk=1\n"),
                // A file that is not there has no length.
                size("pieeprom.sig", 0),
                // Out of the archive, under the processor's name.
                size("bootmain", 2000),
                file("bootmain", &[0xB0; 2000]),
                // A file in the folder takes the place of the archive's.
                size("mcb.bin", 15),
                file("mcb.bin", b"from the folder"),
                size("boot.img", 100_000),
                file("boot.img", &[0x1B; 100_000]),
                // A read without the question for the size before it gets nothing.
                size("boot.img", 0),
            ]
        );
        assert_eq!(
            lines,
            [
                "Waiting for the FIBER on USB port 1-1",
                "Sending bootcode4.bin",
                "Second stage boot server",
                "USER_SERIAL_NUM: 10000000a1b2c3d4",
                "File read: config.txt",
                "Cannot open file pieeprom.sig",
                "File read: bootmain",
                "File read: mcb.bin",
                "File read: boot.img",
                "Second stage boot server done",
            ]
        );
    }

    #[test]
    fn the_boot_rom_may_leave_without_an_answer_and_the_boot_code_without_a_word() {
        let mut sim = Sim::new(vec![Says::Size("boot.img"), Says::Read("boot.img"), Says::Leaves]);
        sim.rom = VecDeque::from([Rom::Leaves]);
        // A later CM4's boot ROM has a serial number, under index 3.
        sim.rom_serial = 3;
        let (lines, sim) = boots(sim);
        assert_eq!(sim.lock().unwrap().boot_codes.len(), 1);
        assert_eq!(answers(&sim).len(), 2);
        assert_eq!(lines.last().unwrap(), "Second stage boot server done");
    }

    #[test]
    fn a_request_without_a_name_ends_it_too() {
        let (_, sim) = boots(Sim::new(vec![Says::Size("config.txt"), Says::NoName, Says::Size("never asked")]));
        assert_eq!(answers(&sim), [("config.txt".to_string(), 15, None), (String::new(), 0, None)]);
    }

    #[test]
    fn the_boot_code_is_sent_again_until_the_rom_takes_it() {
        let mut sim = Sim::new(vec![Says::Done]);
        sim.rom = VecDeque::from([Rom::Status(0x80), Rom::Status(0)]);
        // Not there at the first looks, and not to be opened at once.
        sim.absent = 3;
        sim.busy = 2;
        let (lines, sim) = boots(sim);
        assert_eq!(sim.lock().unwrap().boot_codes.len(), 2);
        assert_eq!(
            lines,
            [
                "Waiting for the FIBER on USB port 1-1",
                "The FIBER is not ready yet: opening it: busy",
                "Sending bootcode4.bin",
                "The boot ROM did not confirm bootcode4.bin: the boot ROM answered 0x80",
                "Sending bootcode4.bin",
                "Second stage boot server",
                "Second stage boot server done",
            ]
        );
    }

    /// What was seen on a desk: a FIBER left in its boot code by a start that broke off. It is
    /// "found" like any board in boot mode, but says nothing; the app says to press RESET, and
    /// when that is done it starts the board from its boot ROM by itself.
    #[test]
    fn a_board_left_in_an_earlier_start_is_started_again_after_reset() {
        let mut sim = Sim::new(vec![
            Says::Silence,
            Says::Silence,
            Says::Reset,
            Says::Size("boot.img"),
            Says::Read("boot.img"),
            Says::Leaves,
        ]);
        sim.stage = Stage::Code;
        let (lines, sim) = boots(sim);
        assert_eq!(
            lines,
            [
                "Waiting for the FIBER on USB port 1-1",
                "Second stage boot server",
                "The FIBER is not ready yet: it does not ask for its files; press RESET on it",
                "The FIBER did not get all its files; waiting for it on USB port 1-1 again",
                "Sending bootcode4.bin",
                "Second stage boot server",
                "File read: boot.img",
                "Second stage boot server done",
            ]
        );
        assert_eq!(sim.lock().unwrap().boot_codes.len(), 1);
    }

    /// A file that does not go through (a cable moved, RESET pressed in the middle) is not the
    /// end of the write: the board is waited for, and started again when it is back.
    #[test]
    fn a_file_that_does_not_go_through_is_sent_again_after_reset() {
        let mut sim = Sim::new(vec![
            Says::Size("boot.img"),
            Says::Read("boot.img"),
            // Its boot code now waits for the rest of the file, and says nothing.
            Says::Silence,
            Says::Reset,
            Says::Size("boot.img"),
            Says::Read("boot.img"),
            Says::Done,
        ]);
        sim.lost = 1;
        let (lines, sim) = boots(sim);
        assert_eq!(
            lines,
            [
                "Waiting for the FIBER on USB port 1-1",
                "Sending bootcode4.bin",
                "Second stage boot server",
                "File read: boot.img",
                "Sending boot.img to the FIBER did not go through: no answer in time",
                "The FIBER did not get all its files; waiting for it on USB port 1-1 again",
                "Second stage boot server",
                "The FIBER is not ready yet: it does not ask for its files; press RESET on it",
                "The FIBER did not get all its files; waiting for it on USB port 1-1 again",
                "Sending bootcode4.bin",
                "Second stage boot server",
                "File read: boot.img",
                "Second stage boot server done",
            ]
        );
        assert_eq!(sim.lock().unwrap().boot_codes.len(), 2);
        // The whole file arrived the second time.
        assert_eq!(answers(&sim).last().unwrap().2.as_ref().map(Vec::len), Some(100_000));
    }

    /// Without RESET such a board ends the wait with the advice to press it.
    #[test]
    fn says_to_press_reset_when_the_boot_code_stays_silent() {
        let mut sim = Sim::new(vec![Says::Silence; 200]);
        sim.stage = Stage::Code;
        sim.silence = Duration::from_millis(20);
        let dir = gadget();
        let (result, _, _) = start(sim, dir.path(), Duration::from_secs(1), &Cancel::new());
        assert_eq!(
            result.unwrap_err().to_string(),
            "the FIBER on USB port 1-1 could not be started within 1 s (it does not ask for its files; press RESET on it); press RESET and try again"
        );
    }

    /// A board that leaves before it read a file was reset, not started: no gadget comes of it.
    #[test]
    fn a_board_that_leaves_before_it_read_a_file_is_not_taken_for_started() {
        let dir = gadget();
        let (result, lines, _) = start(
            Sim::new(vec![Says::Size("boot.img"), Says::Leaves]),
            dir.path(),
            Duration::from_secs(1),
            &Cancel::new(),
        );
        assert!(result.is_err());
        assert!(!lines.contains(&"Second stage boot server done".to_string()), "{lines:#?}");
    }

    #[test]
    fn waits_out_a_silent_or_stalled_board() {
        let (_, sim) = boots(Sim::new(vec![
            Says::Silence,
            Says::Stall,
            Says::Size("config.txt"),
            Says::Silence,
            Says::Read("config.txt"),
            Says::Done,
        ]));
        assert_eq!(answers(&sim).len(), 2);
    }

    #[test]
    fn a_cm5_gets_its_own_files() {
        let mut sim = Sim::new(vec![Says::Size("bootmain"), Says::Read("bootmain"), Says::Size("mcb.bin"), Says::Done]);
        sim.product_id = 0x2712;
        let (lines, sim) = boots(sim);
        assert_eq!(sim.lock().unwrap().boot_codes, [vec![5u8; 700]]);
        let answers = answers(&sim);
        assert_eq!(answers[1], ("bootmain".to_string(), 100, Some(vec![0xB5; 100])));
        // The CM4's file, in the folder and in the archive, is not the CM5's.
        assert_eq!(answers[2], ("mcb.bin".to_string(), 0, None));
        assert!(lines.contains(&"Sending bootcode5.bin".to_string()), "{lines:?}");
        assert!(lines.contains(&"Cannot open file mcb.bin".to_string()), "{lines:?}");
    }

    #[test]
    fn a_board_without_boot_code_here_is_refused() {
        let mut sim = Sim::new(vec![]);
        sim.product_id = 0x2764;
        let dir = gadget();
        let (result, _, sim) = start(sim, dir.path(), Duration::from_secs(20), &Cancel::new());
        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("this board (0a5c:2764) needs bootcode.bin"), "{error}");
        assert!(sim.lock().unwrap().boot_codes.is_empty());
    }

    #[test]
    fn a_board_cannot_ask_its_way_out_of_the_folder() {
        let outer = tempfile::tempdir().unwrap();
        std::fs::write(outer.path().join("secret.txt"), "not for boards").unwrap();
        let dir = outer.path().join("mass-storage-gadget64");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("config.txt"), "inside").unwrap();
        std::fs::create_dir(dir.join("overlays")).unwrap();
        std::fs::write(dir.join("overlays/dwc2.dtbo"), "overlay").unwrap();
        let files = Files::open(&dir).unwrap();
        let get = |name: &str| files.get(Chip::Bcm2711, name).map(|data| String::from_utf8(data).unwrap());
        assert_eq!(get("config.txt").as_deref(), Some("inside"));
        assert_eq!(get("overlays/dwc2.dtbo").as_deref(), Some("overlay"));
        // As rpiboot would open them, under the folder.
        assert_eq!(get("/config.txt").as_deref(), Some("inside"));
        assert_eq!(get("./overlays//dwc2.dtbo").as_deref(), Some("overlay"));
        let secret = outer.path().join("secret.txt").display().to_string();
        for name in [
            "../secret.txt",
            "overlays/../../secret.txt",
            "..",
            "..\\secret.txt",
            "C:\\secret.txt",
            &secret,
            "",
            "/",
            ".",
        ] {
            assert_eq!(get(name), None, "{name}");
        }
    }

    #[test]
    fn reads_the_firmware_archive() {
        let archive = tar(&[
            ("2711/", b""),
            ("2711/a.bin", b"first"),
            ("2711/empty", b""),
            ("2711/B.BIN", &[7u8; 513]),
            ("x", b"last"),
        ]);
        assert_eq!(tar_entry(&archive, "2711/a.bin"), Some(&b"first"[..]));
        assert_eq!(tar_entry(&archive, "2711/empty"), Some(&b""[..]));
        // rpiboot compares the names without regard to case.
        assert_eq!(tar_entry(&archive, "2711/b.bin"), Some(&[7u8; 513][..]));
        assert_eq!(tar_entry(&archive, "x"), Some(&b"last"[..]));
        assert_eq!(tar_entry(&archive, "2711/c.bin"), None);
        assert_eq!(tar_entry(&archive, ""), Some(&b""[..]));
        assert_eq!(tar_entry(&[], "x"), None);
        // An archive that ends before its file does.
        assert_eq!(tar_entry(&archive[..512 + 3], "2711/a.bin"), None);
        assert_eq!(tar_entry(&archive[..2048], "x"), None);
    }

    /// The archive Raspberry Pi ships, where `npm run usbboot` has put it.
    #[test]
    fn finds_the_cm4s_boot_code_in_the_real_archive() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources/usbboot/mass-storage-gadget64");
        if !is_gadget(&dir) {
            eprintln!("not run: no gadget in {}", dir.display());
            return;
        }
        let files = Files::open(&dir).unwrap();
        let code = files.get(Chip::Bcm2711, "bootcode4.bin").unwrap();
        assert!(code.len() > 10_000, "{} bytes", code.len());
        assert!(files.get(Chip::Bcm2711, "bootmain").is_some());
        assert!(files.get(Chip::Bcm2711, "boot.img").is_some_and(|img| img.len() > 1 << 20));
        assert!(files.get(Chip::Bcm2711, "config.txt").is_some());
        assert!(files.get(Chip::Bcm2711, "boot.sig").is_none());
    }

    #[test]
    fn a_wrong_request_stops_it() {
        let dir = gadget();
        let (result, _, _) =
            start(Sim::new(vec![Says::Number(7)]), dir.path(), Duration::from_secs(20), &Cancel::new());
        assert_eq!(result.unwrap_err().to_string(), "the FIBER sent a request that is not known (7)");
        let (result, _, _) = start(Sim::new(vec![Says::Short]), dir.path(), Duration::from_secs(20), &Cancel::new());
        assert_eq!(result.unwrap_err().to_string(), "the FIBER sent an incomplete request (12 bytes)");
    }

    #[test]
    fn no_permission_counts_only_when_it_stays() {
        // For a moment after the board appeared: it is opened all the same.
        let mut sim = Sim::new(vec![Says::Done]);
        sim.denied = 3;
        let (lines, sim) = boots(sim);
        assert_eq!(lines[1], "The FIBER is not ready yet: no permission to use it");
        assert_eq!(sim.lock().unwrap().opens, 5);

        let mut sim = Sim::new(vec![]);
        sim.denied = usize::MAX;
        let dir = gadget();
        let (result, _, sim) = start(sim, dir.path(), Duration::from_secs(20), &Cancel::new());
        assert_eq!(result.unwrap_err().to_string(), "no permission to use the USB device; allow USB access first");
        assert_eq!(sim.lock().unwrap().opens, 5);
    }

    #[test]
    fn says_why_when_the_board_stays_unready() {
        let mut sim = Sim::new(vec![]);
        sim.busy = usize::MAX;
        let dir = gadget();
        let (result, lines, _) = start(sim, dir.path(), Duration::from_secs(1), &Cancel::new());
        assert_eq!(
            result.unwrap_err().to_string(),
            "the FIBER on USB port 1-1 could not be started within 1 s (opening it: busy); press RESET and try again"
        );
        assert_eq!(lines, ["Waiting for the FIBER on USB port 1-1", "The FIBER is not ready yet: opening it: busy"]);
    }

    #[test]
    fn gives_up_when_no_board_answers() {
        let mut sim = Sim::new(vec![]);
        sim.port = "9-9";
        let dir = gadget();
        let (result, lines, sim) = start(sim, dir.path(), Duration::from_secs(1), &Cancel::new());
        let error = result.unwrap_err().to_string();
        assert!(error.starts_with("the FIBER did not answer on USB port 1-1 within 1 s"), "{error}");
        assert_eq!(lines, ["Waiting for the FIBER on USB port 1-1"]);
        assert_eq!(sim.lock().unwrap().opens, 0);
    }

    #[test]
    fn cancel_does_not_wait_for_the_board() {
        // The board has nothing to say, and each question takes its time.
        let mut sim = Sim::new(vec![Says::Silence; 10]);
        sim.silence = Duration::from_secs(3);
        let dir = gadget();
        let cancel = Cancel::new();
        let stop = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            stop.cancel();
        });
        let started = Instant::now();
        let (result, _, _) = start(sim, dir.path(), Duration::from_secs(60), &cancel);
        assert_eq!(result.unwrap_err().to_string(), "cancelled");
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    }

    #[test]
    fn finds_the_gadget_in_a_folder_given() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_tools(&[dir.path().to_path_buf()]).is_none_or(|t| !t.gadget.starts_with(dir.path())));
        let gadget = dir.path().join("mass-storage-gadget64");
        std::fs::create_dir(&gadget).unwrap();
        std::fs::write(gadget.join("boot.img"), "x").unwrap();
        assert!(find_tools(&[dir.path().to_path_buf()]).is_none_or(|t| !t.gadget.starts_with(dir.path())));
        std::fs::write(gadget.join("bootfiles.bin"), "x").unwrap();
        // Unless this computer has one that comes before it (FIBER_SETUP_USBBOOT).
        if std::env::var_os("FIBER_SETUP_USBBOOT").is_none() {
            assert_eq!(find_tools(&[dir.path().to_path_buf()]).unwrap().gadget, gadget);
        }
    }

    #[test]
    fn the_boot_roms_are_the_ones_the_board_list_knows() {
        assert_eq!(format!("{VENDOR:04x}"), crate::usb::BROADCOM_VID);
        assert_eq!(PRODUCTS.map(|p| format!("{p:04x}")), crate::usb::BOOT_PIDS);
    }
}
