fn main() {
    // The app manifest adds the Windows 10/11 compatibility section to Tauri's default; see
    // windows-app-manifest.xml.
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run tauri-build");

    // tauri-build embeds the manifest in the app exe only. The updater tests (WIN-10.3) build a
    // mock app, which links `TaskDialogIndirect` from Common Controls 6; without the manifest a
    // test exe fails to load (STATUS_ENTRYPOINT_NOT_FOUND).
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        let manifest = std::path::Path::new(&dir).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
