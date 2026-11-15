//! The FIBER Setup window: Tauri commands over `fiber-setup-core`. Long jobs (image, flash) run
//! on their own thread and report through webview events; the window follows them.

use fiber_setup_core::firstboot::Settings;
use fiber_setup_core::image::Prepared;
use fiber_setup_core::{Cancel, Event, access, catalog, check, flash, image, rpiboot, usb};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct AppState {
    cancel: Cancel,
    prepared: Mutex<Option<Prepared>>,
    busy: AtomicBool,
}

impl AppState {
    /// One long job at a time: preparing the image and writing share the cache and the cancel.
    fn start(&self) -> Result<(), String> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err("another job is still running".into());
        }
        self.cancel.reset();
        Ok(())
    }

    fn prepared(&self) -> Option<Prepared> {
        self.prepared.lock().ok().and_then(|p| p.clone())
    }
}

#[derive(Serialize)]
struct Defaults {
    hostname: String,
    username: String,
    country: String,
    timezone: Option<String>,
    ssh_key: Option<String>,
    ssh_key_path: Option<String>,
    language: String,
}

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    os: &'static str,
    image: catalog::OsImage,
    tools_found: bool,
    defaults: Defaults,
}

#[derive(Serialize, Clone)]
struct Done<T: Serialize> {
    ok: bool,
    result: Option<T>,
    error: Option<String>,
}

fn done<T: Serialize>(r: anyhow::Result<T>) -> Done<T> {
    match r {
        Ok(v) => Done { ok: true, result: Some(v), error: None },
        Err(e) => Done { ok: false, result: None, error: Some(format!("{e:#}")) },
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The language and region this user chose in System Settings: an app started from the Finder
/// or the Dock has no `LANG`, and the one Terminal sets leaves out a region chosen apart from
/// the language.
#[cfg(target_os = "macos")]
fn system_locale() -> Option<String> {
    let out = std::process::Command::new("/usr/bin/defaults").args(["read", "-g", "AppleLocale"]).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    out.status.success().then(|| apple_locale(text.trim())).flatten()
}

/// `cs_CZ` as it is; `en_US@rg=czzzzz` (English, region Czechia) -> `en_CZ`.
#[cfg(any(target_os = "macos", test))]
fn apple_locale(text: &str) -> Option<String> {
    let (base, keywords) = text.split_once('@').unwrap_or((text, ""));
    let lang = base.split('_').next().filter(|l| !l.is_empty())?;
    let region = keywords
        .split(';')
        .find_map(|k| k.strip_prefix("rg="))
        .and_then(|r| r.get(..2))
        .or_else(|| base.split('_').find(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_uppercase())));
    Some(match region {
        Some(r) => format!("{lang}_{}", r.to_uppercase()),
        None => lang.to_string(),
    })
}

#[cfg(not(target_os = "macos"))]
fn system_locale() -> Option<String> {
    None
}

/// `cs_CZ.UTF-8` -> (`cs`, `CZ`).
fn locale() -> (String, Option<String>) {
    let env = || ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
    let raw = system_locale().or_else(env).unwrap_or_default();
    let base = raw.split('.').next().unwrap_or("");
    let mut parts = base.split('_');
    let lang = parts.next().unwrap_or("en").to_lowercase();
    let region =
        parts.next().map(|r| r.to_uppercase()).filter(|r| r.len() == 2 && r.bytes().all(|b| b.is_ascii_uppercase()));
    (if lang == "cs" { "cs".into() } else { "en".into() }, region)
}

/// The computer's time zone: `/etc/timezone`, or the target of `/etc/localtime` (macOS too).
fn timezone() -> Option<String> {
    if let Ok(tz) = std::fs::read_to_string("/etc/timezone") {
        let tz = tz.trim();
        if !tz.is_empty() {
            return Some(tz.to_string());
        }
    }
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let s = target.to_string_lossy();
    s.split_once("zoneinfo/").map(|(_, tz)| tz.to_string())
}

fn ssh_key() -> Option<(String, String)> {
    let dir = home()?.join(".ssh");
    ["id_ed25519.pub", "id_ecdsa.pub", "id_rsa.pub"].iter().find_map(|name| {
        let path = dir.join(name);
        let key = std::fs::read_to_string(&path).ok()?.trim().to_string();
        (!key.is_empty()).then(|| (key, path.display().to_string()))
    })
}

fn resource_dirs(app: &AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(r) = app.path().resource_dir() {
        dirs.push(r.join("usbboot"));
    }
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        dirs.push(exe_dir.join("usbboot"));
    }
    dirs
}

fn tools(app: &AppHandle) -> Option<rpiboot::Tools> {
    rpiboot::find_tools(&resource_dirs(app))
}

#[tauri::command]
fn app_info(app: AppHandle) -> AppInfo {
    let (language, region) = locale();
    let (ssh_key, ssh_key_path) = match ssh_key() {
        Some((k, p)) => (Some(k), Some(p)),
        None => (None, None),
    };
    AppInfo {
        version: fiber_setup_core::VERSION,
        os: std::env::consts::OS,
        image: catalog::RASPIOS_LITE,
        tools_found: tools(&app).is_some(),
        defaults: Defaults {
            hostname: "fiber".into(),
            username: "fiber".into(),
            country: region.unwrap_or_else(|| "CZ".into()),
            timezone: timezone(),
            ssh_key,
            ssh_key_path,
            language,
        },
    }
}

#[tauri::command]
fn image_status(state: State<'_, AppState>) -> serde_json::Value {
    let s = image::status(&image::cache_dir(), &catalog::RASPIOS_LITE);
    serde_json::json!({ "status": s, "prepared": state.prepared() })
}

