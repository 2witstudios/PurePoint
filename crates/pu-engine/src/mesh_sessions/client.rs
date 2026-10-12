//! Typed client handshake/requests. No bearer token can replace independent mTLS.
use super::*;
use serde::de::DeserializeOwned;

pub struct ControlClient<S> {
    connection: AuthenticatedConnection<S>,
    welcome: Welcome,
    limits: SessionLimits,
}
/// Bulk observation client established from a live control welcome over its own
/// authenticated Role::Bulk connection. Server still checks live authority.
pub struct BulkClient<S> {
    connection: AuthenticatedConnection<S>,
    limits: SessionLimits,
}
impl<S: AsyncRead + AsyncWrite + Unpin> ControlClient<S> {
    /// Discover only on an independent pinned Control-role TLS stream. The
    /// stream closes after its bounded reply. Use NEW streams for strict Control
    /// and Bulk; this generation is not a session or replay permission.
    pub async fn discover(
        transport: &MeshTransport,
        io: S,
        receiver: PeerId,
        daemon: String,
        limits: SessionLimits,
    ) -> Result<GenerationDiscovery> {
        limits.validate()?;
        let mut connection = transport.connect(io, Role::Control, receiver).await?;
        let hello = Hello::Discover {
            version: wire::BRIDGE_VERSION,
            target_daemon: daemon.clone(),
        };
        let bytes = serde_json::to_vec(&hello).map_err(|_| Error::Operation)?;
        Hello::decode(&bytes)?;
        connection.write_frame(&bytes, limits.send_deadline).await?;
        let response =
            GenerationDiscovery::decode(&connection.read_frame(limits.read_deadline).await?)?;
        if response.daemon_id != daemon {
            return Err(Error::Denied);
        }
        Ok(response)
    }
    pub async fn connect(
        transport: &MeshTransport,
        io: S,
        receiver: PeerId,
        daemon: String,
        generation: u64,
        limits: SessionLimits,
    ) -> Result<Self> {
        limits.validate()?;
        let mut connection = transport.connect(io, Role::Control, receiver).await?;
        let hello = Hello::Control {
            version: wire::BRIDGE_VERSION,
            target_daemon: daemon.clone(),
            generation,
        };
        let bytes = serde_json::to_vec(&hello).map_err(|_| Error::Operation)?;
        Hello::decode(&bytes)?;
        connection.write_frame(&bytes, limits.send_deadline).await?;
        let welcome = Welcome::decode(&connection.read_frame(limits.read_deadline).await?)?;
        if welcome.daemon_id != daemon || welcome.generation != generation {
            return Err(Error::Denied);
        }
        Ok(Self {
            connection,
            welcome,
            limits,
        })
    }
    pub fn welcome(&self) -> &Welcome {
        &self.welcome
    }
    pub async fn request<T: DeserializeOwned>(&mut self, request: &Request) -> Result<T> {
        request.validate()?;
        if request.target_daemon != self.welcome.daemon_id {
            return Err(Error::Denied);
        }
        let bytes = serde_json::to_vec(request).map_err(|_| Error::Operation)?;
        if bytes.len() > self.limits.control_frame {
            return Err(Error::Frame);
        }
        self.connection
            .write_frame(&bytes, self.limits.send_deadline)
            .await?;
        let bytes = self
            .connection
            .read_frame(self.limits.read_deadline)
            .await?;
        if bytes.len() > self.limits.control_frame {
            return Err(Error::Frame);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Operation)
    }
    pub async fn connect_bulk<B: AsyncRead + AsyncWrite + Unpin>(
        &self,
        transport: &MeshTransport,
        io: B,
    ) -> Result<BulkClient<B>> {
        let mut connection = transport
            .connect(io, Role::Bulk, self.connection.peer_id())
            .await?;
        let hello = Hello::Bulk {
            version: wire::BRIDGE_VERSION,
            target_daemon: self.welcome.daemon_id.clone(),
            generation: self.welcome.generation,
            incarnation: self.welcome.incarnation.clone(),
            control_binding: self.welcome.control_binding.clone(),
        };
        connection
            .write_frame(
                &serde_json::to_vec(&hello).map_err(|_| Error::Operation)?,
                self.limits.send_deadline,
            )
            .await?;
        let welcome = Welcome::decode(&connection.read_frame(self.limits.read_deadline).await?)?;
        if welcome != self.welcome {
            return Err(Error::Denied);
        }
        Ok(BulkClient {
            connection,
            limits: self.limits.clone(),
        })
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> BulkClient<S> {
    pub async fn read<T: DeserializeOwned>(&mut self) -> Result<T> {
        let bytes = self
            .connection
            .read_frame(self.limits.read_deadline)
            .await?;
        if bytes.len() > self.limits.bulk_frame {
            return Err(Error::Frame);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Operation)
    }
}
