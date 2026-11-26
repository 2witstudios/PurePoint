//! Port of `Services/MachineConnection.swift` and the registry half of `Models/Machine.swift`
//! (WIN-8.14): machines reachable over SSH, through the `ssh.exe` exec relay ([`super::relay`]).
//!
//! The Mac keeps one `ssh -L` forward per machine and probes the daemon through it. Windows has
//! no forward (WINAPP-013): every connection is its own relay, so a probe is one relayed
//! `health` whose exit status tells the states apart (REQ-WINAPP-054): a reply is a running (or
//! incompatible) daemon, exit 69 is no daemon (stopped), 127 or 9009 is no `pu` on the machine,
//! and 255 is ssh itself failing (offline). The front end's `MachineConnection`
//! (`ui/src/services/machines.ts`) runs the supervision loop and the probe tracker on top.
//!
//! - Registry: `%USERPROFILE%\.pu\machines.json`, read and written with pu-core's schema under
//!   its `machines.json.lock` (REQ-WINAPP-056), shared with `pu machine`.
//! - One [`DaemonClient`] per machine ([`MachineClients`]): its connector caches the `uname -s`
//!   probe, and every remote request, subscription and fs stream uses it.
//! - Start daemon runs the machine's start command with a 60 s limit; nothing else ever starts
//!   a remote daemon (MACH-003).
//!
//! Tauri surface (front end: `ui/src/services/machines.ts`): `machines_load`, `machines_add`,
//! `machines_remove`, `machines_ssh_config`, `machine_request`, `machine_probe`,
//! `machine_start_daemon` and `machine_test_connection`.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pu_core::machines::{self, Machine};
use pu_core::protocol::{PROTOCOL_VERSION, Request, Response};
use serde::Serialize;
use tauri::State;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use super::client::{DaemonClient, DaemonError, parse, timeout_for};
use super::relay::{self, RemoteShell, SSH_CONNECT_FAILED, SshRelayConnector};

/// `pu daemon proxy`'s exit status when nothing listens on the machine's socket.
pub const EXIT_NO_DAEMON: i32 = 69;
/// How long a relayed `health` may take: it includes the ssh handshake.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a relay that closed gets to report its exit status.
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);
/// Start daemon's limit (`docs/product/windows-app.md`, Remote relay).
pub const START_TIMEOUT: Duration = Duration::from_secs(60);

/// One `health` probe through the relay (Swift `HealthProbe`, plus what the relay's exit
/// status says on Windows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HealthProbe {
    Running,
    Incompatible {
        version: u32,
    },
    /// The daemon answered BUSY (at its connection limit): up, but not yet known compatible.
    Busy,
    /// ssh reached the machine and `pu daemon proxy` found no daemon (exit 69).
    Stopped,
    /// ssh could not reach the machine (exit 255, or no ssh at all), or `pu` is not installed
    /// there (exit 127 or 9009).
    Offline {
        reason: String,
    },
    /// No answer, an error, or the deadline passed: one failure, not yet a verdict.
    Failed {
        reason: String,
    },
}

/// Swift `MachineConnection.classify`, for a reply line.
pub fn classify(response: Result<Response, DaemonError>) -> HealthProbe {
    match response {
        Ok(Response::HealthReport {
            protocol_version, ..
        }) if protocol_version == PROTOCOL_VERSION => HealthProbe::Running,
        Ok(Response::HealthReport {
            protocol_version, ..
        }) => HealthProbe::Incompatible {
            version: protocol_version,
        },
        Err(DaemonError::Busy(_)) => HealthProbe::Busy,
        Ok(Response::Error { code, .. }) if code == "BUSY" => HealthProbe::Busy,
        Ok(Response::Error { message, .. }) => HealthProbe::Failed { reason: message },
        Ok(_) => HealthProbe::Failed {
            reason: "Unexpected reply to health".into(),
        },
        Err(error) => HealthProbe::Failed {
            reason: error.to_string(),
        },
    }
}

