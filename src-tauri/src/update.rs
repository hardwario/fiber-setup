//! Update notices in the window. A moment after the start, and once a day for as long as the
//! app stays open, it asks GitHub for the newest published release
//! (`fiber_setup_core::update`). A newer one is offered on the Welcome page and under the step
//! list; nothing is downloaded or installed. No tray item and no notification either: an app
//! with one window, open for one job, has no use for them.

use fiber_setup_core::update::{self, CheckError, Release};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter};

/// After the start: the window comes first.
const FIRST_CHECK: Duration = Duration::from_secs(1);
const INTERVAL: Duration = Duration::from_secs(24 * 3600);
/// After a check without an answer (offline, or GitHub's hourly limit for this address).
const RETRY: Duration = Duration::from_secs(3600);
/// The schedule follows the wall clock, read this often: the other clock stands still while a
/// computer sleeps, so a day of it can take weeks.
const POLL: Duration = Duration::from_secs(600);

/// The newer release, once a check found one.
#[derive(Clone, Default)]
pub struct Updates(Arc<Mutex<Option<Release>>>);

impl Updates {
    pub fn available(&self) -> Option<Release> {
        self.0.lock().ok().and_then(|found| found.clone())
    }

    fn set(&self, found: Option<Release>) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = found;
        }
    }
}

/// Whether to ask at all: not in the demo, which touches no network, and not when the user's
/// `FIBER_SETUP_UPDATE_CHECK` says `0` (or `off`, `no`, `false`).
fn enabled(demo: bool, setting: Option<&str>) -> bool {
    let off = setting.is_some_and(|v| ["0", "off", "no", "false"].contains(&v.trim().to_lowercase().as_str()));
    !demo && !off
}

/// When to ask next. Without an answer (offline, a limit reached) within the hour; an answer
/// that cannot be used will not change that soon, and is asked for again tomorrow, like a
/// good one.
fn pause_after(result: &Result<Option<Release>, CheckError>) -> Duration {
    if matches!(result, Err(CheckError::Network(_))) { RETRY } else { INTERVAL }
}

/// Starts the checks on a thread of their own. What they find comes as `update-available`
/// events and is kept in `updates`.
pub fn start(app: &AppHandle, updates: Updates, demo: bool) {
    if !enabled(demo, std::env::var("FIBER_SETUP_UPDATE_CHECK").ok().as_deref()) {
        return;
    }
    // A local file server in place of GitHub, for trying the notice.
    let url = std::env::var("FIBER_SETUP_UPDATE_URL").unwrap_or_else(|_| update::LATEST.into());
    let app = app.clone();
    let started = std::thread::Builder::new().name("updates".into()).spawn(move || run(&app, &updates, &url));
    if let Err(e) = started {
        eprintln!("cannot start the update check: {e}");
    }
}

fn run(app: &AppHandle, updates: &Updates, url: &str) {
    let mut due = SystemTime::now() + FIRST_CHECK;
    loop {
        let wait = due.duration_since(SystemTime::now()).unwrap_or_default();
        if !wait.is_zero() {
            std::thread::sleep(wait.min(POLL));
            continue;
        }
        let result = update::check(url);
        due = SystemTime::now() + pause_after(&result);
        match result {
            Ok(found) => {
                updates.set(found.clone());
                let _ = app.emit("update-available", found);
            }
            // A release found before stays known.
            Err(e) => eprintln!("update check failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_unless_told_not_to() {
        assert!(enabled(false, None));
        assert!(enabled(false, Some("1")));
        assert!(enabled(false, Some("")));
        for off in ["0", "off", "OFF", "no", "false", " 0 "] {
            assert!(!enabled(false, Some(off)), "{off:?}");
        }
        // The demo touches no network, whatever the setting.
        assert!(!enabled(true, None));
        assert!(!enabled(true, Some("1")));
    }

    #[test]
    fn asks_again_sooner_only_when_there_was_no_answer() {
        let release = Release { version: "1.2.0".into(), url: "https://github.com/hardwario/fiber-setup".into() };
        assert_eq!(pause_after(&Ok(Some(release))), INTERVAL);
        assert_eq!(pause_after(&Ok(None)), INTERVAL);
        assert_eq!(pause_after(&Err(CheckError::Answer("no version".into()))), INTERVAL);
        assert_eq!(pause_after(&Err(CheckError::Network("offline".into()))), RETRY);
        assert!(RETRY < INTERVAL && POLL < RETRY);
    }

    #[test]
    fn keeps_what_a_check_found() {
        let updates = Updates::default();
        assert_eq!(updates.available(), None);
        let release = Release { version: "1.2.0".into(), url: "https://github.com/hardwario/fiber-setup".into() };
        updates.set(Some(release.clone()));
        // The window asks through a copy of it.
        assert_eq!(updates.clone().available(), Some(release));
        updates.set(None);
        assert_eq!(updates.available(), None);
    }
}
