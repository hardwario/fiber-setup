fn main() {
    let mut attributes = tauri_build::Attributes::new();
    // On Windows the app asks for administrator rights when it starts (windows/app.manifest).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=windows/app.manifest");
        let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows/app.manifest"));
        attributes = attributes.windows_attributes(windows);
    }
    if let Err(error) = tauri_build::try_build(attributes) {
        println!("{error:#}");
        std::process::exit(1);
    }
}
