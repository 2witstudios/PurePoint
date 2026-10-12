//! Explicit paired LAN endpoints. No listener is created by constructing an
//! Engine or SessionAuthority; production startup consumes local configuration.

use crate::{
    daemon_lifecycle::mesh_exports::MeshExports,
    daemon_lifecycle::owned_work::WorkOwner,
    execution_state::{Caller, Ledger},
    mesh_sessions::{self, Accepted, SessionAuthority},
    mesh_transport::{MeshTransport, Role},
};
use pu_core::mesh::admin::ListenConfig;
use pu_core::mesh::{PeerPolicy, wire::Request};
use std::{io, net::SocketAddr, sync::Arc};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, watch},
    task::JoinSet,
};

pub struct BoundListeners {
    control: TcpListener,
    bulk: TcpListener,
}

impl BoundListeners {
    /// Bind both explicit interfaces before exposing either to the service loop.
    /// A failed second bind drops the first; disabled configuration binds nothing.
    pub async fn bind(config: &ListenConfig) -> io::Result<Option<Self>> {
        if !config.validate() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid mesh listener addresses",
            ));
        }
        let (Some(control), Some(bulk)) = (config.bind_address, config.bulk_bind_address) else {
            return Ok(None);
        };
        let control = TcpListener::bind(control).await?;
        let bulk = TcpListener::bind(bulk).await?;
        Ok(Some(Self { control, bulk }))
    }

    pub fn addresses(&self) -> io::Result<(SocketAddr, SocketAddr)> {
        Ok((self.control.local_addr()?, self.bulk.local_addr()?))
    }

    /// Bound TCP tasks independently before spawning them, then let the session
    /// authority enforce its own global/per-peer limits across listener reloads.
    /// Stopping closes transports; accepted owned effects retain their barriers
    /// until actual completion. This does not stop local IPC or worker execution.
    pub async fn run<F>(
        self,
        authority: Arc<SessionAuthority>,
        transport: Arc<MeshTransport>,
        work: Arc<WorkOwner>,
        mut stop: watch::Receiver<bool>,
        dispatch: F,
    ) -> io::Result<()>
    where
        F: Fn(
                &Ledger,
                &MeshExports,
                &Caller,
                &PeerPolicy,
                &Request,
            ) -> mesh_sessions::Result<Vec<u8>>
            + Send
            + Sync
            + 'static,
    {
        let control = Arc::new(Semaphore::new(authority.limits().control_total));
        let bulk = Arc::new(Semaphore::new(authority.limits().bulk_total));
        let dispatch = Arc::new(dispatch);
        let mut connections = JoinSet::new();
        let outcome = loop {
            if *stop.borrow() || work.is_stopping() {
                break Ok(());
            }
            let incoming = tokio::select! {
                biased;
                _=stop.changed()=>break Ok(()),
                _=work.stopping()=>break Ok(()),
                _=connections.join_next(),if !connections.is_empty()=>continue,
                incoming=async {
                    tokio::select! {
                        stream=self.control.accept()=>stream.map(|(io,_)|(io,Role::Control)),
                        stream=self.bulk.accept()=>stream.map(|(io,_)|(io,Role::Bulk)),
                    }
                }=>incoming,
            };
            let (stream, role) = match incoming {
                Ok(value) => value,
                Err(error) => break Err(error),
            };
            let pool = match role {
                Role::Control => &control,
                Role::Bulk => &bulk,
            };
            let Ok(admission) = pool.clone().try_acquire_owned() else {
                continue;
            };
            if stream.set_nodelay(true).is_err() {
                continue;
            }
            let authority = authority.clone();
            let transport = transport.clone();
            let work = work.clone();
            let dispatch = dispatch.clone();
            connections.spawn(async move {
                let _admission = admission;
                let mut session = match role {
                    Role::Control => match authority.accept_incoming(&transport, stream).await {
                        Ok(Accepted::Discovery) => return,
                        Ok(Accepted::Session(session)) => session,
                        Err(_) => return,
                    },
                    Role::Bulk => match authority.accept(&transport, stream, Role::Bulk).await {
                        Ok(session) => session,
                        Err(_) => return,
                    },
                };
                if role == Role::Bulk {
                    // Bulk subscriptions are wired by the observation adapter.
                    // This read-only metadata service accepts no bulk requests.
                    let _ = session.wait_bulk_close().await;
                    return;
                }
                loop {
                    let effect = dispatch.clone();
                    if session
                        .dispatch(&work, move |ledger, exports, caller, policy, request| {
                            effect(ledger, exports, caller, policy, request)
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        };
        connections.shutdown().await;
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        daemon_lifecycle::{mesh_store::MeshStore, owned_work::WorkOwner, try_acquire_daemon_lock},
        execution_state::{Caller, Ledger, Limits, LocalAdmin},
        mesh_sessions::{ControlClient, SessionAuthority, SessionLimits},
        mesh_transport::{EnrolledPeers, Identity, MeshTransport},
    };
    use pu_core::mesh::admin::ListenConfig;
    use pu_core::mesh::{Capability, ExportGrant, wire};
    use std::{collections::BTreeSet, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn given_explicit_listener_should_discover_serve_bound_control_bulk_and_stop_without_stopping_local_work()
     {
        let tmp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let lock = Arc::new(
            try_acquire_daemon_lock(&tmp.path().join("daemon.sock"))
                .unwrap()
                .unwrap(),
        );
        let work = WorkOwner::new(&lock);
        let admin =
            LocalAdmin::authorized_by_local_policy(&Caller::authenticated_local("admin").unwrap())
                .unwrap();
        let mut store = MeshStore::initialize(lock.clone(), &admin).unwrap();
        let exports =
            crate::daemon_lifecycle::mesh_exports::MeshExports::open(lock, store.daemon_id(), 8)
                .unwrap();
        let ledger = Ledger::open(
            tmp.path().join("ledger.sqlite"),
            store.daemon_id(),
            Limits::default(),
        )
        .unwrap();
        let peer = Identity::generate().unwrap();
        store
            .enroll(
                &ledger,
                &admin,
                peer.certificate_der(),
                vec![ExportGrant {
                    export_id: "export".into(),
                    capabilities: BTreeSet::from([Capability::Inspect, Capability::ReadOutput]),
                    runtimes: BTreeSet::new(),
                }],
                None,
            )
            .unwrap();
        let pin = store.identity().peer_id();
        let client_transport = MeshTransport::new(
            &peer,
            EnrolledPeers::new([store.identity().certificate_der()]).unwrap(),
        )
        .unwrap();
        let transport = Arc::new(
            MeshTransport::new(
                store.identity(),
                EnrolledPeers::new([peer.certificate_der()]).unwrap(),
            )
            .unwrap(),
        );
        let limits = SessionLimits {
            control_total: 1,
            control_per_peer: 1,
            bulk_total: 1,
            bulk_per_peer: 1,
            ..SessionLimits::default()
        };
        let authority = SessionAuthority::new(store, ledger, exports, limits.clone()).unwrap();
        let pair = BoundListeners {
            control: TcpListener::bind("127.0.0.1:0").await.unwrap(),
            bulk: TcpListener::bind("127.0.0.1:0").await.unwrap(),
        };
        let (control, bulk) = pair.addresses().unwrap();
        let (stop, rx) = tokio::sync::watch::channel(false);
        let serve = tokio::spawn(pair.run(
            authority.clone(),
            transport,
            work.clone(),
            rx,
            |_, _, _, _, request| {
                Ok(pu_core::mesh::response::Response {
                    request_id: request.request_id.clone(),
                    daemon_id: request.target_daemon.clone(),
                    body: pu_core::mesh::response::Body::Inventory { exports: vec![] },
                }
                .encode()?)
            },
        ));
        let request = wire::Request {
            request_id: "query".into(),
            target_daemon: authority.daemon_id().into(),
            operation: wire::Operation::Inventory {},
        };
        let response = crate::mesh_host::HostReader::new(2)
            .unwrap()
            .read(&client_transport, pin, control, &request)
            .await
            .unwrap();
        assert!(
            matches!(response.body,pu_core::mesh::response::Body::Inventory {exports} if exports.is_empty())
        );
        let discovery = ControlClient::discover(
            &client_transport,
            tokio::net::TcpStream::connect(control).await.unwrap(),
            pin,
            authority.daemon_id().into(),
            limits.clone(),
        )
        .await
        .unwrap();
        let mut client = ControlClient::connect(
            &client_transport,
            tokio::net::TcpStream::connect(control).await.unwrap(),
            pin,
            authority.daemon_id().into(),
            discovery.generation,
            limits.clone(),
        )
        .await
        .unwrap();
        assert!(
            matches!(client.request::<pu_core::mesh::response::Response>(&request).await.unwrap().body,
            pu_core::mesh::response::Body::Inventory {exports} if exports.is_empty())
        );
        // Independent bulk remains admissible while the only control slot is held.
        let _bulk = client
            .connect_bulk(
                &client_transport,
                tokio::net::TcpStream::connect(bulk).await.unwrap(),
            )
            .await
            .unwrap();
        let denied = tokio::time::timeout(
            Duration::from_secs(2),
            ControlClient::discover(
                &client_transport,
                tokio::net::TcpStream::connect(control).await.unwrap(),
                pin,
                authority.daemon_id().into(),
                limits,
            ),
        )
        .await
        .unwrap();
        assert!(denied.is_err());
        stop.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(2), serve)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(client.request::<serde_json::Value>(&request).await.is_err());
        assert!(tokio::net::TcpStream::connect(control).await.is_err());
        assert!(tokio::net::TcpStream::connect(bulk).await.is_err());
        assert!(!work.is_stopping());
        work.spawn(async {}).unwrap().await.unwrap();
    }

    #[tokio::test]
    async fn given_disabled_or_partial_bind_should_leave_no_remote_endpoint() {
        assert!(
            BoundListeners::bind(&ListenConfig::default())
                .await
                .unwrap()
                .is_none()
        );
        let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let control = reserved.local_addr().unwrap();
        let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bulk = occupied.local_addr().unwrap();
        drop(reserved);
        let config = ListenConfig {
            bind_address: Some(control),
            bulk_bind_address: Some(bulk),
        };
        assert!(BoundListeners::bind(&config).await.is_err());
        let freed = TcpListener::bind(control).await.unwrap();
        drop(freed);
        drop(occupied);
        let listeners = BoundListeners::bind(&config).await.unwrap().unwrap();
        assert_eq!(listeners.addresses().unwrap(), (control, bulk));
    }
}
