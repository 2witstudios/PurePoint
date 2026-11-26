use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::worktree::GitMetadata;
use super::*;
use crate::conversations::test_support::TempDir;

/// Watchers whose events land in a shared list.
fn recording() -> (Arc<Watchers>, Arc<Mutex<Vec<WatchEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let watchers = Watchers::new(Arc::new(move |event: &WatchEvent| {
        sink.lock().unwrap().push(event.clone());
    }));
    (watchers, events)
}

fn count(events: &Mutex<Vec<WatchEvent>>, pred: impl Fn(&WatchEvent) -> bool) -> usize {
    events.lock().unwrap().iter().filter(|e| pred(e)).count()
}

/// Waits until `pred` holds or `timeout` passes; returns whether it held.
fn wait_until(timeout: Duration, pred: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    pred()
}

/// Lets late events from the test's own setup arrive, then forgets them.
fn settle(events: &Mutex<Vec<WatchEvent>>) {
    std::thread::sleep(Duration::from_millis(700));
    events.lock().unwrap().clear();
}

/// The daemon's write: a temp file renamed over the target.
fn atomic_write(path: &Path, content: &str) {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

fn is_manifest(event: &WatchEvent) -> bool {
    matches!(event, WatchEvent::ManifestChanged { .. })
}

#[test]
fn path_keys_compare_spellings_and_whole_components() {
    let root = Path::new(r"C:\Proj\.pu\worktrees\wt-1");
    assert!(is_at_or_under(root, root));
    assert!(is_at_or_under(
        Path::new(r"C:\Proj\.pu\worktrees\wt-1\src\a.rs"),
        root
    ));
    assert!(is_at_or_under(
        Path::new("C:/Proj/.pu/worktrees/wt-1/"),
        root
    ));
    assert!(is_at_or_under(
        Path::new(r"\\?\C:\Proj\.pu\worktrees\wt-1"),
        root
    ));
    assert!(!is_at_or_under(
        Path::new(r"C:\Proj\.pu\worktrees\wt-10"),
        root
    ));
    assert!(!is_at_or_under(Path::new(r"C:\Proj\.pu\worktrees"), root));
    assert!(is_at_or_under(Path::new(r"C:\a"), Path::new(r"C:\")));
    if cfg!(windows) {
        assert!(is_at_or_under(
            Path::new(r"c:\proj\.PU\Worktrees\WT-1"),
            root
        ));
    }
}

#[test]
fn events_serialize_to_the_tauri_names_and_payloads() {
    let cases = [
        (
            WatchEvent::ManifestChanged {
                project_root: r"C:\p".into(),
            },
            "manifest-changed",
            serde_json::json!({ "projectRoot": r"C:\p" }),
        ),
        (
            WatchEvent::WorktreesChanged {
                worktree_path: r"C:\p\wt".into(),
            },
            "worktrees-changed",
            serde_json::json!({ "worktreePath": r"C:\p\wt" }),
        ),
        (
            WatchEvent::FileTreeChanged {
                root: r"C:\p".into(),
                paths: vec![r"C:\p\a".into()],
            },
            "file-tree-changed",
            serde_json::json!({ "root": r"C:\p", "paths": [r"C:\p\a"] }),
        ),
        (
            WatchEvent::MachinesChanged {},
            "machines-changed",
            serde_json::json!({}),
        ),
    ];
    for (event, name, json) in cases {
        assert_eq!(event.name(), name);
        assert_eq!(serde_json::to_value(&event).unwrap(), json);
    }
}

#[test]
fn manifest_atomic_rename_burst_fires_one_refresh() {
    let temp = TempDir::new();
    let manifest = temp.write(".pu/manifest.json", "{}");
    let (watchers, events) = recording();
    watchers.watch(WatchSpec::Manifest {
        project_root: temp.path().to_path_buf(),
    });
    settle(&events);

    for i in 0..20 {
        atomic_write(&manifest, &format!("{{\"n\":{i}}}"));
    }
    assert!(wait_until(Duration::from_secs(2), || count(
        &events,
        is_manifest
    ) >= 1));
    std::thread::sleep(Duration::from_millis(400));
    let burst = count(&events, is_manifest);
    assert!((1..=2).contains(&burst), "20 renames raised {burst} events");

    atomic_write(&manifest, "{\"n\":99}");
    assert!(wait_until(Duration::from_secs(2), || count(
        &events,
        is_manifest
    ) > burst));
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(count(&events, is_manifest), burst + 1);
    assert_eq!(
        events.lock().unwrap().last().unwrap(),
        &WatchEvent::ManifestChanged {
            project_root: display(temp.path()),
        }
    );
}

#[test]
fn manifest_ignores_other_files_and_waits_for_the_pu_dir() {
    let temp = TempDir::new();
    let (watchers, events) = recording();
    watchers.watch(WatchSpec::Manifest {
        project_root: temp.path().to_path_buf(),
    });
    settle(&events);
    temp.write("README.md", "hi");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(count(&events, is_manifest), 0);

    // `.pu` appears (pu init): the watcher moves from the root into it.
    let manifest = temp.write(".pu/manifest.json", "{}");
    assert!(wait_until(Duration::from_secs(2), || count(
        &events,
        is_manifest
    ) >= 1));
    std::thread::sleep(Duration::from_millis(300));
    let before = count(&events, is_manifest);
    temp.write(".pu/config.yaml", "x: 1");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        count(&events, is_manifest),
        before,
        "config.yaml is not the manifest"
    );
    atomic_write(&manifest, "{\"n\":1}");
    assert!(wait_until(Duration::from_secs(2), || count(
        &events,
        is_manifest
    ) > before));
}

#[test]
fn machine_registry_replace_raises_machines_changed() {
    let temp = TempDir::new();
    let registry = temp.path().join(".pu").join("machines.json");
    let (watchers, events) = recording();
    watchers.watch(WatchSpec::Machines {
        registry_path: registry.clone(),
    });
    settle(&events);
    let is_machines = |e: &WatchEvent| matches!(e, WatchEvent::MachinesChanged {});

    std::fs::create_dir_all(registry.parent().unwrap()).unwrap();
    std::fs::write(&registry, "{\"machines\":[]}").unwrap();
    assert!(wait_until(Duration::from_secs(2), || count(
        &events,
        is_machines
    ) >= 1));
    std::thread::sleep(Duration::from_millis(300));
    let before = count(&events, is_machines);
    atomic_write(&registry, "{\"machines\":[{}]}");
    assert!(wait_until(Duration::from_secs(2), || count(
        &events,
        is_machines
    ) > before));
}

/// A main checkout's `.git` directory with HEAD, index and a branch ref.
fn fake_repo(temp: &TempDir, name: &str) -> std::path::PathBuf {
    temp.write(&format!("{name}/.git/HEAD"), "ref: refs/heads/main\n");
    temp.write(&format!("{name}/.git/index"), "");
    temp.write(&format!("{name}/.git/refs/heads/main"), "0000\n");
    temp.write(&format!("{name}/src/lib.rs"), "");
    temp.path().join(name)
}

#[test]
fn resolves_linked_worktree_metadata() {
    let temp = TempDir::new();
    let main = fake_repo(&temp, "main");
    temp.write("main/.git/worktrees/wt/HEAD", "ref: refs/heads/wt\n");
    temp.write("main/.git/worktrees/wt/commondir", "../..\n");
    temp.write("wt/.git", "gitdir: ../main/.git/worktrees/wt\n");
    let meta = GitMetadata::resolve(&temp.path().join("wt"));
    assert_eq!(meta.marker, temp.path().join("wt").join(".git"));
    assert_eq!(
        path_key(&meta.git_dir),
        path_key(&main.join(".git/worktrees/wt"))
    );
    assert_eq!(path_key(&meta.common_dir), path_key(&main.join(".git")));
    let plain = GitMetadata::resolve(&main);
    assert_eq!(plain.git_dir, main.join(".git"));
    assert_eq!(plain.common_dir, main.join(".git"));
}

#[test]
fn worktree_watcher_raises_on_git_metadata_only() {
    let temp = TempDir::new();
    let wt = fake_repo(&temp, "wt");
    let (watchers, events) = recording();
    watchers.watch(WatchSpec::Worktree {
        worktree_path: wt.clone(),
    });
    settle(&events);
    let is_wt = |e: &WatchEvent| matches!(e, WatchEvent::WorktreesChanged { .. });

    std::fs::write(wt.join("src").join("lib.rs"), "fn main() {}").unwrap();
    std::fs::write(wt.join("notes.txt"), "x").unwrap();
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(count(&events, is_wt), 0, "working files are not metadata");

    // git commit: index and a ref replaced by rename.
    let lock = wt.join(".git").join("index.lock");
    std::fs::write(&lock, "new").unwrap();
    std::fs::rename(&lock, wt.join(".git").join("index")).unwrap();
    std::fs::write(wt.join(".git/refs/heads/main"), "1111\n").unwrap();
    assert!(wait_until(Duration::from_secs(3), || count(&events, is_wt) >= 1));
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(count(&events, is_wt), 1, "one burst, one refresh");
    assert_eq!(
        events.lock().unwrap().last().unwrap(),
        &WatchEvent::WorktreesChanged {
            worktree_path: display(&wt),
        }
    );
}

#[test]
fn file_tree_watcher_reports_paths_and_skips_hidden_names() {
    let temp = TempDir::new();
    let root = temp.path().join("tree");
    std::fs::create_dir_all(root.join("node_modules")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    let (watchers, events) = recording();
    watchers.watch(WatchSpec::FileTree { root: root.clone() });
    settle(&events);
    let is_tree = |e: &WatchEvent| matches!(e, WatchEvent::FileTreeChanged { .. });

    std::fs::write(root.join("node_modules").join("pkg.js"), "x").unwrap();
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(count(&events, is_tree), 0, "node_modules is hidden");

    let added = root.join("src").join("new.rs");
    std::fs::write(&added, "x").unwrap();
    assert!(wait_until(Duration::from_secs(3), || count(
        &events, is_tree
    ) >= 1));
    let events = events.lock().unwrap();
    let WatchEvent::FileTreeChanged { root: r, paths } = events.last().unwrap() else {
        unreachable!()
    };
    assert_eq!(r, &display(&root));
    assert!(
        paths
            .iter()
            .any(|p| path_key(Path::new(p)) == path_key(&added)),
        "{paths:?}"
    );
    assert!(super::file_tree::is_hidden(
        &root,
        &root.join(".git").join("HEAD")
    ));
    assert!(!super::file_tree::is_hidden(&root, &added));
}

#[test]
fn release_under_drops_only_watchers_inside_and_restore_recreates_them() {
    let temp = TempDir::new();
    let wt = fake_repo(&temp, "wt-1");
    let sibling = fake_repo(&temp, "wt-10");
    let (watchers, events) = recording();
    let tree = watchers.watch(WatchSpec::FileTree { root: wt.clone() });
    let meta = watchers.watch(WatchSpec::Worktree {
        worktree_path: wt.clone(),
    });
    let other = watchers.watch(WatchSpec::FileTree {
        root: sibling.clone(),
    });
    let manifest = watchers.watch(WatchSpec::Manifest {
        project_root: temp.path().to_path_buf(),
    });

    let guard = watchers.release_under(&wt);
    let released: Vec<WatchId> = guard.specs().iter().map(|(id, _)| *id).collect();
    assert_eq!(released, vec![tree, meta]);
    let active: Vec<WatchId> = watchers.active().iter().map(|(id, _)| *id).collect();
    assert_eq!(active, vec![other, manifest]);

    // Released watchers are silent.
    std::fs::write(wt.join("src").join("a.rs"), "x").unwrap();
    std::thread::sleep(Duration::from_millis(900));
    let in_wt = |e: &WatchEvent| matches!(e, WatchEvent::FileTreeChanged { root, .. } if *root == display(&wt));
    assert_eq!(count(&events, in_wt), 0);

    // The request failed: the same ids come back and raise again.
    assert_eq!(guard.restore(), 2);
    let active: Vec<WatchId> = watchers.active().iter().map(|(id, _)| *id).collect();
    assert_eq!(active, vec![tree, meta, other, manifest]);
    settle(&events);
    std::fs::write(wt.join("src").join("b.rs"), "x").unwrap();
    assert!(wait_until(Duration::from_secs(3), || count(&events, in_wt) >= 1));
    assert!(watchers.unwatch(tree));
    assert!(!watchers.unwatch(tree));
}

/// Fills a worktree-like directory with nested files.
fn populate(dir: &Path) {
    for i in 0..5 {
        let sub = dir.join(format!("d{i}")).join("nested");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("f.txt"), "x").unwrap();
    }
}

/// WIN-3.13: after a release the worktree directory can be deleted at once.
#[test]
fn released_worktree_can_be_removed() {
    let temp = TempDir::new();
    let wt = fake_repo(&temp, "wt");
    populate(&wt);
    let (watchers, events) = recording();
    watchers.watch(WatchSpec::FileTree { root: wt.clone() });
    watchers.watch(WatchSpec::Worktree {
        worktree_path: wt.clone(),
    });
    settle(&events);

    let started = Instant::now();
    let guard = watchers.release_under(&wt);
    assert_eq!(guard.specs().len(), 2);
    // The handles closed (notify dropped our handler), not the close timeout.
    assert!(started.elapsed() < CLOSE_TIMEOUT, "{:?}", started.elapsed());
    std::fs::remove_dir_all(&wt).expect("remove a released worktree");
    assert!(!wt.exists());
    // The removal succeeded: forgetting the guard leaves the watchers stopped.
    drop(guard);
    assert!(watchers.active().is_empty());
}

/// Records what removing a still-watched worktree does here (run with `--nocapture`).
///
/// Observed on Windows 11 (26300): `notify` opens its directory handles with
/// `FILE_SHARE_DELETE`, and std's `remove_dir_all` uses POSIX delete semantics, so the
/// removal succeeds even while watched; `cmd /c rd /s /q` (plain `RemoveDirectoryW`, as
/// git uses) also succeeds. The release still matters: handles from other tools and
/// non-POSIX deletes (FAT, network shares, older builds) fail with sharing violations,
/// and an orphaned watcher would otherwise outlive its directory (WINAPP-015).
#[test]
fn removing_a_watched_worktree_before_release() {
    let temp = TempDir::new();
    let wt = fake_repo(&temp, "wt");
    let rd = fake_repo(&temp, "wt-rd");
    populate(&wt);
    populate(&rd);
    let (watchers, events) = recording();
    for dir in [&wt, &rd] {
        watchers.watch(WatchSpec::FileTree { root: dir.clone() });
        watchers.watch(WatchSpec::Worktree {
            worktree_path: dir.clone(),
        });
    }
    settle(&events);

    let result = std::fs::remove_dir_all(&wt);
    eprintln!(
        "remove_dir_all while watched: {:?}; still exists: {}",
        result.as_ref().map_err(|e| (e.kind(), e.raw_os_error())),
        wt.exists()
    );
    if cfg!(windows) {
        let status = std::process::Command::new("cmd")
            .args(["/c", "rd", "/s", "/q"])
            .arg(&rd)
            .status()
            .unwrap();
        eprintln!(
            "rd /s /q while watched: {status}; still exists: {}",
            rd.exists()
        );
    }
    assert_eq!(watchers.release_under(temp.path()).specs().len(), 4);
    let _ = std::fs::remove_dir_all(&wt);
    let _ = std::fs::remove_dir_all(&rd);
    assert!(
        !wt.exists() && !rd.exists(),
        "removal succeeds once released"
    );
}
