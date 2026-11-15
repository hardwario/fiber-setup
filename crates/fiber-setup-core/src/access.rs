//! USB and eMMC access without root: two udev rules under the Provisioning Station's file names.
//! They are its rules with the partition lines fixed (see the eMMC rule). The Station only checks
//! that the files exist, so it keeps ours; this app replaces the Station's text with ours.
//!
//! macOS has no such step: any user may talk to the boot ROM over USB, and for the eMMC macOS
//! asks for the password each time it is written (`macos.rs`). There the rules count as
//! installed and installing them does nothing.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::path::Path;
use std::process::Command;

pub const RULES_DIR: &str = "/etc/udev/rules.d";
pub const RPIBOOT_RULE: &str = "55-fiber-rpiboot.rules";
pub const EMMC_RULE: &str = "61-fiber-cm4-emmc.rules";
pub const RPIBOOT_RULE_TEXT: &str = include_str!("../udev/55-fiber-rpiboot.rules");
pub const EMMC_RULE_TEXT: &str = include_str!("../udev/61-fiber-cm4-emmc.rules");

/// Root's side: write both rules and apply them to devices already plugged in, so they work at
/// once, without a new login. `$1` the directory, `$2` / `$3` the rules.
const INSTALL_SCRIPT: &str = r#"set -eu
dir=$1
mkdir -p "$dir"
printf '%s' "$2" > "$dir/55-fiber-rpiboot.rules"
printf '%s' "$3" > "$dir/61-fiber-cm4-emmc.rules"
chmod 0644 "$dir/55-fiber-rpiboot.rules" "$dir/61-fiber-cm4-emmc.rules"
if [ "$dir" = /etc/udev/rules.d ] && command -v udevadm >/dev/null 2>&1; then
  udevadm control --reload-rules
  udevadm trigger --subsystem-match=usb --attr-match=idVendor=0a5c || true
  udevadm trigger --subsystem-match=block --property-match=ID_BUS=usb --action=change || true
fi
"#;

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// Both rules are installed with our text. Always true on macOS, which needs none.
    pub rules_installed: bool,
    /// The polkit helper that asks for the password is there.
    pub pkexec: bool,
}

pub fn status() -> Status {
    if cfg!(target_os = "macos") {
        return Status { rules_installed: true, pkexec: false };
    }
    let dir = Path::new(RULES_DIR);
    let same = |name: &str, text: &str| std::fs::read_to_string(dir.join(name)).map(|t| t == text).unwrap_or(false);
    Status {
        rules_installed: same(RPIBOOT_RULE, RPIBOOT_RULE_TEXT) && same(EMMC_RULE, EMMC_RULE_TEXT),
        pkexec: which("pkexec"),
    }
}

fn which(name: &str) -> bool {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file())).unwrap_or(false)
}

/// Installs the rules through pkexec: the desktop asks for the password.
pub fn install() -> Result<()> {
    if cfg!(target_os = "macos") {
        return Ok(());
    }
    if !which("pkexec") {
        bail!(
            "pkexec is missing; install the rules by hand: sudo cp {RPIBOOT_RULE} {EMMC_RULE} {RULES_DIR}/ && sudo udevadm control --reload-rules"
        );
    }
    let out = Command::new("pkexec")
        .args(["sh", "-c", INSTALL_SCRIPT, "sh", RULES_DIR, RPIBOOT_RULE_TEXT, EMMC_RULE_TEXT])
        .output()
        .context("starting pkexec")?;
    if !out.status.success() {
        // 126: the dialog was dismissed, 127: not authorised.
        if matches!(out.status.code(), Some(126) | Some(127)) {
            bail!("not allowed (the password dialog was closed or refused)");
        }
        bail!("installing the rules failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}
