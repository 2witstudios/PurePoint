//! Opt-in TLS transport only. No listeners, execution, persistence or grants.
//! A verified certificate proves possession, not current operation authority.
//! Callers must fence each dispatch/send with their live revocation barrier.
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_rustls::{TlsAcceptor, TlsConnector, TlsStream};

/// Errors never contain private-key material or plaintext fallback instructions.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid identity or enrollment material")]
    Material,
    #[error("TLS configuration rejected")]
    Configuration,
    #[error("TLS or stream I/O failed")]
    Io(#[from] std::io::Error),
    #[error("transport deadline elapsed")]
    Timeout,
    #[error("peer certificate is not the exact enrolled pin")]
    Pin,
    #[error("unexpected TLS protocol or connection role")]
    Protocol,
    #[error("invalid frame length")]
    FrameLength,
    #[error("framing state is poisoned")]
    Poisoned,
}

/// SHA-256 of the complete authenticated certificate DER, never a supplied label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PeerId([u8; 32]);
impl PeerId {
    /// Derive an expected pin from a locally enrolled public certificate. This
    /// parses material only; it neither authenticates possession nor grants access.
    pub fn from_certificate_der(cert: &[u8]) -> Result<Self, Error> {
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(cert.to_vec()))
            .map_err(|_| Error::Material)?;
        Ok(Self::of(cert))
    }
    pub fn fingerprint(&self) -> &[u8; 32] {
        &self.0
    }
    fn of(cert: &[u8]) -> Self {
        Self(Sha256::digest(cert).into())
    }
}

/// Stable identity. Intentionally no Debug/serialization; export secrets explicitly.
pub struct Identity {
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
}
impl Identity {
    pub fn generate() -> Result<Self, Error> {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["mesh.invalid".into()])
                .map_err(|_| Error::Material)?;
        Self::restore(cert.der(), &signing_key.serialize_der())
    }
    /// Restore exactly these bytes; failure never generates a replacement identity.
    pub fn restore(cert: &[u8], key: &[u8]) -> Result<Self, Error> {
        let cert = CertificateDer::from(cert.to_vec());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.to_vec()));
        let certified = rustls::sign::CertifiedKey::from_der(
            vec![cert.clone()],
            key.clone_key(),
            &rustls::crypto::ring::default_provider(),
        )
        .map_err(|_| Error::Material)?;
        certified.keys_match().map_err(|_| Error::Material)?;
        Ok(Self { cert, key })
    }
    pub fn certificate_der(&self) -> &[u8] {
        self.cert.as_ref()
    }
    /// The parent must persist this only within its approved private directory.
    pub fn private_key_der(&self) -> &[u8] {
        self.key.secret_der()
    }
    pub fn peer_id(&self) -> PeerId {
        PeerId::of(self.certificate_der())
    }
}

/// Receiver-admin supplied exact certificate pins; no platform root store.
pub struct EnrolledPeers {
    roots: RootCertStore,
    certs: Vec<CertificateDer<'static>>,
}
impl EnrolledPeers {
    pub fn new<'a>(certificates: impl IntoIterator<Item = &'a [u8]>) -> Result<Self, Error> {
        let mut roots = RootCertStore::empty();
        let mut certs = Vec::new();
        for bytes in certificates {
            let cert = CertificateDer::from(bytes.to_vec());
            roots.add(cert.clone()).map_err(|_| Error::Material)?;
            certs.push(cert);
        }
        if certs.is_empty() {
            return Err(Error::Material);
        }
        Ok(Self { roots, certs })
    }
}

pub const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(5);
pub const MAX_CONTROL_FRAME: usize = 64 * 1024;
pub const MAX_BULK_FRAME: usize = 256 * 1024;

/// ALPN is authenticated by TLS. Incarnation/generation and cross-connection
/// binding still belong to the parent's authenticated bridge handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Control,
    Bulk,
}
impl Role {
    fn alpn(self) -> &'static [u8] {
        match self {
            Self::Control => b"pu-mesh-control/1",
            Self::Bulk => b"pu-mesh-bulk/1",
        }
    }
    fn max_frame(self) -> usize {
        match self {
            Self::Control => MAX_CONTROL_FRAME,
            Self::Bulk => MAX_BULK_FRAME,
        }
    }
}

