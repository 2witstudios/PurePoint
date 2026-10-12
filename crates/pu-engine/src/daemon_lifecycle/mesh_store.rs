//! Private receiver identity and canonical peer policy storage. Initialization is
//! explicit; missing/corrupt existing identity never generates replacement keys.
//!
//! Mutations require a local administrative capability and `&mut self`. The engine
//! must place this store behind its authority barrier: exclusive for enrollment,
//! policy changes/revocation, shared through dispatch and bounded output sends.
//! All ledger administration must use that same barrier. Files are policy data,
//! not live authorization; every use compares their canonical digest to the ledger.
//!
//! Identity is immutable. Policy writes precede their durable ledger anchor. A
//! failed/torn policy write or failed anchor denies access, including after restart;
//! a local administrator can explicitly reapply policy. No automatic rollback,
//! regrant, certificate rotation, execution, listener or source provisioning occurs.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Seek, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Arc;

use pu_core::mesh::{Action, ExportGrant, PeerPolicy, valid_identifier};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::DaemonLock;
use crate::execution_state::{Caller, Ledger, LocalAdmin, PayloadDigest, PeerRecord};
use crate::mesh_transport::{AuthenticatedConnection, Identity, PeerId};

const MAX_IDENTITY_BYTES: usize = 32 * 1024;
const MAX_POLICY_BYTES: usize = 512 * 1024;
const MAX_CERTIFICATE_BYTES: usize = 16 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("private mesh state I/O failed")]
    Io(#[from] std::io::Error),
    #[error("invalid or damaged mesh state; explicit local recovery required")]
    Invalid,
    #[error("mesh identity already exists; refusing replacement")]
    AlreadyInitialized,
    #[error("mesh transport peer limit exceeded")]
    Capacity,
    #[error("mesh authority or policy anchor does not match")]
    Denied,
    #[error("execution ledger rejected mesh authority")]
    Ledger(#[from] crate::execution_state::Error),
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityRecord {
    schema: u32,
    daemon_id: String,
    certificate_der: Vec<u8>,
    private_key_der: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyRecord {
    schema: u32,
    owner_daemon: String,
    certificate_der: Vec<u8>,
    expires_at: Option<i64>,
    exports: Vec<ExportGrant>,
}

/// Intentionally no Debug/serialization: identity contains private key material.
/// The retained singleton also prevents a successor from changing trust while
/// an accepted operation still owns this store.
pub struct MeshStore {
    lock: Arc<DaemonLock>,
    daemon_id: String,
    identity: Identity,
}

/// Stable ledger principal derived exclusively from an exact certificate pin.
pub fn peer_name(peer: PeerId) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut name = String::with_capacity(64);
    for byte in peer.fingerprint() {
        name.push(HEX[(byte >> 4) as usize] as char);
        name.push(HEX[(byte & 15) as usize] as char);
    }
    name
}

/// Local public metadata only, never later-effect authorization or key material.
pub type AdminPeer = (PeerRecord, PeerPolicy, Vec<u8>);

/// Parse a locally selected authoritative ledger fingerprint name. This is only
/// an expected pin/notification selector, not possession, authentication or Caller
/// authority; it cannot construct an AuthenticatedConnection.
pub fn peer_id_from_name(value: &str) -> Result<PeerId> {
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    if value.len() != 64 {
        return Err(Error::Invalid);
    }
    let mut bytes = [0; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = (nibble(pair[0]).ok_or(Error::Invalid)? << 4)
            | nibble(pair[1]).ok_or(Error::Invalid)?;
    }
    Ok(PeerId::from_fingerprint(bytes))
}

fn name(lock: &DaemonLock, suffix: &str) -> OsString {
    let mut name = lock.socket_name.clone();
    name.push(suffix);
    name
}

fn read(lock: &DaemonLock, name: &OsStr, limit: usize) -> Result<Vec<u8>> {
    lock.verify_lock()?;
    let file = lock.directory.open_file(name, false)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(Error::Invalid);
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Error::Invalid);
    }
    lock.verify_lock()?;
    Ok(bytes)
}

fn persist(lock: &DaemonLock, name: &OsStr, bytes: &[u8]) -> Result<()> {
    lock.verify_lock()?;
    let mut file = lock.directory.open_file(name, true)?;
    file.set_len(0)?;
    file.rewind()?;
    file.write_all(bytes)?;
    file.sync_all()?;
    lock.directory.sync()?;
    lock.verify_lock()?;
    Ok(())
}

