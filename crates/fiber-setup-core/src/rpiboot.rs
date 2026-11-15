//! Raspberry Pi's rpiboot (github.com/raspberrypi/usbboot): boots the FIBER's CM4 over USB into
//! the mass-storage gadget, which shows its eMMC as a disk on this computer.

use crate::event::Cancel;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct Tools {
    pub rpiboot: PathBuf,
    /// The `mass-storage-gadget64` directory (`boot.img`, `bootfiles.bin`, `config.txt`).
    pub gadget: PathBuf,
}

/// rpiboot's file name: `rpiboot.exe` on Windows (built with Cygwin, its DLLs next to it).
const RPIBOOT: &str = if cfg!(windows) { "rpiboot.exe" } else { "rpiboot" };

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// The gadget needs these; a directory without them is someone else's.
fn is_gadget(dir: &Path) -> bool {
    dir.join("boot.img").is_file() && (dir.join("bootfiles.bin").is_file() || dir.join("bootcode4.bin").is_file())
}

/// Finds rpiboot and the gadget: `FIBER_SETUP_USBBOOT` (a usbboot checkout or bundle), the
/// directories the app passes (its bundled resources), a usbboot checkout in
/// `~/github/raspberrypi/usbboot`, then a system install.
pub fn find_tools(extra: &[PathBuf]) -> Option<Tools> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("FIBER_SETUP_USBBOOT") {
        dirs.push(PathBuf::from(d));
    }
    dirs.extend(extra.iter().cloned());
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("github/raspberrypi/usbboot"));
    }
    for dir in &dirs {
        let gadget = dir.join("mass-storage-gadget64");
        let rpiboot = [dir.join(RPIBOOT), dir.join("bin").join(RPIBOOT)].into_iter().find(|p| is_executable(p));
        if let Some(rpiboot) = rpiboot
            && is_gadget(&gadget)
        {
            return Some(Tools { rpiboot, gadget });
        }
    }
    // A system rpiboot (Debian/Ubuntu package `rpiboot`) with its gadget files.
    if cfg!(windows) {
        return None;
    }
    let system = ["/usr/bin/rpiboot", "/usr/local/bin/rpiboot"].iter().map(PathBuf::from).find(|p| is_executable(p))?;
    ["/usr/share/rpiboot/mass-storage-gadget64", "/usr/local/share/rpiboot/mass-storage-gadget64"]
        .iter()
        .map(PathBuf::from)
        .find(|d| is_gadget(d))
        .map(|gadget| Tools { rpiboot: system, gadget })
}

/// Runs rpiboot for the board on `port` and returns when it has handed the board over to the
/// gadget (rpiboot exits by itself then). Output lines go to `on_line`.
///
/// On Windows rpiboot is not told the port: it names ports its own way there, which the app
/// does not know, so it takes the board it finds (the caller sees to it that there is one).
pub fn run(
    tools: &Tools,
    port: &str,
    timeout: Duration,
    on_line: &(dyn Fn(String) + Send + Sync),
    cancel: &Cancel,
) -> Result<()> {
    let started = std::time::Instant::now();
    #[cfg(not(windows))]
    let mut command = Command::new(&tools.rpiboot);
    #[cfg(windows)]
    let mut command = crate::windows::hidden(&tools.rpiboot);
    command.arg("-d").arg(&tools.gadget);
    if !cfg!(windows) {
        command.arg("-p").arg(port);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting {}", tools.rpiboot.display()))?;

    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        }));
    }
    if let Some(err) = child.stderr.take() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        }));
    }
    drop(tx);

    let mut permission_problem = false;
    let status = loop {
        while let Ok(line) = rx.try_recv() {
            let l = line.to_lowercase();
            if l.contains("permission") || l.contains("access denied") || l.contains("libusb_error_access") {
                permission_problem = true;
            }
            on_line(line);
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            bail!("cancelled");
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "the FIBER did not answer on USB port {port} within {} s; check that the jumper is on BOOT, press RESET and try again",
                timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    for r in readers {
        let _ = r.join();
    }
    for line in rx.try_iter() {
        on_line(line);
    }
    if !status.success() {
        if permission_problem {
            bail!("rpiboot has no permission to use the USB device; allow USB access first");
        }
        bail!("rpiboot stopped with {status}");
    }
    Ok(())
}