/// What a relay that closed without a reply means, from its exit status (the CLI's
/// `relay_exit_error`).
pub fn classify_exit(machine: &str, code: Option<i32>, stderr: Option<String>) -> HealthProbe {
    match code {
        Some(EXIT_NO_DAEMON) => HealthProbe::Stopped,
        Some(127 | 9009) => HealthProbe::Offline {
            reason: format!(
                "PurePoint is not installed on {machine} (`pu` was not found there). Install it there and connect again."
            ),
        },
        Some(SSH_CONNECT_FAILED) => HealthProbe::Offline {
            reason: stderr.unwrap_or_else(|| "ssh exited with status 255".into()),
        },
        _ => {
            let status = code.map_or("no exit status".to_string(), |c| format!("exit {c}"));
            HealthProbe::Failed {
                reason: match stderr {
                    Some(stderr) => {
                        format!("The relay closed without a reply ({status}: {stderr})")
                    }
                    None => format!("The relay closed without a reply ({status})"),
                },
            }
        }
    }
}

/// Probe the machine's daemon with one relayed `health`. Never starts anything.
pub async fn probe(connector: &SshRelayConnector) -> HealthProbe {
    let (mut child, target) = match connector.spawn(Stdio::piped()).await {
        Ok(spawned) => spawned,
        // No ssh, an unknown machine, or the `uname -s` probe could not reach the host.
        Err(DaemonError::NotRunning(reason)) => return HealthProbe::Offline { reason },
        Err(error) => {
            return HealthProbe::Failed {
                reason: error.to_string(),
            };
        }
    };
    let (Some(mut stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return HealthProbe::Failed {
            reason: "ssh has no stdio".into(),
        };
    };
    // Collected on its own task, bounded: ssh's diagnostics are why a connect failed.
    let stderr_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        let _ = stderr.take(16_384).read_to_end(&mut bytes).await;
        bytes
    });
    let mut line = serde_json::to_string(&Request::Health).unwrap_or_default();
    line.push('\n');
    let reply = async {
        stdin.write_all(line.as_bytes()).await.ok()?;
        stdin.flush().await.ok()?;
        let mut reader = BufReader::new(stdout);
        let mut text = String::new();
        match reader.read_line(&mut text).await {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(text),
        }
    };
    let outcome = tokio::time::timeout(PROBE_TIMEOUT, reply).await;
    let result = match outcome {
        Ok(Some(text)) => classify(parse(text.trim_end().as_bytes())),
        Ok(None) => {
            let code = match tokio::time::timeout(EXIT_TIMEOUT, child.wait()).await {
                Ok(Ok(status)) => status.code(),
                _ => None,
            };
            // A relay that ended has written all it will; one still running holds stderr open.
            let stderr = tokio::time::timeout(Duration::from_millis(500), stderr_task)
                .await
                .ok()
                .and_then(Result::ok)
                .and_then(|bytes| relay::last_line(&bytes));
            classify_exit(&target.name, code, stderr)
        }
        Err(_) => HealthProbe::Failed {
            reason: format!("No reply within {} s", PROBE_TIMEOUT.as_secs()),
        },
    };
    let _ = child.start_kill();
    result
}

/// Run the machine's start command over ssh; the error ssh gave, or none.
pub async fn start_daemon(connector: &SshRelayConnector) -> Result<(), String> {
    let target = relay::machine_target(connector.machine()).map_err(|e| e.to_string())?;
    let ssh = relay::find_ssh()
        .ok_or_else(|| "Could not find ssh.exe; install the OpenSSH client".to_string())?;
    let shell = connector
        .shell(&ssh, &target.ssh_host)
        .await
        .map_err(|e| e.to_string())?;
    let mut start = relay::command(&ssh);
    start
        .args(relay::start_args(&target.ssh_host, shell))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = start
        .spawn()
        .map_err(|e| format!("Could not run ssh: {e}"))?;
    match tokio::time::timeout(START_TIMEOUT, child.wait_with_output()).await {
        Err(_) => Err(format!("ssh timed out after {} s", START_TIMEOUT.as_secs())),
        Ok(Err(e)) => Err(format!("Could not run ssh: {e}")),
        Ok(Ok(output)) if output.status.success() => Ok(()),
        Ok(Ok(output)) => {
            Err(
                relay::last_line(&output.stderr).unwrap_or_else(|| match output.status.code() {
                    Some(code) => format!("ssh exited with status {code}"),
                    None => "ssh exited".into(),
                }),
            )
        }
    }
}