/// Explicitly constructed transport; never opens a listener or loads OS trust.
pub struct MeshTransport {
    client: ClientConfig,
    server: ServerConfig,
    peers: EnrolledPeers,
}
impl MeshTransport {
    pub fn new(identity: &Identity, peers: EnrolledPeers) -> Result<Self, Error> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(peers.roots.clone()),
            provider.clone(),
        )
        .build()
        .map_err(|_| Error::Configuration)?;
        let mut server = ServerConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| Error::Configuration)?
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![identity.cert.clone()], identity.key.clone_key())
            .map_err(|_| Error::Material)?;
        server.max_early_data_size = 0;
        server.send_tls13_tickets = 0;
        server.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        let mut client = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| Error::Configuration)?
            .with_root_certificates(peers.roots.clone())
            .with_client_auth_cert(vec![identity.cert.clone()], identity.key.clone_key())
            .map_err(|_| Error::Material)?;
        client.enable_early_data = false;
        client.resumption = rustls::client::Resumption::disabled();
        Ok(Self {
            client,
            server,
            peers,
        })
    }
    pub async fn accept<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        io: S,
        role: Role,
    ) -> Result<AuthenticatedConnection<S>, Error> {
        let mut config = self.server.clone();
        config.alpn_protocols = vec![role.alpn().to_vec()];
        let stream = tokio::time::timeout(
            HANDSHAKE_DEADLINE,
            TlsAcceptor::from(Arc::new(config)).accept(io),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        self.authenticate(TlsStream::Server(stream), role, None)
    }
    pub async fn connect<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        io: S,
        role: Role,
        expected_server: PeerId,
    ) -> Result<AuthenticatedConnection<S>, Error> {
        // Reject unknown requested pins before transmitting even the TLS handshake.
        if !self
            .peers
            .certs
            .iter()
            .any(|c| PeerId::of(c) == expected_server)
        {
            return Err(Error::Pin);
        }
        let mut config = self.client.clone();
        config.alpn_protocols = vec![role.alpn().to_vec()];
        let name = ServerName::try_from("mesh.invalid").map_err(|_| Error::Configuration)?;
        let stream = tokio::time::timeout(
            HANDSHAKE_DEADLINE,
            TlsConnector::from(Arc::new(config)).connect(name, io),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        self.authenticate(TlsStream::Client(stream), role, Some(expected_server))
    }
    fn authenticate<S>(
        &self,
        stream: TlsStream<S>,
        role: Role,
        expected: Option<PeerId>,
    ) -> Result<AuthenticatedConnection<S>, Error> {
        let (_, session) = stream.get_ref();
        let cert = session
            .peer_certificates()
            .and_then(|c| c.first())
            .ok_or(Error::Pin)?;
        // Chain validity alone is insufficient: a CA or another cert with the
        // same key cannot substitute for the receiver's exact enrollment.
        if !self.peers.certs.iter().any(|pin| pin == cert) {
            return Err(Error::Pin);
        }
        let peer = PeerId::of(cert);
        if expected.is_some_and(|pin| pin != peer) {
            return Err(Error::Pin);
        }
        if session.protocol_version() != Some(rustls::ProtocolVersion::TLSv1_3)
            || session.alpn_protocol() != Some(role.alpn())
        {
            return Err(Error::Protocol);
        }
        Ok(AuthenticatedConnection {
            stream,
            peer,
            role,
            poisoned: false,
        })
    }
}

/// No access to the raw stream or caller-supplied principal. This identity is
/// transport evidence only and must never replace live per-operation authority.
pub struct AuthenticatedConnection<S> {
    stream: TlsStream<S>,
    peer: PeerId,
    role: Role,
    poisoned: bool,
}
impl<S> AuthenticatedConnection<S> {
    pub fn peer_id(&self) -> PeerId {
        self.peer
    }
    pub fn role(&self) -> Role {
        self.role
    }
    pub fn protocol(&self) -> rustls::ProtocolVersion {
        rustls::ProtocolVersion::TLSv1_3
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AuthenticatedConnection<S> {
    /// Four-byte network-order length followed by opaque payload. An invalid
    /// header is rejected before allocation. Deadline covers the whole frame.
    /// Errors or cancellation permanently poison BOTH directions; drop to close.
    pub async fn read_frame(&mut self, deadline: Duration) -> Result<Vec<u8>, Error> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        // Set before the first await: dropping the future must not reset framing.
        self.poisoned = true;
        let result = tokio::time::timeout(deadline, async {
            let length = self.stream.read_u32().await? as usize;
            if length == 0 || length > self.role.max_frame() {
                return Err(Error::FrameLength);
            }
            let mut payload = vec![0; length];
            self.stream.read_exact(&mut payload).await?;
            Ok(payload)
        })
        .await
        .map_err(|_| Error::Timeout)?;
        if result.is_ok() {
            self.poisoned = false;
        }
        result
    }
    /// Successful write means transport flush, not remote operation acceptance.
    /// Callers must hold their authority barrier through this send. Cancellation
    /// or any error permanently poisons framing; never retry on this connection.
    pub async fn write_frame(&mut self, payload: &[u8], deadline: Duration) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        self.poisoned = true;
        if payload.is_empty() || payload.len() > self.role.max_frame() {
            return Err(Error::FrameLength);
        }
        let result = tokio::time::timeout(deadline, async {
            self.stream.write_u32(payload.len() as u32).await?;
            self.stream.write_all(payload).await?;
            self.stream.flush().await?;
            Ok(())
        })
        .await
        .map_err(|_| Error::Timeout)?;
        if result.is_ok() {
            self.poisoned = false;
        }
        result
    }
}

#[cfg(test)]
mod tests;
