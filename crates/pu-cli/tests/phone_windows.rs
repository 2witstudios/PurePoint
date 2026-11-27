//! `pu phone` on Windows through the real binary, on a temp home. Only paths
//! that fail before touching the registry or starting anything run here; the
//! adapters' behaviour is covered by `phone_windows.rs` unit tests.
#![cfg(windows)]

fn pu(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_pu"))
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("LOCALAPPDATA", home.join(r"AppData\Local"))
        .output()
        .unwrap()
}

#[test]
fn given_no_running_bridge_should_say_so_for_admin_subcommands() {
    let tmp = tempfile::TempDir::new().unwrap();
    for args in [
        &["phone", "devices", "--json"][..],
        &["phone", "pair", "--text"],
        &["phone", "revoke", "iPhone"],
        &["phone", "model"],
        &["phone", "login"],
        &["phone", "grant", "iPhone", "--terminals", "view"],
    ] {
        let output = pu(tmp.path(), args);
        assert!(!output.status.success(), "{args:?} succeeded");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Point Guard is not running"),
            "{args:?}: {stderr}"
        );
        assert!(!stderr.contains("not available on Windows"), "{stderr}");
    }
}

#[test]
fn given_no_payload_should_refuse_install_start_and_run() {
    let tmp = tempfile::TempDir::new().unwrap();
    for args in [
        &["phone", "install", "--now"][..],
        &["phone", "start"],
        &["phone", "run"],
        &["phone", "restart"],
    ] {
        let output = pu(tmp.path(), args);
        assert!(!output.status.success(), "{args:?} succeeded");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("phone access is not installed for pu")
                && stderr.contains("runtime-manifest.json"),
            "{args:?}: {stderr}"
        );
    }
}
