//! The updater (WIN-10.3) against a feed served from 127.0.0.1: newer, same and replayed
//! versions, a signature from another key, the placeholder key, an unreachable feed. Nothing is
//! installed: a verified download stops at the bytes the installer would run.
//!
//! An integration test, not a unit test: the mock app links `TaskDialogIndirect`, so the test
//! exe needs the app manifest, which build.rs embeds in test targets only.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use purepoint_windows_lib::updater::{
    PLACEHOLDER_PUBKEY, UpdaterService, UpdaterStatus, app_updater, configured_pubkey,
    is_release_key,
};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

// A throwaway minisign key pair made for these tests with `tauri signer generate` in a temp
// dir; only the public key and two signatures of PAYLOAD are kept. FOREIGN_SIG is the same
// payload signed by a second throwaway key. Both sign version 0.2.0 (`--app-version`).
const DEV_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEMyNEJDMDNFQUQ1QkIwMEUKUldRT3NGdXRQc0JMd2tCZzg5dGxnWnV1Q3RHdkdWSVM2Z1NzaE5iL1NVOWp6YWlRVmY2aG1FengK";
const PAYLOAD: &[u8] = b"PurePoint updater test payload (WIN-10.3). Not an installer.\n";
const GOOD_SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRT3NGdXRQc0JMd2lOZE1JQ1o4emVoT2lBSVhpZDNjNGZlVW1xOUdNZXd5cWVybjNwOVN1UmhseW44QmV6NytsdDVOak5Fazl0eExibUgxOFY5OENadEw3R0cvaVdBcWdFPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk1Njg5MDEyCWZpbGU6cGF5bG9hZC5iaW4JdmVyc2lvbjowLjIuMApZL21GUFcxN0pHS2RNM1FvOWMwOS9DRFNERXdRUFNsdFRZTWdXTE9OZ2p4bTZ0d2JrVGNRU3M4WVhSdzA2Q0M1aFhOdFVEUlVaN3hDSEVrMHpRSjZBdz09Cg==";
const FOREIGN_SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSOHUrZm10RVF0ZEtzS2VqaG4vZ01LRnRaOGR4dXFFTFQ2N1hNaDZYNWcrTTlHa2VPekZZUTNKTVFZZHpiM29yNlFnQUQ5ajZodlE2ai93WXpPaVB5dzZNcWg0dE93cUE0PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk1Njg5MDEyCWZpbGU6cGF5bG9hZC5iaW4JdmVyc2lvbjowLjIuMApRd3JsTUw2NWNyU1c2RWkrMXFhSjJLUXFMZFEzcHJzQ2VtaE9aYlYyWlZ3UUJvTDNIZFZMdVFRN3U2K3BnVi9jUVhsZ2RvR2ZyR20xa0lQNm9PYTBDZz09Cg==";

/// A one-file-per-path HTTP server on 127.0.0.1, counting requests.
struct FeedServer {
    base: String,
    requests: Arc<AtomicUsize>,
}

fn respond(mut stream: TcpStream, routes: &[(String, Vec<u8>)], requests: &AtomicUsize) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => break,
            Ok(_) if header == "\r\n" || header == "\n" => break,
            Ok(_) => {}
        }
    }
    requests.fetch_add(1, Ordering::SeqCst);
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let reply = match routes.iter().find(|(route, _)| route == path) {
        Some((_, body)) => {
            let mut reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            reply.extend_from_slice(body);
            reply
        }
        None => {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        }
    };
    let _ = stream.write_all(&reply);
    let _ = stream.flush();
    let mut rest = [0u8; 64];
    let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
    let _ = stream.read(&mut rest);
}

impl FeedServer {
    /// Serves `/latest-windows.json` (announcing `version`, signed `signature`) and `/setup.exe`.
    fn start(version: &str, signature: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let platform =
            serde_json::json!({ "signature": signature, "url": format!("{base}/setup.exe") });
        let feed = serde_json::json!({
            "version": version,
            "notes": "Faster agents.",
            "pub_date": "2026-11-20T12:00:00Z",
            "platforms": {
                "windows-x86_64": platform,
                "windows-aarch64": platform,
                "windows-x86_64-nsis": platform,
                "windows-aarch64-nsis": platform,
            },
        });
        let routes = vec![
            (
                "/latest-windows.json".to_owned(),
                serde_json::to_vec(&feed).expect("feed json"),
            ),
            ("/setup.exe".to_owned(), PAYLOAD.to_vec()),
        ];
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                respond(stream, &routes, &counter);
            }
        });
        Self { base, requests }
    }

    fn feed_url(&self) -> String {
        format!("{}/latest-windows.json", self.base)
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

/// A mock app (version 0.1.0) with the updater plugin pointed at `endpoint`.
fn app(endpoint: &str, pubkey: &str) -> tauri::App<MockRuntime> {
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".to_owned(),
        serde_json::json!({
            "pubkey": pubkey,
            "endpoints": [endpoint],
            "requireSignedVersion": true,
            "dangerousInsecureTransportProtocol": true,
        }),
    );
    mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .expect("mock app")
}

fn service_for(app: &tauri::App<MockRuntime>) -> UpdaterService {
    UpdaterService::new(
        configured_pubkey(app.config()).is_some(),
        app.package_info().version.to_string(),
    )
}

fn recorded(service: &UpdaterService) -> Arc<Mutex<Vec<UpdaterStatus>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    service.on_change(move |status| lock(&sink).push(status.clone()));
    seen
}