/// Gets the image ready on a thread: `image-event` while it works, `image-done` at the end.
#[tauri::command]
fn image_prepare(app: AppHandle, state: State<'_, AppState>, file: Option<String>) -> Result<(), String> {
    state.start()?;
    // The old image must not stay usable while (or after) another one is being prepared.
    if let Ok(mut slot) = state.prepared.lock() {
        *slot = None;
    }
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let emit = |e: Event| {
            let _ = app.emit("image-event", e);
        };
        let cache = image::cache_dir();
        let r = match &file {
            Some(f) => image::prepare_local(&cache, std::path::Path::new(f), &emit, &state.cancel),
            None => image::prepare_official(&cache, &catalog::RASPIOS_LITE, &emit, &state.cancel),
        };
        if let (Ok(p), Ok(mut slot)) = (&r, state.prepared.lock()) {
            *slot = Some(p.clone());
        }
        state.busy.store(false, Ordering::SeqCst);
        let _ = app.emit("image-done", done(r));
    });
    Ok(())
}

#[tauri::command]
async fn pick_image(app: AppHandle) -> Option<String> {
    let picked = app.dialog().file().add_filter("Disk image", &["xz", "img"]).blocking_pick_file();
    picked.and_then(|f| f.into_path().ok()).map(|p| p.display().to_string())
}

#[tauri::command]
fn access_status() -> access::Status {
    access::status()
}

#[tauri::command]
async fn access_install() -> Result<access::Status, String> {
    tauri::async_runtime::spawn_blocking(|| access::install().map(|_| access::status()).map_err(|e| format!("{e:#}")))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn usb_boards() -> Vec<usb::UsbBoard> {
    usb::boards()
}

/// Writes the FIBER on `port` on a thread: `flash-event` while it works, `flash-done` at the end.
#[tauri::command]
fn flash_start(app: AppHandle, state: State<'_, AppState>, settings: Settings, port: String) -> Result<(), String> {
    settings.validate().map_err(|e| format!("{e:#}"))?;
    let image = state.prepared().ok_or("the image is not ready yet")?;
    let tools = tools(&app).ok_or("rpiboot is missing from this installation")?;
    state.start()?;
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let emit = |e: Event| {
            let _ = app.emit("flash-event", e);
        };
        let r = flash::run(&tools, &image, &settings, &port, &emit, &state.cancel);
        state.busy.store(false, Ordering::SeqCst);
        let _ = app.emit("flash-done", done(r));
    });
    Ok(())
}

#[tauri::command]
fn cancel(state: State<'_, AppState>) {
    state.cancel.cancel();
}

#[tauri::command]
async fn find_device(hostname: String) -> Option<check::Found> {
    tauri::async_runtime::spawn_blocking(move || check::find(&hostname)).await.ok().flatten()
}

/// Opens a web page in the browser; only https links, only ours.
#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    let allowed = ["https://docs.hardwario.com/", "https://www.hardwario.store/", "https://www.hardwario.com/"];
    if !allowed.iter().any(|p| url.starts_with(p)) {
        return Err("link not allowed".into());
    }
    let opener = if cfg!(target_os = "macos") { "/usr/bin/open" } else { "xdg-open" };
    let mut child = std::process::Command::new(opener).arg(&url).spawn().map_err(|e| e.to_string())?;
    // Reaped on a thread, so no zombie is left behind.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// `fiber-setup --demo` (or FIBER_SETUP_DEMO=1): the window runs on the simulated FIBER, to show
/// all the steps without hardware. Nothing is downloaded, written or looked for.
fn demo() -> bool {
    std::env::args().skip(1).any(|a| a == "--demo") || std::env::var_os("FIBER_SETUP_DEMO").is_some()
}

#[tauri::command]
fn demo_mode() -> bool {
    demo()
}

/// Everything in the window a quarter larger than tower-bridge's sizes (Marek, 2026-11-04:
/// "everything needs to be bigger").
const ZOOM: f64 = 1.25;

/// The window takes a good part of the screen rather than the fixed size in tauri.conf.json, which
/// left it small on a large screen.
fn size_window(app: &tauri::App) {
    let Some(w) = app.get_webview_window("main") else { return };
    if demo() {
        let _ = w.set_title("FIBER Setup (demo)");
    }
    if let Ok(Some(m)) = w.current_monitor() {
        let screen = m.size().to_logical::<f64>(m.scale_factor());
        let width = (screen.width * 0.8).clamp(1000.0, 1600.0).min(screen.width);
        let height = (screen.height * 0.85).clamp(700.0, 1060.0).min(screen.height);
        let _ = w.set_size(tauri::LogicalSize::new(width, height));
        let _ = w.center();
    }
    let _ = w.set_zoom(ZOOM);
}

pub fn run() {
    let result = tauri::Builder::default()
        .setup(|app| {
            size_window(app);
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            image_status,
            image_prepare,
            pick_image,
            access_status,
            access_install,
            usb_boards,
            flash_start,
            cancel,
            find_device,
            open_link,
            demo_mode
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("error: FIBER Setup could not start: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_locales() {
        assert_eq!(apple_locale("cs_CZ").as_deref(), Some("cs_CZ"));
        assert_eq!(apple_locale("en_US@rg=czzzzz").as_deref(), Some("en_CZ"));
        assert_eq!(apple_locale("en_CZ@currency=EUR").as_deref(), Some("en_CZ"));
        assert_eq!(apple_locale("zh-Hans_CN").as_deref(), Some("zh-Hans_CN"));
        assert_eq!(apple_locale("sr_Latn_RS@calendar=gregorian;rg=atzzzz").as_deref(), Some("sr_AT"));
        assert_eq!(apple_locale("en").as_deref(), Some("en"));
        assert_eq!(apple_locale(""), None);
    }
}
