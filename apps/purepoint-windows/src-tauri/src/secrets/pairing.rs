//! The Point Guard pairing secret (WIN-9.7): the Swift `PiPairingSecret`.
//!
//! The secret lives in the Windows Credential Manager through the `keyring` crate, as a
//! generic credential with service [`SERVICE`] and the endpoint as account, the names the
//! Mac's Keychain item uses (`keyring` names it `<endpoint>.<service>`, with the endpoint as
//! user name). It is never written to a file.
//!
//! Tauri surface (front end: `ui/src/services/pairingSecret.ts`): `pointguard_pairing_read`
//! (the chat's secret for an endpoint, empty when none is saved), `pointguard_pairing_save`
//! (the chat binding an endpoint) and `pointguard_pairing_import` (Settings: a pairing code,
//! or an address and secret typed in), which validates, saves and returns the endpoint.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Deserialize;

/// The Keychain service name, kept on Windows.
pub const SERVICE: &str = "PurePoint.PointGuard.Pairing";

/// Where secrets are kept: the Credential Manager, or memory in tests.
pub trait SecretStore: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String>;
    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, service: &str, account: &str) -> Result<(), String>;
}

/// The Windows Credential Manager (`keyring`'s native Windows backend).
pub struct CredentialManager;

impl SecretStore for CredentialManager {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(service, account).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), String> {
        keyring::Entry::new(service, account)
            .and_then(|entry| entry.set_password(secret))
            .map_err(|e| e.to_string())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(service, account).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Secrets in memory, for tests.
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<(String, String), String>>);

impl MemoryStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), String>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        Ok(self.lock().get(&(service.into(), account.into())).cloned())
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), String> {
        self.lock()
            .insert((service.into(), account.into()), secret.into());
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        self.lock().remove(&(service.into(), account.into()));
        Ok(())
    }
}

/// A Point Guard chat address in canonical form (the Swift `PiConnectionAddress` and
/// `url.absoluteString`): ws or wss, path `/v1`, no credentials, query or fragment, and a
/// loopback, Tailscale (100.64.0.0/10, fd7a:115c:a1e0::/48) or `*.ts.net` host.
pub fn endpoint(input: &str) -> Option<String> {
    let input = input.trim();
    let url = url::Url::parse(input).ok()?;
    if !matches!(url.scheme(), "ws" | "wss")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || input.contains(['?', '#'])
        || url.path() != "/v1"
    {
        return None;
    }
    let allowed = match url.host()? {
        url::Host::Ipv4(ip) => {
            let [a, b, ..] = ip.octets();
            ip.octets() == [127, 0, 0, 1] || (a == 100 && (64..=127).contains(&b))
        }
        url::Host::Ipv6(ip) => ip.is_loopback() || ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
        url::Host::Domain(name) => name == "localhost" || name.ends_with(".ts.net"),
    };
    allowed.then(|| url.as_str().to_owned())
}

/// A secret the bridge can take as a bearer header: 32 characters to 1 KiB, one line.
pub fn valid_secret(secret: &str) -> bool {
    secret.chars().count() >= 32 && secret.len() <= 1024 && !secret.contains(['\n', '\r'])
}

/// The saved secret for `endpoint`, or empty when there is none or it cannot be read
/// (`PiPairingSecret.read`).
pub fn read(store: &dyn SecretStore, endpoint: &str) -> String {
    store
        .get(SERVICE, endpoint)
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Save (or replace) the secret for `endpoint` (`PiPairingSecret.save`).
pub fn save(store: &dyn SecretStore, secret: &str, endpoint: &str) -> Result<(), String> {
    store
        .set(SERVICE, endpoint, secret)
        .map_err(|e| format!("Could not save the pairing secret in Credential Manager ({e})."))
}

/// What Settings imports: a pairing code (the `pi-mobile-pairing` v1 JSON), or an address
/// and secret typed in.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingImport {
    pub code: Option<String>,
    pub endpoint: Option<String>,
    pub secret: Option<String>,
}