#[tokio::test]
async fn given_newer_signed_release_should_offer_download_and_verify_it() {
    let server = FeedServer::start("0.2.0", GOOD_SIG);
    let app = app(&server.feed_url(), DEV_PUBKEY);
    let service = service_for(&app);
    let seen = recorded(&service);

    let status = service.check(|| app_updater(app.handle())).await;
    assert_eq!(
        status,
        UpdaterStatus::Available {
            version: "0.2.0".into(),
            current_version: "0.1.0".into(),
            notes: Some("Faster agents.".into()),
            date: Some("2026-11-20T12:00:00Z".into()),
        }
    );
    assert_eq!(lock(&seen)[0], UpdaterStatus::Checking);

    let status = service.download().await;
    assert_eq!(
        status,
        UpdaterStatus::ReadyToRestart {
            version: "0.2.0".into()
        }
    );
    assert!(lock(&seen).iter().any(|s| matches!(
        s,
        UpdaterStatus::Downloading { downloaded, .. } if *downloaded == PAYLOAD.len() as u64
    )));
    // The verified bytes are what the installer would run; nothing is installed here.
    let (update, bytes) = service.take_for_install().expect("ready to install");
    assert_eq!(bytes, PAYLOAD);
    assert!(matches!(service.status(), UpdaterStatus::Installing { .. }));
    service.restore(update, bytes);
    assert!(matches!(
        service.status(),
        UpdaterStatus::ReadyToRestart { .. }
    ));
}

#[tokio::test]
async fn given_same_version_should_be_up_to_date() {
    let server = FeedServer::start("0.1.0", GOOD_SIG);
    let app = app(&server.feed_url(), DEV_PUBKEY);
    let service = service_for(&app);

    let status = service.check(|| app_updater(app.handle())).await;
    assert_eq!(
        status,
        UpdaterStatus::UpToDate {
            current_version: "0.1.0".into()
        }
    );
    assert_eq!(server.requests(), 1);
    // Nothing to download or install.
    assert_eq!(service.download().await, status);
    assert!(service.take_for_install().is_none());
}

#[tokio::test]
async fn given_signature_from_another_key_should_fail_and_keep_nothing() {
    let server = FeedServer::start("0.2.0", FOREIGN_SIG);
    let app = app(&server.feed_url(), DEV_PUBKEY);
    let service = service_for(&app);

    assert!(matches!(
        service.check(|| app_updater(app.handle())).await,
        UpdaterStatus::Available { .. }
    ));
    let status = service.download().await;
    let UpdaterStatus::Error { message } = &status else {
        panic!("expected an error, got {status:?}");
    };
    assert!(
        message.starts_with("The update is improperly signed"),
        "{message}"
    );
    assert!(service.take_for_install().is_none());
}

#[tokio::test]
async fn given_signature_for_another_version_should_fail() {
    // GOOD_SIG is bound to 0.2.0; a feed announcing 0.3.0 with it is a replay.
    let server = FeedServer::start("0.3.0", GOOD_SIG);
    let app = app(&server.feed_url(), DEV_PUBKEY);
    let service = service_for(&app);

    service.check(|| app_updater(app.handle())).await;
    let status = service.download().await;
    assert!(
        matches!(&status, UpdaterStatus::Error { message } if message.contains("signed for version 0.2.0")),
        "{status:?}"
    );
}

#[tokio::test]
async fn given_placeholder_key_should_not_be_configured_and_never_fetch() {
    let server = FeedServer::start("0.2.0", GOOD_SIG);
    let app = app(&server.feed_url(), PLACEHOLDER_PUBKEY);
    let service = service_for(&app);
    assert!(!service.is_configured());
    assert_eq!(service.status(), UpdaterStatus::NotConfigured);

    let status = service
        .check(|| panic!("no updater is built without a release key"))
        .await;
    assert_eq!(status, UpdaterStatus::NotConfigured);
    assert_eq!(service.download().await, UpdaterStatus::NotConfigured);
    assert!(service.take_for_install().is_none());
    assert_eq!(server.requests(), 0);
}

#[tokio::test]
async fn given_unreachable_feed_should_report_retrieval_error() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!(
        "http://{}/latest-windows.json",
        listener.local_addr().expect("addr")
    );
    drop(listener);
    let app = app(&url, DEV_PUBKEY);
    let service = service_for(&app);
    let status = service.check(|| app_updater(app.handle())).await;
    assert!(
        matches!(&status, UpdaterStatus::Error { message } if message.starts_with("An error occurred in retrieving update information")),
        "{status:?}"
    );
    // A failed check may be retried.
    assert!(status.can_check());
}

#[test]
fn committed_config_carries_the_placeholder_and_the_appcast_feed() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
    let updater = &config["plugins"]["updater"];
    assert_eq!(updater["pubkey"], PLACEHOLDER_PUBKEY);
    assert_eq!(
        updater["endpoints"],
        serde_json::json!([
            "https://github.com/2witstudios/purepoint/releases/download/appcast/latest-windows.json"
        ])
    );
    assert_eq!(updater["requireSignedVersion"], true);
    assert!(!is_release_key(PLACEHOLDER_PUBKEY));
    assert!(!is_release_key("  "));
    assert!(is_release_key(DEV_PUBKEY));
}

#[test]
fn given_status_should_serialize_for_the_ui() {
    let json = serde_json::to_value(UpdaterStatus::Downloading {
        version: "0.2.0".into(),
        downloaded: 10,
        total: Some(20),
    })
    .expect("json");
    assert_eq!(
        json,
        serde_json::json!({"state": "downloading", "version": "0.2.0", "downloaded": 10, "total": 20})
    );
    assert_eq!(
        serde_json::to_value(UpdaterStatus::NotConfigured).expect("json"),
        serde_json::json!({"state": "notConfigured"})
    );
    assert_eq!(
        serde_json::to_value(UpdaterStatus::UpToDate {
            current_version: "0.1.0".into()
        })
        .expect("json"),
        serde_json::json!({"state": "upToDate", "currentVersion": "0.1.0"})
    );
}
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
