//! fiber-setup-cli: the same steps as the FIBER Setup app, in a terminal.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use fiber_setup_core::firstboot::{Settings, Wifi};
use fiber_setup_core::{Cancel, Event, access, catalog, check, disk, flash, image, mbr, rpiboot, usb};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "fiber-setup-cli", version, about = "Flash a HARDWARIO FIBER (CM4) over its USB-B port")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Download (or reuse) and check the image, unpack it into the cache.
    Image {
        /// Use this .img.xz or .img instead of the known Raspberry Pi OS Lite.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// FIBERs on USB (boot mode or gadget) and the disks on this computer.
    Devices,
    /// Whether the USB udev rules (on Windows: the USB driver) are installed; --install installs
    /// them (Linux asks for the password, Windows whether to trust the driver's publisher).
    Access {
        #[arg(long)]
        install: bool,
    },
    /// Start the FIBER (jumper on BOOT) over USB so that its eMMC shows as a disk on this
    /// computer, and stop there: nothing is written.
    Boot {
        /// The USB port of the FIBER in boot mode (see `devices`); found by itself when there is one.
        #[arg(long)]
        port: Option<String>,
    },
    /// Write the image with the first-boot settings to the FIBER (or, with --to-file, to a file).
    /// Passwords come from FIBER_SETUP_PASSWORD and FIBER_SETUP_WIFI_PASSWORD.
    Flash(Box<FlashArgs>),
    /// Show the first-boot files on an image file or a FIBER's eMMC (jumper on BOOT, once it shows as a disk).
    Inspect { path: PathBuf },
    /// Look for <hostname>.local on this network.
    Find {
        #[arg(default_value = "fiber")]
        hostname: String,
    },
}

#[derive(clap::Args)]
struct FlashArgs {
    #[arg(long)]
    file: Option<PathBuf>,
    /// Device name. With the FIBER software, a FIBER written over USB is named fiber-<serial> instead.
    #[arg(long, default_value = "fiber")]
    hostname: String,
    #[arg(long, default_value = "fiber")]
    user: String,
    /// A public key file to log in with (e.g. ~/.ssh/id_ed25519.pub).
    #[arg(long)]
    ssh_key: Option<PathBuf>,
    #[arg(long)]
    wifi_ssid: Option<String>,
    #[arg(long)]
    wifi_hidden: bool,
    /// Wi-Fi country, two letters.
    #[arg(long, default_value = "CZ")]
    country: String,
    #[arg(long)]
    timezone: Option<String>,
    /// Do not run the FIBER setup script on the first start (plain Raspberry Pi OS).
    #[arg(long)]
    no_fiber_software: bool,
    /// The USB port of the FIBER in boot mode (see `devices`); found by itself when there is one.
    #[arg(long)]
    port: Option<String>,
    /// Write into this file instead of a FIBER.
    #[arg(long)]
    to_file: Option<PathBuf>,
}

fn print_event(e: Event) {
    match e {
        Event::Phase { phase } => eprintln!("== {phase:?}"),
        Event::Progress { phase, done, total } => {
            let pct = (done * 100).checked_div(total).unwrap_or(100);
            eprint!("\r   {phase:?} {pct:3}% ({} / {} MB)   ", done >> 20, total >> 20);
            if done >= total {
                eprintln!();
            }
        }
        Event::Log { line } => eprintln!("   {line}"),
    }
}

