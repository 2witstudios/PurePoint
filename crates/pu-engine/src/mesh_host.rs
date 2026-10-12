//! Bounded host-side one-shot reads. Addresses are routing hints; the configured
//! exact certificate and daemon identity must both match. No automatic retries.

use crate::{
    mesh_sessions::{self, ControlClient, SessionLimits},
    mesh_transport::{self, MeshTransport, PeerId},
};
use pu_core::mesh::{
    response::Response,
    wire::{Operation, Request, WireError},
};
use std::{net::SocketAddr, time::Duration};
use tokio::{net::TcpStream, sync::Semaphore, time::timeout};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("host read capacity exhausted")]
    Capacity,
    #[error("invalid explicit mesh target or request")]
    Invalid,
    #[error("operation is not available through the read-only route")]
    Unsupported,
    #[error("selected worker is unreachable")]
    Offline,
    #[error("selected worker did not respond before the deadline")]
    Timeout,
    #[error("selected worker authentication or authorization failed")]
    Denied,
    #[error("selected worker response is incompatible")]
    Incompatible,
    #[error("selected worker closed the session")]
    Closed,
}

/// One shared instance bounds aggregate concurrent reads across all destinations.
/// No queue, persistent subscriptions or output buffering is allocated here.
pub struct HostReader {
    admission: Semaphore,
}
impl HostReader {
    pub fn new(max_concurrent: usize) -> Result<Self, Error> {
        if !(1..=64).contains(&max_concurrent) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            admission: Semaphore::new(max_concurrent),
        })
    }

    pub async fn read(
        &self,
        transport: &MeshTransport,
        receiver: PeerId,
        address: SocketAddr,
        request: &Request,
    ) -> Result<Response, Error> {
        if !matches!(
            request.operation,
            Operation::Inventory {}
                | Operation::Status { .. }
                | Operation::Receipt { .. }
                | Operation::InputReceipt { .. }
        ) {
            return Err(Error::Unsupported);
        }
        request.validate().map_err(|_| Error::Invalid)?;
        let ip = match address.ip() {
            std::net::IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or(address.ip(), Into::into),
            ip => ip,
        };
        if address.port() == 0
            || ip.is_unspecified()
            || ip.is_multicast()
            || matches!(ip, std::net::IpAddr::V4(ip) if ip.is_broadcast())
        {
            return Err(Error::Invalid);
        }
        let _admission = self.admission.try_acquire().map_err(|_| Error::Capacity)?;
        // One overall deadline also bounds the sequence of two TLS handshakes.
        timeout(Duration::from_secs(20), async {
            let generation = ControlClient::discover(
                transport,
                connect(address).await?,
                receiver,
                request.target_daemon.clone(),
                SessionLimits::default(),
            )
            .await
            .map_err(session_error)?;
            let mut client = ControlClient::connect(
                transport,
                connect(address).await?,
                receiver,
                request.target_daemon.clone(),
                generation.generation,
                SessionLimits::default(),
            )
            .await
            .map_err(session_error)?;
            let response: Response = client.request(request).await.map_err(session_error)?;
            // The generic session client validates framing; this adapter checks
            // exact receiver, request, kind and resource correlation as well.
            Response::decode(
                &response.encode().map_err(|_| Error::Incompatible)?,
                request,
            )
            .map_err(|_| Error::Incompatible)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
}
async fn connect(address: SocketAddr) -> Result<TcpStream, Error> {
    let stream = timeout(Duration::from_secs(5), TcpStream::connect(address))
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Offline)?;
    stream.set_nodelay(true).map_err(|_| Error::Offline)?;
    Ok(stream)
}
fn session_error(error: mesh_sessions::Error) -> Error {
    match error {
        mesh_sessions::Error::Denied => Error::Denied,
        mesh_sessions::Error::Capacity => Error::Capacity,
        mesh_sessions::Error::Closed => Error::Closed,
        mesh_sessions::Error::Transport(mesh_transport::Error::Timeout) => Error::Timeout,
        mesh_sessions::Error::Wire(WireError::Version) => Error::Incompatible,
        mesh_sessions::Error::Transport(_) => Error::Denied,
        _ => Error::Incompatible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_transport::{EnrolledPeers, Identity};
    use pu_core::mesh::wire::{Operation, Request};
    use std::sync::Arc;

    fn transport_pair() -> (MeshTransport, MeshTransport, PeerId) {
        let client = Identity::generate().unwrap();
        let server = Identity::generate().unwrap();
        let pin = server.peer_id();
        (
            MeshTransport::new(
                &client,
                EnrolledPeers::new([server.certificate_der()]).unwrap(),
            )
            .unwrap(),
            MeshTransport::new(
                &server,
                EnrolledPeers::new([client.certificate_der()]).unwrap(),
            )
            .unwrap(),
            pin,
        )
    }

    fn inventory() -> Request {
        Request {
            request_id: "read".into(),
            target_daemon: "worker".into(),
            operation: Operation::Inventory {},
        }
    }

    #[tokio::test]
    async fn given_stalled_peer_should_bound_host_fanout_and_release_on_cancellation() {
        let (transport, _, pin) = transport_pair();
        let transport = Arc::new(transport);
        let host = Arc::new(HostReader::new(1).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (h, t) = (host.clone(), transport.clone());
        let pending = tokio::spawn(async move { h.read(&t, pin, address, &inventory()).await });
        let (_stalled, _) = listener.accept().await.unwrap();
        assert!(matches!(
            host.read(&transport, pin, address, &inventory()).await,
            Err(Error::Capacity)
        ));
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        let (h, t) = (host.clone(), transport.clone());
        let next = tokio::spawn(async move { h.read(&t, pin, address, &inventory()).await });
        let (_next, _) = timeout(Duration::from_secs(1), listener.accept())
            .await
            .unwrap()
            .unwrap();
        next.abort();
        assert!(next.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn given_authenticated_mismatched_reply_should_refuse_without_replaying_request() {
        use pu_core::mesh::{response::Body, wire};
        let (transport, server, pin) = transport_pair();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let serve = tokio::spawn(async move {
            let (io, _) = listener.accept().await.unwrap();
            let mut discovery = server
                .accept(io, mesh_transport::Role::Control)
                .await
                .unwrap();
            assert!(matches!(
                wire::Hello::decode(&discovery.read_frame(Duration::from_secs(1)).await.unwrap())
                    .unwrap(),
                wire::Hello::Discover { .. }
            ));
            discovery
                .write_frame(
                    &serde_json::to_vec(&wire::GenerationDiscovery {
                        version: wire::BRIDGE_VERSION,
                        daemon_id: "worker".into(),
                        generation: 2,
                    })
                    .unwrap(),
                    Duration::from_secs(1),
                )
                .await
                .unwrap();
            drop(discovery);
            let (io, _) = listener.accept().await.unwrap();
            let mut control = server
                .accept(io, mesh_transport::Role::Control)
                .await
                .unwrap();
            assert!(matches!(
                wire::Hello::decode(&control.read_frame(Duration::from_secs(1)).await.unwrap())
                    .unwrap(),
                wire::Hello::Control { generation: 2, .. }
            ));
            control
                .write_frame(
                    &serde_json::to_vec(&wire::Welcome {
                        version: wire::BRIDGE_VERSION,
                        daemon_id: "worker".into(),
                        generation: 2,
                        incarnation: "incarnation".into(),
                        control_binding: "binding".into(),
                    })
                    .unwrap(),
                    Duration::from_secs(1),
                )
                .await
                .unwrap();
            let request =
                Request::decode(&control.read_frame(Duration::from_secs(1)).await.unwrap())
                    .unwrap();
            assert_eq!(request, inventory());
            let reply = Response {
                request_id: "someone-else".into(),
                daemon_id: "worker".into(),
                body: Body::Inventory { exports: vec![] },
            };
            control
                .write_frame(&reply.encode().unwrap(), Duration::from_secs(1))
                .await
                .unwrap();
            assert!(
                timeout(Duration::from_millis(30), listener.accept())
                    .await
                    .is_err()
            );
        });
        assert!(matches!(
            HostReader::new(1)
                .unwrap()
                .read(&transport, pin, address, &inventory())
                .await,
            Err(Error::Incompatible)
        ));
        serve.await.unwrap();
    }
    #[tokio::test]
    async fn given_mutation_should_refuse_before_contacting_the_selected_machine() {
        let identity = Identity::generate().unwrap();
        let receiver = Identity::generate().unwrap();
        let transport = MeshTransport::new(
            &identity,
            EnrolledPeers::new([receiver.certificate_der()]).unwrap(),
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let request = Request {
            request_id: "operation".into(),
            target_daemon: "worker".into(),
            operation: Operation::IssueWindow { ttl_seconds: 60 },
        };
        assert!(matches!(
            HostReader::new(2)
                .unwrap()
                .read(
                    &transport,
                    receiver.peer_id(),
                    listener.local_addr().unwrap(),
                    &request
                )
                .await,
            Err(Error::Unsupported)
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), listener.accept())
                .await
                .is_err()
        );
    }
}
