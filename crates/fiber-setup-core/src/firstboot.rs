//! The first-boot settings, written into the image's FAT boot partition before it reaches the
//! eMMC (the "image loopback": change the image, then flash it). Same files and format as
//! Raspberry Pi Imager 2.x on Raspberry Pi OS trixie: cloud-init NoCloud `user-data`,
//! `network-config` and `meta-data`, plus `ds=nocloud;i=<id>` and the Wi-Fi country on the
//! kernel command line.
//!
//! With "install the FIBER software" on, `fiber-install.sh` goes onto the boot partition too and
//! cloud-init runs it once, unattended: it waits for the network and the clock, then runs
//! `curl -fsSL <fiber-agent>/main/bootstrap.sh | sudo bash`. The FIBER restarts only when that
//! finished OK. The FIBER software names
//! the FIBER `fiber-<last 8 characters of the CM4 serial>`; written over USB, it has that name
//! from the first start ([`start_name`]).

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};

/// The first-start installer, built into the app.
pub const INSTALL_SCRIPT: &str = include_str!("../../../image/fiber-install.sh");
pub const INSTALL_SCRIPT_NAME: &str = "fiber-install.sh";
/// Files of earlier FIBER Setup versions that a re-personalised image must not keep.
const OLD_FILES: [&str; 1] = ["fiber-cm4-setup.sh"];
/// Where cloud-init leaves a copy of the script's log for reading over USB (jumper on BOOT).
pub const SETUP_LOG_NAME: &str = "fiber-setup.log";

/// Accounts Raspberry Pi OS already has; cloud-init cannot create the login user under one of them.
const SYSTEM_USERS: [&str; 22] = [
    "root",
    "daemon",
    "bin",
    "sys",
    "sync",
    "games",
    "man",
    "lp",
    "mail",
    "news",
    "uucp",
    "proxy",
    "www-data",
    "backup",
    "list",
    "irc",
    "nobody",
    "messagebus",
    "sshd",
    "avahi",
    "polkitd",
    "systemd-network",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wifi {
    pub ssid: String,
    /// Empty for an open network.
    pub password: String,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub hostname: String,
    pub username: String,
    /// Hashed here (SHA-512 crypt); the plain text is never written anywhere.
    pub password: String,
    #[serde(default)]
    pub ssh_key: Option<String>,
    #[serde(default)]
    pub wifi: Option<Wifi>,
    /// Two-letter country for the Wi-Fi regulatory domain, e.g. `CZ`.
    pub country: String,
    #[serde(default)]
    pub timezone: Option<String>,
    /// Run the FIBER setup script on the first start (needs the internet).
    pub install_fiber: bool,
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        let h = &self.hostname;
        let host_ok = !h.is_empty()
            && h.len() <= 63
            && h.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !h.starts_with('-')
            && !h.ends_with('-');
        if !host_ok {
            bail!("the device name may use only a-z, 0-9 and '-' (not at the start or end), up to 63 characters");
        }
        let u = &self.username;
        let user_ok = !u.is_empty()
            && u.len() <= 32
            && u.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
            && u.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
            && !SYSTEM_USERS.contains(&u.as_str());
        if !user_ok {
            bail!(
                "the user name may use only a-z, 0-9, '_' and '-', must start with a letter and cannot be a system account"
            );
        }
        // Its length is the user's business; without any, nobody could log in with it.
        if self.password.is_empty() {
            bail!("the password is missing");
        }
        if let Some(key) = &self.ssh_key {
            let key = key.trim();
            let known = ["ssh-", "ecdsa-", "sk-ssh-", "sk-ecdsa-"].iter().any(|p| key.starts_with(p));
            if !known || key.contains('\n') || key.split_whitespace().count() < 2 {
                bail!("the SSH key is not a public key line (it should start with ssh-ed25519 or ssh-rsa)");
            }
        }
        if self.country.len() != 2 || !self.country.bytes().all(|b| b.is_ascii_uppercase()) {
            bail!("the country must be two capital letters, e.g. CZ");
        }
        if let Some(w) = &self.wifi {
            if w.ssid.is_empty() || w.ssid.len() > 32 {
                bail!("the Wi-Fi name must have 1 to 32 characters");
            }
            let p = &w.password;
            let hex64 = p.len() == 64 && p.bytes().all(|b| b.is_ascii_hexdigit());
            if !p.is_empty() && !hex64 && !(8..=63).contains(&p.len()) {
                bail!("the Wi-Fi password must have 8 to 63 characters (or be empty for an open network)");
            }
        }
        if let Some(tz) = &self.timezone
            && (tz.is_empty() || !tz.bytes().all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b)))
        {
            bail!("the time zone is not valid");
        }
        Ok(())
    }
}