#[derive(Deserialize)]
struct PairingCode {
    #[serde(rename = "type")]
    kind: String,
    version: u32,
    endpoint: String,
    secret: String,
}

pub const BAD_ADDRESS: &str =
    "Enter a Tailscale address ending in /v1, such as ws://100.100.1.2:8787/v1.";
pub const BAD_SECRET: &str = "Enter the pairing secret from Point Guard (at least 32 characters).";
pub const BAD_CODE: &str = "This is not a Point Guard pairing code.";

fn checked(address: &str, secret: &str) -> Result<String, String> {
    let address = endpoint(address).ok_or(BAD_ADDRESS)?;
    if !valid_secret(secret) {
        return Err(BAD_SECRET.into());
    }
    Ok(address)
}

/// Validate an import and save its secret; returns the canonical endpoint.
pub fn import(store: &dyn SecretStore, request: &PairingImport) -> Result<String, String> {
    let (address, secret) = match &request.code {
        Some(text) => {
            let code = (text.len() <= 8192)
                .then(|| serde_json::from_str::<PairingCode>(text).ok())
                .flatten()
                .filter(|c| c.kind == "pi-mobile-pairing" && c.version == 1)
                .ok_or(BAD_CODE)?;
            (code.endpoint, code.secret)
        }
        None => (
            request.endpoint.clone().unwrap_or_default(),
            request.secret.clone().unwrap_or_default(),
        ),
    };
    let address = checked(&address, &secret)?;
    save(store, &secret, &address)?;
    Ok(address)
}

/// Tauri state: the store the commands use.
#[derive(Clone)]
pub struct PairingSecrets(pub Arc<dyn SecretStore>);