fn prepared(file: Option<&Path>, cancel: &Cancel) -> Result<image::Prepared> {
    let cache = image::cache_dir();
    match file {
        Some(f) => image::prepare_local(&cache, f, &print_event, cancel),
        None => image::prepare_official(&cache, &catalog::RASPIOS_LITE, &print_event, cancel),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let cancel = Cancel::new();
    match cli.cmd {
        Cmd::Image { file } => {
            let p = prepared(file.as_deref(), &cancel)?;
            println!("{}", serde_json::to_string_pretty(&p)?);
        }
        Cmd::Devices => {
            for b in usb::boards() {
                println!("usb  {:<10} {:?} 0a5c:{} {}", b.port, b.kind, b.product_id, b.serial.unwrap_or_default());
            }
            for d in disk::list()? {
                println!(
                    "disk {:<10} {:>7.1} GB  {} {}  port={} {}",
                    d.path,
                    d.size as f64 / 1e9,
                    d.vendor.as_deref().unwrap_or("-"),
                    d.model.as_deref().unwrap_or("-"),
                    d.usb_port.as_deref().unwrap_or("-"),
                    if d.looks_like_cm4_emmc() { "<- FIBER eMMC" } else { "" }
                );
            }
        }
        Cmd::Access { install } => {
            if install {
                access::install(rpiboot::find_tools(&[]).as_ref())?;
            }
            println!("{}", serde_json::to_string_pretty(&access::status())?);
        }
        Cmd::Boot { port } => {
            let emmc = flash::start_gadget(&tools()?, &boot_port(port)?, 1, &print_event, &cancel)?;
            println!("{}", serde_json::to_string_pretty(&emmc)?);
        }
        Cmd::Flash(a) => flash_cmd(*a, &cancel)?,
        Cmd::Inspect { path } => inspect(&path)?,
        Cmd::Find { hostname } => match check::find(&hostname) {
            Some(f) => println!("{hostname}.local = {} (SSH {})", f.ip, if f.ssh { "open" } else { "closed" }),
            None => bail!("{hostname}.local not found on this network"),
        },
    }
    Ok(())
}

/// The gadget's files, which a FIBER is started with over USB.
fn tools() -> Result<rpiboot::Tools> {
    rpiboot::find_tools(&[]).context(
        "the gadget's files (mass-storage-gadget64) are not found; set FIBER_SETUP_USBBOOT to the folder they are in, or to a usbboot checkout",
    )
}

/// The USB port to work on: the one given, or that of the only board in boot mode.
fn boot_port(port: Option<String>) -> Result<String> {
    if let Some(port) = port {
        return Ok(port);
    }
    let boot: Vec<_> = usb::boards().into_iter().filter(|b| b.kind == usb::BoardKind::Boot).collect();
    match boot.as_slice() {
        [one] => Ok(one.port.clone()),
        [] => bail!("no FIBER in boot mode on USB (jumper on BOOT, power, USB-B cable, RESET)"),
        _ => bail!("several boards in boot mode; choose one with --port (see `devices`)"),
    }
}

fn flash_cmd(a: FlashArgs, cancel: &Cancel) -> Result<()> {
    let password = std::env::var("FIBER_SETUP_PASSWORD").context("set FIBER_SETUP_PASSWORD to the login password")?;
    let ssh_key = match &a.ssh_key {
        Some(p) => {
            Some(std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?.trim().to_string())
        }
        None => None,
    };
    let wifi = a.wifi_ssid.map(|ssid| Wifi {
        ssid,
        password: std::env::var("FIBER_SETUP_WIFI_PASSWORD").unwrap_or_default(),
        hidden: a.wifi_hidden,
    });
    let settings = Settings {
        hostname: a.hostname,
        username: a.user,
        password,
        ssh_key,
        wifi,
        country: a.country.to_uppercase(),
        timezone: a.timezone,
        install_fiber: !a.no_fiber_software,
    };
    settings.validate()?;
    let image = prepared(a.file.as_deref(), cancel)?;
    let report = if let Some(out) = a.to_file {
        flash::run_to_file(&image, &settings, &out, &print_event, cancel)?
    } else {
        flash::run(&tools()?, &image, &settings, &boot_port(a.port)?, &print_event, cancel)?
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// Prints the first-boot files from the boot partition of an image file or an eMMC.
fn inspect(path: &Path) -> Result<()> {
    let part = mbr::boot_partition(path)?;
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(part.start))?;
    let mut bytes = vec![0u8; part.size as usize];
    f.read_exact(&mut bytes)?;
    let mut cursor = std::io::Cursor::new(bytes);
    let fs = fatfs::FileSystem::new(&mut cursor, fatfs::FsOptions::new()).context("opening the boot partition")?;
    let root = fs.root_dir();
    for name in ["cmdline.txt", "meta-data", "network-config", "user-data", "fiber-setup.log"] {
        let mut text = String::new();
        match root.open_file(name).and_then(|mut file| file.read_to_string(&mut text)) {
            Ok(_) => {
                let shown = if name == "network-config" { mask_passwords(&text) } else { text };
                println!("----- {name}\n{}", shown.trim_end());
            }
            Err(_) => println!("----- {name}: (none)"),
        }
    }
    let script = root.open_file(fiber_setup_core::firstboot::INSTALL_SCRIPT_NAME).is_ok();
    println!(
        "----- {}: {}",
        fiber_setup_core::firstboot::INSTALL_SCRIPT_NAME,
        if script { "present" } else { "(none)" }
    );
    Ok(())
}

/// The Wi-Fi password stays off the screen.
fn mask_passwords(text: &str) -> String {
    text.lines()
        .map(|l| match l.find("password:") {
            Some(i) => format!("{}password: \"********\"", &l[..i]),
            None => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