/// A YAML double-quoted string. JSON escaping is valid YAML, and it keeps quotes, backslashes and
/// non-ASCII characters in passwords and Wi-Fi names intact.
fn q(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

pub fn new_instance_id() -> String {
    let mut bytes = [0u8; 4];
    let _ = getrandom::fill(&mut bytes);
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("fiber-setup-{secs}-{}", hex::encode(bytes))
}

pub fn hash_password(password: &str) -> Result<String> {
    let params = sha_crypt::Sha512Params::new(sha_crypt::ROUNDS_DEFAULT)
        .map_err(|e| anyhow::anyhow!("password hash parameters: {e:?}"))?;
    sha_crypt::sha512_simple(password, &params).map_err(|e| anyhow::anyhow!("hashing the password: {e:?}"))
}

/// The name a FIBER gives itself once the FIBER software runs: `fiber-` and the last 8 characters
/// of its CM4 serial (fiber-agent's `set-hostname.sh`).
pub fn hub_name(cm4_serial: &str) -> Option<String> {
    let s = cm4_serial.trim().to_lowercase();
    (s.len() >= 8 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("fiber-{}", &s[s.len() - 8..]))
}

/// The name the FIBER starts with. With the FIBER software it is the one that software gives it
/// ([`hub_name`]), so the FIBER keeps one name (and SSH asks about its key once); without it, or
/// with no serial number, the one from the settings.
pub fn start_name(settings: &Settings, cm4_serial: Option<&str>) -> String {
    match cm4_serial.and_then(hub_name) {
        Some(name) if settings.install_fiber => name,
        _ => settings.hostname.clone(),
    }
}

pub struct Files {
    pub user_data: String,
    pub network_config: String,
    pub meta_data: String,
    /// What goes at the end of `cmdline.txt`.
    pub cmdline: String,
}

pub fn render(s: &Settings, instance_id: &str, password_hash: &str) -> Files {
    let mut u = String::new();
    u.push_str("#cloud-config\n");
    u.push_str(&format!("# Written by FIBER Setup {}.\n", crate::VERSION));
    u.push_str("manage_resolv_conf: false\n\n");
    // With the FIBER software, cloud-init must leave /etc/hosts alone: the software makes it a link
    // and names the FIBER after its serial, and cloud-init would rewrite it at every start.
    let manage_hosts = if s.install_fiber { "false" } else { "true" };
    u.push_str(&format!("hostname: {}\nmanage_etc_hosts: {manage_hosts}\n", q(&s.hostname)));
    if let Some(tz) = &s.timezone {
        u.push_str(&format!("timezone: {}\n", q(tz)));
    }
    u.push_str("user:\n");
    u.push_str(&format!("  name: {}\n", q(&s.username)));
    u.push_str("  shell: /bin/bash\n  lock_passwd: false\n");
    u.push_str(&format!("  passwd: {}\n", q(password_hash)));
    u.push_str("  sudo: null\n");
    if let Some(key) = &s.ssh_key {
        u.push_str(&format!("  ssh_authorized_keys:\n    - {}\n", q(key.trim())));
    }
    u.push_str("ssh_pwauth: true\n");
    // SSH first: it is up during the long setup run and after a failure.
    u.push_str("runcmd:\n  - [ systemctl, enable, --now, ssh ]\n");
    if s.install_fiber {
        // The script restarts the FIBER itself when it succeeded (Raspberry Pi OS's cloud-init has no
        // power_state module), retries at later starts when it failed, and copies its log to the
        // boot partition either way.
        u.push_str(&format!("  - [ bash, /boot/firmware/{INSTALL_SCRIPT_NAME} ]\n"));
    }

    let mut n = String::new();
    n.push_str(
        "network:\n  version: 2\n  ethernets:\n    eth0:\n      dhcp4: true\n      dhcp6: true\n      optional: true\n",
    );
    if let Some(w) = &s.wifi {
        n.push_str("  wifis:\n    wlan0:\n      dhcp4: true\n");
        n.push_str(&format!("      regulatory-domain: {}\n", q(&s.country)));
        n.push_str("      access-points:\n");
        n.push_str(&format!("        {}:\n", q(&w.ssid)));
        if !w.password.is_empty() {
            n.push_str(&format!("          password: {}\n", q(&w.password)));
        }
        if w.hidden {
            n.push_str("          hidden: true\n");
        }
        if w.password.is_empty() && !w.hidden {
            // An access point needs at least one key; an open network has none.
            n.push_str("          {}\n");
        }
        n.push_str("      optional: true\n");
    }

    Files {
        user_data: u,
        network_config: n,
        meta_data: format!("instance-id: {instance_id}\n"),
        cmdline: format!("ds=nocloud;i={instance_id} cfg80211.ieee80211_regdom={}", s.country),
    }
}

/// `cmdline.txt` with our tokens at the end; an earlier `ds=` or regulatory domain is replaced,
/// so personalising an already personalised image does not stack them.
pub fn patch_cmdline(old: &str, tail: &str) -> String {
    let kept: Vec<&str> = old
        .split_whitespace()
        .filter(|t| !t.starts_with("ds=") && !t.starts_with("cfg80211.ieee80211_regdom="))
        .collect();
    format!("{} {}\n", kept.join(" "), tail)
}

fn write_file<T: fatfs::ReadWriteSeek>(root: &fatfs::Dir<'_, T>, name: &str, data: &[u8]) -> Result<()> {
    let mut f = root.create_file(name).with_context(|| format!("creating {name} on the boot partition"))?;
    f.truncate()?;
    f.write_all(data).with_context(|| format!("writing {name} on the boot partition"))?;
    f.flush()?;
    Ok(())
}

/// Writes the settings into a copy of the boot partition and returns it with the instance id.
pub fn personalise(boot: Vec<u8>, s: &Settings) -> Result<(Vec<u8>, String)> {
    s.validate()?;
    let instance_id = new_instance_id();
    let files = render(s, &instance_id, &hash_password(&s.password)?);
    let mut cursor = Cursor::new(boot);
    {
        let fs = fatfs::FileSystem::new(&mut cursor, fatfs::FsOptions::new())
            .context("opening the image's boot partition (FAT)")?;
        {
            let root = fs.root_dir();
            let mut old = String::new();
            root.open_file("cmdline.txt")
                .and_then(|mut f| f.read_to_string(&mut old))
                .context("reading cmdline.txt from the boot partition")?;
            write_file(&root, "cmdline.txt", patch_cmdline(&old, &files.cmdline).as_bytes())?;
            write_file(&root, "user-data", files.user_data.as_bytes())?;
            write_file(&root, "network-config", files.network_config.as_bytes())?;
            write_file(&root, "meta-data", files.meta_data.as_bytes())?;
            if s.install_fiber {
                write_file(&root, INSTALL_SCRIPT_NAME, INSTALL_SCRIPT.as_bytes())?;
            } else {
                let _ = root.remove(INSTALL_SCRIPT_NAME);
            }
            for name in OLD_FILES.iter().chain([&SETUP_LOG_NAME]) {
                let _ = root.remove(name);
            }
        }
        fs.unmount().context("closing the boot partition")?;
    }
    Ok((cursor.into_inner(), instance_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample() -> Settings {
        Settings {
            hostname: "fiber".into(),
            username: "fiber".into(),
            password: "correct horse".into(),
            ssh_key: Some("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample marek@laptop".into()),
            wifi: Some(Wifi { ssid: "Office \"5G\"".into(), password: "pa:ss;word\\x".into(), hidden: false }),
            country: "CZ".into(),
            timezone: Some("Europe/Prague".into()),
            install_fiber: true,
        }
    }

    #[test]
    fn validates() {
        assert!(sample().validate().is_ok());
        let mut s = sample();
        s.hostname = "Fiber".into();
        assert!(s.validate().is_err());
        let mut s = sample();
        s.password = "short".into();
        assert!(s.validate().is_ok());
        s.password = String::new();
        assert!(s.validate().is_err());
        let mut s = sample();
        s.country = "cz".into();
        assert!(s.validate().is_err());
        let mut s = sample();
        s.wifi = Some(Wifi { ssid: "x".into(), password: "1234567".into(), hidden: false });
        assert!(s.validate().is_err());
        let mut s = sample();
        s.ssh_key = Some("not a key".into());
        assert!(s.validate().is_err());
        for name in ["root", "www-data", "_x", "1abc"] {
            let mut s = sample();
            s.username = name.into();
            assert!(s.validate().is_err(), "{name}");
        }
    }

    #[test]
    fn renders_quoted_yaml() {
        let f = render(&sample(), "fiber-setup-1-abcd", "$6$salt$hash");
        assert!(f.user_data.starts_with("#cloud-config\n"));
        assert!(f.user_data.contains("passwd: \"$6$salt$hash\""));
        assert!(f.user_data.contains("[ bash, /boot/firmware/fiber-install.sh ]"));
        assert!(f.user_data.contains("manage_etc_hosts: false"));
        assert!(!f.user_data.contains("power_state"));
        assert!(f.network_config.contains("\"Office \\\"5G\\\"\":"));
        assert!(f.network_config.contains("password: \"pa:ss;word\\\\x\""));
        assert!(f.network_config.contains("regulatory-domain: \"CZ\""));
        assert_eq!(f.meta_data, "instance-id: fiber-setup-1-abcd\n");
        assert_eq!(f.cmdline, "ds=nocloud;i=fiber-setup-1-abcd cfg80211.ieee80211_regdom=CZ");
    }

    #[test]
    fn no_fiber_install_means_no_script_and_no_restart() {
        let mut s = sample();
        s.install_fiber = false;
        s.wifi = None;
        let f = render(&s, "id", "h");
        assert!(!f.user_data.contains(INSTALL_SCRIPT_NAME));
        assert!(f.user_data.contains("manage_etc_hosts: true"));
        assert!(!f.network_config.contains("wifis"));
    }

    #[test]
    fn cmdline_replaces_old_tokens() {
        let old = "console=tty1 root=PARTUUID=abc rootwait ds=nocloud;i=old cfg80211.ieee80211_regdom=GB\n";
        assert_eq!(
            patch_cmdline(old, "ds=nocloud;i=new cfg80211.ieee80211_regdom=CZ"),
            "console=tty1 root=PARTUUID=abc rootwait ds=nocloud;i=new cfg80211.ieee80211_regdom=CZ\n"
        );
    }

    #[test]
    fn install_script_runs_the_public_installer() {
        assert!(INSTALL_SCRIPT.contains(&format!("INSTALLER={}\n", crate::catalog::FIBER_INSTALLER)));
        assert!(INSTALL_SCRIPT.contains(r#""$INSTALLER" | sudo FIBER_NO_REBOOT=1 bash"#));
    }

    #[test]
    fn hub_name_from_serial() {
        assert_eq!(hub_name("10000000a1b2c3d4").as_deref(), Some("fiber-a1b2c3d4"));
        assert_eq!(hub_name("A1B2C3D4").as_deref(), Some("fiber-a1b2c3d4"));
        assert_eq!(hub_name("xyz"), None);
    }

    #[test]
    fn starts_under_its_final_name_with_the_fiber_software() {
        let mut s = sample();
        assert_eq!(start_name(&s, Some("10000000c787cd13")), "fiber-c787cd13");
        assert_eq!(start_name(&s, None), s.hostname);
        s.install_fiber = false;
        assert_eq!(start_name(&s, Some("10000000c787cd13")), s.hostname);
    }

    #[test]
    fn password_hash_is_sha512_crypt() {
        let h = hash_password("correct horse").unwrap();
        assert!(h.starts_with("$6$"), "{h}");
    }
}