impl MeshStore {
    /// Only explicit local setup creates an identity. Even an empty file left by
    /// interrupted initialization is preserved and requires deliberate recovery.
    pub fn initialize(lock: Arc<DaemonLock>, _admin: &LocalAdmin) -> Result<Self> {
        lock.verify_lock()?;
        let path = name(&lock, ".mesh-identity.json");
        if lock.directory.entry(&path)?.is_some() {
            return Err(Error::AlreadyInitialized);
        }
        let identity = Identity::generate().map_err(|_| Error::Invalid)?;
        let record = IdentityRecord {
            schema: 1,
            daemon_id: pu_core::id::session_id(),
            certificate_der: identity.certificate_der().to_vec(),
            private_key_der: identity.private_key_der().to_vec(),
        };
        let bytes = serde_json::to_vec(&record).map_err(|_| Error::Invalid)?;
        if bytes.len() > MAX_IDENTITY_BYTES {
            return Err(Error::Invalid);
        }
        // Even two explicit initialization requests sharing this process's flock
        // must not overwrite one another. The validated private ancestor chain
        // excludes other users; kernel O_EXCL chooses exactly one creator.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(lock.directory.checked_path_for(&path)?)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    Error::AlreadyInitialized
                } else {
                    Error::Io(error)
                }
            })?;
        pu_core::private_fs::Entry::from_file(&file)?.private_file()?;
        lock.directory
            .require_identity(&path, pu_core::private_fs::Entry::from_file(&file)?)?
            .private_file()?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        lock.directory.sync()?;
        lock.verify_lock()?;
        Ok(Self {
            lock,
            daemon_id: record.daemon_id,
            identity,
        })
    }

    pub fn open(lock: Arc<DaemonLock>) -> Result<Self> {
        let bytes = read(
            &lock,
            &name(&lock, ".mesh-identity.json"),
            MAX_IDENTITY_BYTES,
        )?;
        let record: IdentityRecord = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
        if record.schema != 1 || !valid_identifier(&record.daemon_id) {
            return Err(Error::Invalid);
        }
        let identity = Identity::restore(&record.certificate_der, &record.private_key_der)
            .map_err(|_| Error::Invalid)?;
        Ok(Self {
            lock,
            daemon_id: record.daemon_id,
            identity,
        })
    }

    pub fn daemon_id(&self) -> &str {
        &self.daemon_id
    }
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    fn require_owner(&self, ledger: &Ledger) -> Result<()> {
        self.lock.verify_lock()?;
        if ledger.owner_daemon() != self.daemon_id {
            return Err(Error::Denied);
        }
        Ok(())
    }

    fn policy_name(&self, peer: PeerId) -> OsString {
        name(&self.lock, &format!(".mesh-peer-{}.json", peer_name(peer)))
    }

    fn validate_policy(&self, peer: PeerId, policy: &PolicyRecord) -> Result<PeerPolicy> {
        if policy.schema != 1
            || policy.owner_daemon != self.daemon_id
            || policy.certificate_der.len() > MAX_CERTIFICATE_BYTES
            || PeerId::from_certificate_der(&policy.certificate_der).map_err(|_| Error::Invalid)?
                != peer
        {
            return Err(Error::Invalid);
        }
        let snapshot = PeerPolicy {
            peer_id: peer_name(peer),
            generation: 1,
            revoked: false,
            expires_at: policy.expires_at,
            exports: policy.exports.clone(),
        };
        snapshot.validate().map_err(|_| Error::Invalid)?;
        Ok(snapshot)
    }

    fn read_policy(&self, peer: PeerId) -> Result<PolicyRecord> {
        let bytes = read(&self.lock, &self.policy_name(peer), MAX_POLICY_BYTES)?;
        let policy = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
        self.validate_policy(peer, &policy)?;
        Ok(policy)
    }

    fn policy_bytes(&self, peer: PeerId, policy: &PolicyRecord) -> Result<Vec<u8>> {
        self.validate_policy(peer, policy)?;
        let bytes = serde_json::to_vec(policy).map_err(|_| Error::Invalid)?;
        if bytes.len() > MAX_POLICY_BYTES {
            return Err(Error::Invalid);
        }
        Ok(bytes)
    }

    fn persist_policy(&self, peer: PeerId, policy: &PolicyRecord) -> Result<PayloadDigest> {
        let bytes = self.policy_bytes(peer, policy)?;
        persist(&self.lock, &self.policy_name(peer), &bytes)?;
        Ok(PayloadDigest(Sha256::digest(&bytes).into()))
    }

    /// Certificate bytes are supplied by explicit local enrollment, never by a
    /// remote request. Registration alone has no anchor and therefore denies all.
    /// Failure after registration requires explicit update_policy recovery.
    pub fn enroll(
        &mut self,
        ledger: &Ledger,
        admin: &LocalAdmin,
        certificate: &[u8],
        exports: Vec<ExportGrant>,
        expires_at: Option<i64>,
    ) -> Result<PeerRecord> {
        self.require_owner(ledger)?;
        if certificate.len() > MAX_CERTIFICATE_BYTES {
            return Err(Error::Invalid);
        }
        let peer = PeerId::from_certificate_der(certificate).map_err(|_| Error::Invalid)?;
        if peer == self.identity.peer_id() {
            return Err(Error::Invalid);
        }
        let policy = PolicyRecord {
            schema: 1,
            owner_daemon: self.daemon_id.clone(),
            certificate_der: certificate.to_vec(),
            expires_at,
            exports,
        };
        self.policy_bytes(peer, &policy)?;
        let registered = ledger.register_peer(admin, &peer_name(peer))?;
        let digest = self.persist_policy(peer, &policy)?;
        Ok(ledger.update_peer_authorization(
            admin,
            &registered.id,
            registered.generation,
            digest,
        )?)
    }

    /// Policy bytes are durable before the anchor commits. An intervening failure
    /// denies authorization; this operation never silently regrants a revoked peer.
    pub fn update_policy(
        &mut self,
        ledger: &Ledger,
        admin: &LocalAdmin,
        certificate: &[u8],
        expected_generation: u64,
        exports: Vec<ExportGrant>,
        expires_at: Option<i64>,
    ) -> Result<PeerRecord> {
        self.require_owner(ledger)?;
        if certificate.len() > MAX_CERTIFICATE_BYTES {
            return Err(Error::Invalid);
        }
        let peer = PeerId::from_certificate_der(certificate).map_err(|_| Error::Invalid)?;
        let current = ledger.peer(&peer_name(peer))?;
        if current.generation != expected_generation {
            return Err(Error::Denied);
        }
        // Reapplying the exact public enrollment material also permits explicit
        // repair after a torn policy write. It cannot substitute another peer.
        let policy = PolicyRecord {
            schema: 1,
            owner_daemon: self.daemon_id.clone(),
            certificate_der: certificate.to_vec(),
            expires_at,
            exports,
        };
        let digest = self.persist_policy(peer, &policy)?;
        Ok(ledger.update_peer_authorization(admin, &current.id, expected_generation, digest)?)
    }

    fn current_policy(
        &self,
        ledger: &Ledger,
        peer: PeerId,
        generation: u64,
        now: i64,
    ) -> Result<PeerPolicy> {
        self.require_owner(ledger)?;
        let now = ledger.observe_time(now)?;
        let current = ledger.peer(&peer_name(peer))?;
        if current.revoked || current.generation != generation {
            return Err(Error::Denied);
        }
        let record = self.read_policy(peer)?;
        let bytes = self.policy_bytes(peer, &record)?;
        if current.authorization_digest != Some(PayloadDigest(Sha256::digest(&bytes).into())) {
            return Err(Error::Denied);
        }
        let mut policy = self.validate_policy(peer, &record)?;
        policy.generation = current.generation;
        policy.revoked = current.revoked;
        policy
            .visible_exports(&current.id, generation, now)
            .map_err(|_| Error::Denied)?;
        Ok(policy)
    }

    /// Owner-local display: even revoked/expired metadata must match canonical
    /// policy bytes and exact enrolled certificate in the durable ledger anchor.
    /// Call under the session authority barrier. This grants no remote authority.
    pub fn admin_peer(
        &self,
        ledger: &Ledger,
        _admin: &LocalAdmin,
        peer: &str,
    ) -> Result<AdminPeer> {
        self.require_owner(ledger)?;
        let pin = peer_id_from_name(peer)?;
        let current = ledger.peer(peer)?;
        if current.id != peer || current.generation == 0 || current.generation > i64::MAX as u64 {
            return Err(Error::Invalid);
        }
        let record = self.read_policy(pin)?;
        let bytes = self.policy_bytes(pin, &record)?;
        if current.authorization_digest != Some(PayloadDigest(Sha256::digest(&bytes).into())) {
            return Err(Error::Denied);
        }
        let mut policy = self.validate_policy(pin, &record)?;
        policy.generation = current.generation;
        policy.revoked = current.revoked;
        policy.validate().map_err(|_| Error::Invalid)?;
        Ok((current, policy, record.certificate_der))
    }

    /// Bounded authoritative LEDGER enumeration, never filesystem-name discovery.
    /// Every returned record is checked; missing/unanchored/corrupt state denies.
    pub fn list_admin_peers(
        &self,
        ledger: &Ledger,
        admin: &LocalAdmin,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AdminPeer>> {
        self.require_owner(ledger)?;
        if let Some(after) = after {
            peer_id_from_name(after)?;
        }
        let records = ledger.peers_page(after, limit)?;
        records
            .into_iter()
            .map(|expected| {
                let checked = self.admin_peer(ledger, admin, &expected.id)?;
                if checked.0 != expected {
                    return Err(Error::Denied);
                }
                Ok(checked)
            })
            .collect()
    }

    /// TLS setup input only. Validate ALL records, including revoked/expired
    /// metadata, but include only current active exact pins. Never export keys.
    /// A caller must retain shared authority through constructing its transport.
    pub fn active_peer_certificates(
        &self,
        ledger: &Ledger,
        admin: &LocalAdmin,
        now: i64,
    ) -> Result<Vec<Vec<u8>>> {
        self.require_owner(ledger)?;
        let records = ledger.peers_page(None, 256)?;
        if let Some(last) = records.last()
            && records.len() == 256
            && !ledger.peers_page(Some(&last.id), 1)?.is_empty()
        {
            return Err(Error::Capacity);
        }
        let now = ledger.observe_time(now)?;
        let mut seen = std::collections::HashSet::new();
        let mut certificates = Vec::new();
        for expected in records {
            if !seen.insert(expected.id.clone()) {
                return Err(Error::Invalid);
            }
            let (current, policy, certificate) = self.admin_peer(ledger, admin, &expected.id)?;
            if current != expected {
                return Err(Error::Denied);
            }
            if policy.revoked || policy.expires_at.is_some_and(|expiry| now >= expiry) {
                continue;
            }
            policy
                .visible_exports(&current.id, current.generation, now)
                .map_err(|_| Error::Denied)?;
            certificates.push(certificate);
        }
        Ok(certificates)
    }

    /// Connection establishment/inventory check. The session authority must retain
    /// its shared barrier through use; this snapshot never authorizes later effects.
    pub(crate) fn connection_policy<S>(
        &self,
        ledger: &Ledger,
        connection: &AuthenticatedConnection<S>,
        generation: u64,
        now: i64,
    ) -> Result<PeerPolicy> {
        self.current_policy(ledger, connection.peer_id(), generation, now)
    }

    /// Authenticated, generation-independent discovery only. Session authority
    /// retains its barrier through the bounded response; this policy is no token.
    pub(crate) fn discover_connection_policy<S>(
        &self,
        ledger: &Ledger,
        connection: &AuthenticatedConnection<S>,
        now: i64,
    ) -> Result<PeerPolicy> {
        self.require_owner(ledger)?;
        let peer = connection.peer_id();
        let generation = ledger.peer(&peer_name(peer))?.generation;
        self.current_policy(ledger, peer, generation, now)
    }

    /// Authenticates from the verified TLS connection, checks durable grant bytes,
    /// then constructs a current Caller. Caller is usable only while the engine
    /// retains its shared authority barrier through the actual operation effect.
    pub fn authorize_connection<S>(
        &self,
        ledger: &Ledger,
        connection: &AuthenticatedConnection<S>,
        generation: u64,
        export: &str,
        action: &Action<'_>,
        now: i64,
    ) -> Result<Caller> {
        let peer = connection.peer_id();
        let policy = self.current_policy(ledger, peer, generation, now)?;
        policy
            .authorize(
                &peer_name(peer),
                generation,
                export,
                action,
                ledger.observe_time(now)?,
            )
            .map_err(|_| Error::Denied)?;
        let caller = Caller::authenticated_peer(peer_name(peer), generation)?;
        ledger.validate_principal(&caller)?;
        Ok(caller)
    }
}

#[cfg(test)]
mod tests;