impl Default for PairingSecrets {
    fn default() -> Self {
        Self(Arc::new(CredentialManager))
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

/// The saved secret for `endpoint` ('' when none).
#[tauri::command]
pub async fn pointguard_pairing_read(
    secrets: tauri::State<'_, PairingSecrets>,
    endpoint: String,
) -> Result<String, String> {
    let store = secrets.0.clone();
    blocking(move || Ok(read(store.as_ref(), &endpoint))).await
}

/// Save the secret for `endpoint`.
#[tauri::command]
pub async fn pointguard_pairing_save(
    secrets: tauri::State<'_, PairingSecrets>,
    endpoint: String,
    secret: String,
) -> Result<(), String> {
    let store = secrets.0.clone();
    blocking(move || {
        let address = checked(&endpoint, &secret)?;
        save(store.as_ref(), &secret, &address)
    })
    .await
}

/// Settings' import: a pairing code or an address and secret. Returns the endpoint saved.
#[tauri::command]
pub async fn pointguard_pairing_import(
    secrets: tauri::State<'_, PairingSecrets>,
    request: PairingImport,
) -> Result<String, String> {
    let store = secrets.0.clone();
    blocking(move || import(store.as_ref(), &request)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn given_addresses_should_accept_loopback_and_the_tailnet_in_canonical_form() {
        let ok = |input: &str| endpoint(input);
        assert_eq!(
            ok(" ws://127.0.0.1:8787/v1 ").as_deref(),
            Some("ws://127.0.0.1:8787/v1")
        );
        assert_eq!(
            ok("WSS://100.100.1.2:8787/v1").as_deref(),
            Some("wss://100.100.1.2:8787/v1")
        );
        assert_eq!(
            ok("wss://Mac.tail1.ts.net/v1").as_deref(),
            Some("wss://mac.tail1.ts.net/v1")
        );
        assert_eq!(ok("ws://[::1]:9/v1").as_deref(), Some("ws://[::1]:9/v1"));
        assert!(ok("wss://[fd7a:115c:a1e0::1]:9/v1").is_some());
        for bad in [
            "http://127.0.0.1:1/v1",
            "ws://8.8.8.8:1/v1",
            "ws://100.128.0.1:1/v1",
            "ws://127.0.0.2:1/v1",
            "ws://127.0.0.1:1/v2",
            "ws://127.0.0.1:1/v1?",
            "ws://u:p@127.0.0.1:1/v1",
            "ws://example.com/v1",
        ] {
            assert!(ok(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn given_a_saved_secret_should_read_it_back_by_endpoint() {
        let store = MemoryStore::default();
        assert_eq!(read(&store, "ws://127.0.0.1:1/v1"), "");
        save(&store, SECRET, "ws://127.0.0.1:1/v1").unwrap();
        save(&store, &SECRET.repeat(2), "ws://127.0.0.1:1/v1").unwrap();
        assert_eq!(read(&store, "ws://127.0.0.1:1/v1"), SECRET.repeat(2));
        assert_eq!(read(&store, "ws://127.0.0.1:2/v1"), "");
        assert_eq!(
            store.get(SERVICE, "ws://127.0.0.1:1/v1").unwrap(),
            Some(SECRET.repeat(2))
        );
    }

    #[test]
    fn given_a_settings_import_should_validate_and_save() {
        let store = MemoryStore::default();
        let typed = PairingImport {
            code: None,
            endpoint: Some("ws://100.100.1.2:8787/v1".into()),
            secret: Some(SECRET.into()),
        };
        assert_eq!(import(&store, &typed).unwrap(), "ws://100.100.1.2:8787/v1");
        assert_eq!(read(&store, "ws://100.100.1.2:8787/v1"), SECRET);

        let code = serde_json::json!({
            "type": "pi-mobile-pairing", "version": 1,
            "endpoint": "wss://pc.tail1.ts.net/v1", "secret": SECRET,
        })
        .to_string();
        let scanned = PairingImport {
            code: Some(code.clone()),
            ..PairingImport::default()
        };
        assert_eq!(
            import(&store, &scanned).unwrap(),
            "wss://pc.tail1.ts.net/v1"
        );

        let short = PairingImport {
            secret: Some("short".into()),
            ..typed.clone()
        };
        assert_eq!(import(&store, &short).unwrap_err(), BAD_SECRET);
        let two_lines = PairingImport {
            secret: Some(format!("{SECRET}\nx")),
            ..typed.clone()
        };
        assert_eq!(import(&store, &two_lines).unwrap_err(), BAD_SECRET);
        let public = PairingImport {
            endpoint: Some("ws://8.8.8.8:1/v1".into()),
            ..typed
        };
        assert_eq!(import(&store, &public).unwrap_err(), BAD_ADDRESS);
        let v2 = PairingImport {
            code: Some(code.replace("\"version\":1", "\"version\":2")),
            ..PairingImport::default()
        };
        assert_eq!(import(&store, &v2).unwrap_err(), BAD_CODE);
        assert_eq!(read(&store, "ws://8.8.8.8:1/v1"), "");
    }

    /// One live round trip through the real Credential Manager, under a unique test-only
    /// service name that is deleted afterwards. The product service name is never touched.
    #[cfg(windows)]
    #[test]
    fn given_the_credential_manager_should_round_trip_and_delete_a_test_secret() {
        struct Cleanup(String, &'static str);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = CredentialManager.delete(&self.0, self.1);
            }
        }
        let service = format!("PurePoint.Test.{}", uuid::Uuid::new_v4());
        let account = "ws://127.0.0.1:65000/v1";
        let _cleanup = Cleanup(service.clone(), account);
        let store = CredentialManager;
        assert_eq!(store.get(&service, account).unwrap(), None);
        store.set(&service, account, SECRET).unwrap();
        store.set(&service, account, &SECRET.repeat(2)).unwrap();
        assert_eq!(
            store.get(&service, account).unwrap(),
            Some(SECRET.repeat(2))
        );
        store.delete(&service, account).unwrap();
        assert_eq!(store.get(&service, account).unwrap(), None);
        store.delete(&service, account).unwrap();
    }
}