// MARK: - Registry

/// A registry error for the user: an invalid argument is shown without its prefix.
fn message(error: pu_core::error::PuError) -> String {
    match error {
        pu_core::error::PuError::InvalidArgument(message) => message,
        other => other.to_string(),
    }
}

fn registry_path() -> Result<std::path::PathBuf, String> {
    pu_core::paths::machines_registry_path().map_err(|e| format!("No machine registry: {e}"))
}

/// Add a machine under the registry lock. `id` is kept when given (lowercased), so something
/// keyed by the machine can exist before it is saved.
pub fn add(
    path: &std::path::Path,
    name: &str,
    ssh_host: &str,
    id: Option<&str>,
) -> Result<Machine, String> {
    machines::update(path, |registry| {
        machines::validate_name(name)?;
        machines::validate_ssh_host(ssh_host)?;
        if let Some(existing) = registry.find(name) {
            return Err(pu_core::error::PuError::InvalidArgument(format!(
                "A machine named \"{}\" already exists.",
                existing.name
            )));
        }
        let id = match id {
            Some(id) => {
                machines::validate_id(id)?;
                id.to_ascii_lowercase()
            }
            None => uuid::Uuid::new_v4().to_string(),
        };
        let machine = Machine {
            id,
            name: name.to_owned(),
            ssh_host: ssh_host.to_owned(),
        };
        registry.machines.push(machine.clone());
        Ok(machine)
    })
    .map_err(message)
}

/// Remove a machine by id under the registry lock; false when it was not registered.
pub fn remove(path: &std::path::Path, id: &str) -> Result<bool, String> {
    machines::update(path, |registry| {
        let before = registry.machines.len();
        registry
            .machines
            .retain(|machine| !machine.id.eq_ignore_ascii_case(id));
        Ok(registry.machines.len() != before)
    })
    .map_err(message)
}

// MARK: - Tauri state and commands

/// One daemon client per machine, so the relay's shell probe is made once.
#[derive(Default)]
pub struct MachineClients {
    clients: Mutex<HashMap<String, (Arc<SshRelayConnector>, DaemonClient)>>,
}

impl MachineClients {
    fn entry(&self, machine_id: &str) -> (Arc<SshRelayConnector>, DaemonClient) {
        let key = machine_id.to_ascii_lowercase();
        let mut clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
        clients
            .entry(key)
            .or_insert_with(|| {
                let connector = Arc::new(SshRelayConnector::new(machine_id));
                let client = DaemonClient::new(connector.clone());
                (connector, client)
            })
            .clone()
    }

    /// The machine's daemon client.
    pub fn client(&self, machine_id: &str) -> DaemonClient {
        self.entry(machine_id).1
    }

    fn connector(&self, machine_id: &str) -> Arc<SshRelayConnector> {
        self.entry(machine_id).0
    }

    /// Drop a machine's client (removed, or its host changed): the next one probes again.
    pub fn forget(&self, machine_id: &str) {
        let key = machine_id.to_ascii_lowercase();
        let removed = self
            .clients
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&key);
        if let Some((connector, _)) = removed {
            connector.reset();
        }
    }
}

