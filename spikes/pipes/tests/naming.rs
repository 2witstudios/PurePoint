//! Criterion 9: DEC-2 pipe names from normalized ~/.pu paths.
#![cfg(windows)]

use std::process::Command;

use pipes_spike::naming::{daemon_pipe_name, normalize_pu_dir};

fn check_shape(name: &str) {
    let hash = name
        .strip_prefix(r"\\.\pipe\purepoint-")
        .and_then(|r| r.strip_suffix("-daemon"))
        .unwrap();
    assert_eq!(hash.len(), 16);
    assert!(
        hash.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
}

#[test]
fn given_two_temp_homes_should_yield_different_names() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let (na, nb) = (
        daemon_pipe_name(a.path()).unwrap(),
        daemon_pipe_name(b.path()).unwrap(),
    );
    println!("{} -> {na}", normalize_pu_dir(a.path()).unwrap());
    println!("{} -> {nb}", normalize_pu_dir(b.path()).unwrap());
    check_shape(&na);
    check_shape(&nb);
    assert_ne!(na, nb);
    // Stable across calls.
    assert_eq!(na, daemon_pipe_name(a.path()).unwrap());
}

#[test]
fn given_spellings_of_the_same_home_should_yield_one_name() {
    let home = tempfile::tempdir().unwrap();
    let base = normalize_pu_dir(home.path()).unwrap();
    let text = home.path().to_str().unwrap().to_owned();

    let mut variants = vec![
        text.to_uppercase(),
        text.to_lowercase(),
        text.replace('\\', "/"),
        format!("{text}\\"),
        format!(r"\\?\{text}"),
        format!("{text}\\sub\\.."),
    ];
    // Junction (no admin needed).
    let link_parent = tempfile::tempdir().unwrap();
    let link = link_parent.path().join("j");
    let ok = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(home.path())
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "mklink /J failed");
    variants.push(link.to_str().unwrap().to_owned());
    std::fs::create_dir_all(home.path().join("sub")).unwrap();

    // 8.3 short form, when the volume generates short names.
    let short = Command::new("cmd")
        .args([
            "/c", "for", "%I", "in", "(", &text, ")", "do", "@echo", "%~sI",
        ])
        .output()
        .unwrap();
    let short = String::from_utf8_lossy(&short.stdout).trim().to_owned();
    if !short.is_empty() && short != text {
        println!("8.3 variant: {short}");
        variants.push(short);
    } else {
        println!("8.3 variant: none (short names disabled or identical)");
    }

    println!("normalized: {base}");
    for v in variants {
        let n = normalize_pu_dir(std::path::Path::new(&v)).unwrap();
        println!("  {v} -> {n}");
        assert_eq!(n, base, "{v}");
    }
    let _ = std::fs::remove_dir(&link);
}
