//! `pu phone` on Windows refuses every subcommand until WIN-9.2.
#![cfg(windows)]

#[test]
fn given_any_phone_subcommand_on_windows_should_fail_with_a_clear_message() {
    let tmp = tempfile::TempDir::new().unwrap();
    for args in [
        &["phone", "status"][..],
        &["phone", "install"],
        &["phone", "pair"],
        &["phone", "devices", "--json"],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_pu"))
            .args(args)
            .env("HOME", tmp.path())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?} succeeded");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("pu phone is not available on Windows yet"),
            "{args:?}: {stderr}"
        );
    }
}