/// The registered machines; a missing registry is none.
#[tauri::command]
pub async fn machines_load() -> Result<Vec<Machine>, String> {
    let path = registry_path()?;
    tauri::async_runtime::spawn_blocking(move || {
        machines::load(&path)
            .map(|registry| registry.machines)
            .map_err(message)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Register a machine (Add Machine).
#[tauri::command]
pub async fn machines_add(
    name: String,
    ssh_host: String,
    id: Option<String>,
) -> Result<Machine, String> {
    let path = registry_path()?;
    tauri::async_runtime::spawn_blocking(move || add(&path, &name, &ssh_host, id.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Forget a machine (Remove Machine). Its daemon and agents keep running (MACH-004).
#[tauri::command]
pub async fn machines_remove(
    clients: State<'_, MachineClients>,
    id: String,
) -> Result<bool, String> {
    clients.forget(&id);
    let path = registry_path()?;
    tauri::async_runtime::spawn_blocking(move || remove(&path, &id))
        .await
        .map_err(|e| e.to_string())?
}

/// `%USERPROFILE%\.ssh\config`, for the Add Machine host list; empty when there is none.
#[tauri::command]
pub async fn machines_ssh_config() -> String {
    let Ok(home) = pu_core::paths::home_dir() else {
        return String::new();
    };
    tokio::fs::read_to_string(home.join(".ssh").join("config"))
        .await
        .unwrap_or_default()
}

/// One request to a machine's daemon, through the relay.
#[tauri::command]
pub async fn machine_request(
    clients: State<'_, MachineClients>,
    machine_id: String,
    json: serde_json::Value,
) -> Result<serde_json::Value, DaemonError> {
    let request: Request =
        serde_json::from_value(json).map_err(|e| DaemonError::InvalidRequest(e.to_string()))?;
    let client = clients.client(&machine_id);
    let response = client
        .request_with_timeout(&request, timeout_for(&request))
        .await?;
    serde_json::to_value(response)
        .map_err(|e| DaemonError::Io(format!("Could not encode the reply: {e}")))
}

/// Probe a machine's daemon with `health` (never starts it).
#[tauri::command]
pub async fn machine_probe(
    clients: State<'_, MachineClients>,
    machine_id: String,
) -> Result<HealthProbe, ()> {
    let connector = clients.connector(&machine_id);
    Ok(probe(&connector).await)
}

/// Start the machine's daemon (an explicit user action); the error, or null.
#[tauri::command]
pub async fn machine_start_daemon(
    clients: State<'_, MachineClients>,
    machine_id: String,
) -> Result<Option<String>, ()> {
    let connector = clients.connector(&machine_id);
    Ok(start_daemon(&connector).await.err())
}

/// Add Machine's Test Connection: can ssh reach `ssh_host` without prompting?
#[tauri::command]
pub async fn machine_test_connection(ssh_host: String) -> Result<String, String> {
    machines::validate_ssh_host(&ssh_host).map_err(|e| e.to_string())?;
    let ssh = relay::find_ssh()
        .ok_or_else(|| "Could not find ssh.exe; install the OpenSSH client".to_string())?;
    match relay::probe_shell(&ssh, &ssh_host).await {
        Ok(RemoteShell::Unix) => Ok("Connected. The machine runs a Unix shell.".into()),
        Ok(RemoteShell::Windows) => Ok("Connected. The machine runs Windows.".into()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "6f1c2b3a-1111-4222-8333-944455556666";

    #[test]
    fn given_cli_registry_should_decode() {
        // MachinesTests.givenCLIRegistryShouldDecode
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("machines.json");
        std::fs::write(
            &path,
            format!(
                r#"{{ "version": 1, "machines": [ {{ "id": "{ID}", "name": "studio", "ssh_host": "studio-mac" }} ] }}"#
            ),
        )
        .unwrap();
        let registry = machines::load(&path).unwrap();
        assert_eq!(
            registry.machines,
            [Machine {
                id: ID.into(),
                name: "studio".into(),
                ssh_host: "studio-mac".into()
            }]
        );
    }

    #[test]
    fn given_unknown_version_should_refuse_to_load() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("machines.json");
        std::fs::write(&path, r#"{ "version": 2, "machines": [] }"#).unwrap();
        let error = machines::load(&path).unwrap_err().to_string();
        assert!(
            error.contains("unsupported machine registry version 2"),
            "{error}"
        );
    }

    #[test]
    fn given_add_should_write_the_shared_format_and_reject_duplicate_names_in_any_case() {
        // MachinesTests.givenUpdateShouldWriteAPrivateFileAndRejectDuplicateNamesInAnyCase
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join(".pu").join("machines.json");
        assert!(machines::load(&path).unwrap().machines.is_empty());

        let added = add(&path, "Studio", "me@host:2222", Some(&ID.to_uppercase())).unwrap();
        assert_eq!(added.id, ID);
        let text = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["machines"][0]["ssh_host"], "me@host:2222");
        assert!(json["machines"][0].get("sshHost").is_none());
        pu_core::private_fs::check_private(&path).unwrap();

        let duplicate = add(&path, "studio", "other", None).unwrap_err();
        assert_eq!(duplicate, "A machine named \"Studio\" already exists.");
        assert_eq!(machines::load(&path).unwrap().machines.len(), 1);

        let second = add(&path, "build", "build-box", None).unwrap();
        assert!(uuid::Uuid::parse_str(&second.id).is_ok());
        assert!(remove(&path, &ID.to_uppercase()).unwrap());
        assert!(!remove(&path, ID).unwrap());
        assert_eq!(machines::load(&path).unwrap().machines, [second]);
    }

    #[test]
    fn given_invalid_names_or_hosts_should_reject() {
        // MachinesTests.givenInvalidNamesOrHostsShouldReject
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("machines.json");
        for name in ["", "-x", ".hidden", "has space", "slash/name", "colon:x"] {
            assert!(add(&path, name, "host", None).is_err(), "{name}");
        }
        assert!(add(&path, &"a".repeat(65), "host", None).is_err());
        for host in ["", "-oProxyCommand=x", "a b", "a\nb"] {
            assert!(add(&path, "ok", host, None).is_err(), "{host:?}");
        }
        assert!(add(&path, "ok", "host", Some("not-a-uuid")).is_err());
        for (name, host) in [
            ("studio", "studio-mac"),
            ("Mac-2", "me@10.0.0.2"),
            ("build_box.local", "me@host:2222"),
            ("1", "box"),
        ] {
            add(&path, name, host, None).unwrap();
        }
    }

    #[test]
    fn given_health_replies_should_classify() {
        // MachinesTests.givenHealthRepliesShouldClassify
        let report = |protocol_version| Response::HealthReport {
            pid: 1,
            uptime_seconds: 1,
            protocol_version,
            projects: vec![],
            agent_count: 0,
        };
        assert_eq!(classify(Ok(report(PROTOCOL_VERSION))), HealthProbe::Running);
        assert_eq!(
            classify(Ok(report(2))),
            HealthProbe::Incompatible { version: 2 }
        );
        assert_eq!(
            classify(Err(DaemonError::Busy("full".into()))),
            HealthProbe::Busy
        );
        assert_eq!(
            classify(Ok(Response::Error {
                code: "IO".into(),
                message: "x".into()
            })),
            HealthProbe::Failed { reason: "x".into() }
        );
        assert!(matches!(
            classify(Err(DaemonError::Eof)),
            HealthProbe::Failed { .. }
        ));
    }

    #[test]
    fn given_relay_exit_status_should_tell_stopped_from_offline() {
        assert_eq!(
            classify_exit("studio", Some(69), None),
            HealthProbe::Stopped
        );
        for code in [127, 9009] {
            let HealthProbe::Offline { reason } = classify_exit("studio", Some(code), None) else {
                panic!("{code} is offline");
            };
            assert!(
                reason.contains("PurePoint is not installed on studio"),
                "{reason}"
            );
        }
        assert_eq!(
            classify_exit("studio", Some(255), Some("ssh: connect refused".into())),
            HealthProbe::Offline {
                reason: "ssh: connect refused".into()
            }
        );
        assert!(matches!(
            classify_exit("studio", Some(1), None),
            HealthProbe::Failed { .. }
        ));
        assert!(matches!(
            classify_exit("studio", None, None),
            HealthProbe::Failed { .. }
        ));
    }

    #[test]
    fn given_probes_should_serialize_tagged_camel_case() {
        assert_eq!(
            serde_json::to_value(HealthProbe::Incompatible { version: 3 }).unwrap(),
            serde_json::json!({ "kind": "incompatible", "version": 3 })
        );
        assert_eq!(
            serde_json::to_value(HealthProbe::Stopped).unwrap(),
            serde_json::json!({ "kind": "stopped" })
        );
    }
}
