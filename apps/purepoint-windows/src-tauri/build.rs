fn main() {
    // The app manifest adds the Windows 10/11 compatibility section to Tauri's default; see
    // windows-app-manifest.xml.
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run tauri-build");
}
