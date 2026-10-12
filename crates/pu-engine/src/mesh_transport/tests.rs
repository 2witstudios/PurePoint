#[tokio::test]
async fn given_enrolled_peers_should_authenticate_both_over_loopback() {
    // given
    let a = super::Identity::generate().unwrap();
    let b = super::Identity::generate().unwrap();
    let a_id = a.peer_id();
    let b_id = b.peer_id();
    let server = super::MeshTransport::new(
        &a,
        super::EnrolledPeers::new([b.certificate_der()]).unwrap(),
    )
    .unwrap();
    let client = super::MeshTransport::new(
        &b,
        super::EnrolledPeers::new([a.certificate_der()]).unwrap(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    // when
    let (left, right) = tokio::join!(
        async {
            let (io, _) = listener.accept().await.unwrap();
            server.accept(io, super::Role::Control).await.unwrap()
        },
        async {
            let io = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            client
                .connect(io, super::Role::Control, a_id)
                .await
                .unwrap()
        }
    );
    // then
    assert_eq!(left.peer_id(), b_id);
    assert_eq!(right.peer_id(), a_id);
    assert_eq!(left.protocol(), rustls::ProtocolVersion::TLSv1_3);
}
#[test]
fn given_restored_identity_should_preserve_fingerprint() {
    // given
    let identity = super::Identity::generate().unwrap();
    // when
    let restored =
        super::Identity::restore(identity.certificate_der(), identity.private_key_der()).unwrap();
    // then
    assert_eq!(identity.peer_id(), restored.peer_id());
}

#[tokio::test]
async fn given_authenticated_stream_should_roundtrip_bounded_frames() {
    // given
    let (mut a, mut b) = pair(Role::Bulk).await;
    // when
    a.write_frame(b"payload", Duration::from_secs(1))
        .await
        .unwrap();
    let frame = b.read_frame(Duration::from_secs(1)).await.unwrap();
    // then
    assert_eq!(frame, b"payload");
}

use super::*;
use tokio::net::{TcpListener, TcpStream};
async fn pair(
    role: Role,
) -> (
    AuthenticatedConnection<TcpStream>,
    AuthenticatedConnection<TcpStream>,
) {
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let server =
        MeshTransport::new(&a, EnrolledPeers::new([b.certificate_der()]).unwrap()).unwrap();
    let client =
        MeshTransport::new(&b, EnrolledPeers::new([a.certificate_der()]).unwrap()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (a, b) = tokio::join!(
        async {
            server
                .accept(listener.accept().await.unwrap().0, role)
                .await
                .unwrap()
        },
        async {
            client
                .connect(
                    TcpStream::connect(listener.local_addr().unwrap())
                        .await
                        .unwrap(),
                    role,
                    a.peer_id(),
                )
                .await
                .unwrap()
        }
    );
    (a, b)
}

#[test]
fn given_corrupt_or_mismatched_secret_should_fail_closed() {
    // given
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    // when / then
    for (cert, key) in [
        (a.certificate_der(), b.private_key_der()),
        (a.certificate_der(), &[0, 1, 2][..]),
        (&[0, 1, 2][..], a.private_key_der()),
        (&[][..], &[][..]),
    ] {
        assert!(matches!(Identity::restore(cert, key), Err(Error::Material)));
    }
    assert!(matches!(
        EnrolledPeers::new([&[0, 1, 2][..]]),
        Err(Error::Material)
    ));
    assert!(matches!(EnrolledPeers::new([]), Err(Error::Material)));
}

async fn attempt(
    server: &MeshTransport,
    client: &MeshTransport,
    server_role: Role,
    client_role: Role,
    pin: PeerId,
) -> (
    Result<AuthenticatedConnection<TcpStream>, Error>,
    Result<AuthenticatedConnection<TcpStream>, Error>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    tokio::join!(
        async {
            server
                .accept(listener.accept().await.unwrap().0, server_role)
                .await
        },
        async {
            client
                .connect(
                    TcpStream::connect(listener.local_addr().unwrap())
                        .await
                        .unwrap(),
                    client_role,
                    pin,
                )
                .await
        }
    )
}

#[tokio::test]
async fn given_unknown_client_should_reject_even_with_same_san() {
    // given
    let server_id = Identity::generate().unwrap();
    let enrolled = Identity::generate().unwrap();
    let unknown = Identity::generate().unwrap();
    let server = MeshTransport::new(
        &server_id,
        EnrolledPeers::new([enrolled.certificate_der()]).unwrap(),
    )
    .unwrap();
    let client = MeshTransport::new(
        &unknown,
        EnrolledPeers::new([server_id.certificate_der()]).unwrap(),
    )
    .unwrap();
    // when
    let (received, _) = attempt(
        &server,
        &client,
        Role::Control,
        Role::Control,
        server_id.peer_id(),
    )
    .await;
    // then
    assert!(received.is_err());
}

#[tokio::test]
async fn given_wrong_server_pin_should_reject_another_enrolled_server() {
    // given
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let c = Identity::generate().unwrap();
    let server =
        MeshTransport::new(&a, EnrolledPeers::new([b.certificate_der()]).unwrap()).unwrap();
    let client = MeshTransport::new(
        &b,
        EnrolledPeers::new([a.certificate_der(), c.certificate_der()]).unwrap(),
    )
    .unwrap();
    // when
    let (_, received) = attempt(&server, &client, Role::Control, Role::Control, c.peer_id()).await;
    // then
    assert!(matches!(received, Err(Error::Pin)));
}

#[tokio::test]
async fn given_different_control_bulk_roles_should_reject_handshake() {
    // given
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let server =
        MeshTransport::new(&a, EnrolledPeers::new([b.certificate_der()]).unwrap()).unwrap();
    let client =
        MeshTransport::new(&b, EnrolledPeers::new([a.certificate_der()]).unwrap()).unwrap();
    // when
    let (left, right) = attempt(&server, &client, Role::Control, Role::Bulk, a.peer_id()).await;
    // then
    assert!(left.is_err());
    assert!(right.is_err());
}

#[tokio::test]
async fn given_self_reported_identity_should_not_change_certificate_principal() {
    // given
    let (mut a, mut b) = pair(Role::Control).await;
    let actual = b.peer_id();
    let impostor = Identity::generate().unwrap().peer_id();
    // when
    a.write_frame(impostor.fingerprint(), Duration::from_secs(1))
        .await
        .unwrap();
    let payload = b.read_frame(Duration::from_secs(1)).await.unwrap();
    // then
    assert_eq!(payload, impostor.fingerprint());
    assert_eq!(b.peer_id(), actual);
    assert_ne!(b.peer_id(), impostor);
}

#[tokio::test]
async fn given_bad_frame_headers_should_reject_before_body_and_poison() {
    // given
    for role in [Role::Control, Role::Bulk] {
        for length in [0, (role.max_frame() + 1) as u32, u32::MAX] {
            let (mut a, mut b) = pair(role).await;
            // when
            a.stream.write_u32(length).await.unwrap();
            a.stream.flush().await.unwrap();
            // then
            assert!(matches!(
                b.read_frame(Duration::from_secs(1)).await,
                Err(Error::FrameLength)
            ));
            assert!(matches!(
                b.read_frame(Duration::from_secs(1)).await,
                Err(Error::Poisoned)
            ));
            assert!(matches!(
                b.write_frame(b"x", Duration::from_secs(1)).await,
                Err(Error::Poisoned)
            ));
        }
    }
}

#[tokio::test]
async fn given_truncated_frame_should_fail_and_poison() {
    // given
    for bytes in [&[0, 0][..], &[0, 0, 0, 3, 1][..]] {
        let (mut a, mut b) = pair(Role::Control).await;
        // when
        a.stream.write_all(bytes).await.unwrap();
        a.stream.shutdown().await.unwrap();
        // then
        assert!(matches!(
            b.read_frame(Duration::from_secs(1)).await,
            Err(Error::Io(_))
        ));
        assert!(matches!(
            b.read_frame(Duration::from_secs(1)).await,
            Err(Error::Poisoned)
        ));
    }
}

#[tokio::test]
async fn given_partial_frame_timeout_or_cancellation_should_never_resume() {
    // given
    for cancel in [false, true] {
        let (mut a, mut b) = pair(Role::Control).await;
        a.stream.write_all(&[0, 0]).await.unwrap();
        a.stream.flush().await.unwrap();
        // when
        if cancel {
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(10),
                    b.read_frame(Duration::from_secs(1))
                )
                .await
                .is_err()
            );
        } else {
            assert!(matches!(
                b.read_frame(Duration::from_millis(10)).await,
                Err(Error::Timeout)
            ));
        }
        // then
        assert!(matches!(
            b.read_frame(Duration::from_secs(1)).await,
            Err(Error::Poisoned)
        ));
        assert!(matches!(
            b.write_frame(b"x", Duration::from_secs(1)).await,
            Err(Error::Poisoned)
        ));
    }
}

#[tokio::test]
async fn given_role_frame_limits_should_accept_boundary_and_reject_invalid_writes() {
    // given
    for role in [Role::Control, Role::Bulk] {
        let (mut a, mut b) = pair(role).await;
        let payload = vec![7; role.max_frame()];
        // when
        let (sent, received) = tokio::join!(
            a.write_frame(&payload, Duration::from_secs(1)),
            b.read_frame(Duration::from_secs(1))
        );
        // then
        sent.unwrap();
        assert_eq!(received.unwrap(), payload);
        for payload in [vec![], vec![7; role.max_frame() + 1]] {
            let (mut a, _) = pair(role).await;
            assert!(matches!(
                a.write_frame(&payload, Duration::from_secs(1)).await,
                Err(Error::FrameLength)
            ));
            assert!(matches!(
                a.write_frame(b"x", Duration::from_secs(1)).await,
                Err(Error::Poisoned)
            ));
        }
    }
}

fn transport(identity: &Identity, peer: &Identity) -> MeshTransport {
    MeshTransport::new(
        identity,
        EnrolledPeers::new([peer.certificate_der()]).unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn given_missing_client_certificate_should_reject() {
    // given
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let server = transport(&a, &b);
    let mut roots = RootCertStore::empty();
    roots.add(a.cert.clone()).unwrap();
    let mut config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
    config.alpn_protocols = vec![Role::Control.alpn().to_vec()];
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    // when
    let (received, _) = tokio::join!(
        async {
            server
                .accept(listener.accept().await.unwrap().0, Role::Control)
                .await
        },
        async {
            let io = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            TlsConnector::from(Arc::new(config))
                .connect(ServerName::try_from("mesh.invalid").unwrap(), io)
                .await
        }
    );
    // then
    assert!(received.is_err());
}

#[tokio::test]
async fn given_tls12_only_client_should_reject() {
    // given
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let server = transport(&a, &b);
    let mut roots = RootCertStore::empty();
    roots.add(a.cert.clone()).unwrap();
    let mut config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS12])
            .unwrap()
            .with_root_certificates(roots)
            .with_client_auth_cert(vec![b.cert.clone()], b.key.clone_key())
            .unwrap();
    config.alpn_protocols = vec![Role::Control.alpn().to_vec()];
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    // when
    let (received, sent) = tokio::join!(
        async {
            server
                .accept(listener.accept().await.unwrap().0, Role::Control)
                .await
        },
        async {
            TlsConnector::from(Arc::new(config))
                .connect(
                    ServerName::try_from("mesh.invalid").unwrap(),
                    TcpStream::connect(listener.local_addr().unwrap())
                        .await
                        .unwrap(),
                )
                .await
        }
    );
    // then
    assert!(received.is_err());
    assert!(sent.is_err());
}

fn certificate_with_dates(expired: bool) -> Identity {
    let key = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::new(vec!["mesh.invalid".into()]).unwrap();
    if expired {
        params.not_before = rcgen::date_time_ymd(2000, 1, 1);
        params.not_after = rcgen::date_time_ymd(2001, 1, 1);
    } else {
        params.not_before = rcgen::date_time_ymd(4090, 1, 1);
        params.not_after = rcgen::date_time_ymd(4091, 1, 1);
    }
    let cert = params.self_signed(&key).unwrap();
    Identity::restore(cert.der(), &key.serialize_der()).unwrap()
}

#[tokio::test]
async fn given_expired_or_not_yet_valid_certificates_should_reject_both_directions() {
    // given
    for expired in [true, false] {
        let valid = Identity::generate().unwrap();
        let invalid = certificate_with_dates(expired);
        // when / then
        let (left, _) = attempt(
            &transport(&valid, &invalid),
            &transport(&invalid, &valid),
            Role::Control,
            Role::Control,
            valid.peer_id(),
        )
        .await;
        assert!(left.is_err());
        let (_, right) = attempt(
            &transport(&invalid, &valid),
            &transport(&valid, &invalid),
            Role::Control,
            Role::Control,
            invalid.peer_id(),
        )
        .await;
        assert!(right.is_err());
    }
}

#[tokio::test]
async fn given_same_key_reissued_certificate_should_require_explicit_new_enrollment() {
    // given: normal CA validation trusts the same public key, but pin differs.
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let key = rcgen::KeyPair::from_pkcs8_der_and_sign_algo(
        &PrivatePkcs8KeyDer::from(b.private_key_der().to_vec()),
        &rcgen::PKCS_ECDSA_P256_SHA256,
    )
    .unwrap();
    let mut params = rcgen::CertificateParams::new(vec!["mesh.invalid".into()]).unwrap();
    params.serial_number = Some(42u64.into());
    let cert = params.self_signed(&key).unwrap();
    let reissued = Identity::restore(cert.der(), &key.serialize_der()).unwrap();
    // when
    let (received, _) = attempt(
        &transport(&a, &b),
        &transport(&reissued, &a),
        Role::Control,
        Role::Control,
        a.peer_id(),
    )
    .await;
    // then: specifically post-verification pin failure, not a chain failure.
    assert!(matches!(received, Err(Error::Pin)));
    let (_, received) = attempt(
        &transport(&reissued, &a),
        &transport(&a, &b),
        Role::Control,
        Role::Control,
        b.peer_id(),
    )
    .await;
    assert!(matches!(received, Err(Error::Pin)));
}

#[tokio::test]
async fn given_stalled_handshake_should_expire_on_both_sides() {
    // given
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let server = transport(&a, &b);
    let client = transport(&b, &a);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client_socket = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let server_socket = listener.accept().await.unwrap().0;
    let listener2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client_socket2 = TcpStream::connect(listener2.local_addr().unwrap())
        .await
        .unwrap();
    let server_socket2 = listener2.accept().await.unwrap().0;
    // when
    let (left, right) = tokio::join!(
        server.accept(server_socket, Role::Control),
        client.connect(client_socket2, Role::Control, a.peer_id())
    );
    // then
    assert!(matches!(left, Err(Error::Timeout)));
    assert!(matches!(right, Err(Error::Timeout)));
    drop((client_socket, server_socket2));
}

#[tokio::test]
async fn given_shorter_server_deadline_should_cut_idle_handshake_early() {
    // given: a peer that connects but never sends a TLS ClientHello
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let server = transport(&a, &b);
    let (io, _idle_peer) = tokio::io::duplex(4096);
    let started = std::time::Instant::now();
    // when
    let result = server
        .accept_within(io, Role::Control, Duration::from_millis(100))
        .await;
    // then
    assert!(matches!(result, Err(Error::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn given_server_deadline_above_overall_bound_should_clamp_to_handshake_deadline() {
    // given
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let server = transport(&a, &b);
    let (io, _idle_peer) = tokio::io::duplex(4096);
    // when: a caller-supplied longer deadline cannot extend the overall bound
    let result = tokio::time::timeout(
        HANDSHAKE_DEADLINE + Duration::from_secs(2),
        server.accept_within(io, Role::Control, Duration::from_secs(60)),
    )
    .await;
    // then
    assert!(matches!(result, Ok(Err(Error::Timeout))));
}

#[tokio::test]
async fn given_repeat_connections_should_never_resume_or_offer_early_data() {
    // given
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let server = transport(&a, &b);
    let client = transport(&b, &a);
    assert_eq!(server.server.max_early_data_size, 0);
    assert_eq!(server.server.send_tls13_tickets, 0);
    assert!(!server.server.ticketer.enabled());
    assert!(!server.server.session_storage.can_cache());
    assert!(!client.client.enable_early_data);
    // when / then: reuse configuration/session cache across real handshakes.
    for _ in 0..3 {
        let (left, right) = attempt(&server, &client, Role::Bulk, Role::Bulk, a.peer_id()).await;
        let mut left = left.unwrap();
        let mut right = right.unwrap();
        left.write_frame(b"fresh", Duration::from_secs(1))
            .await
            .unwrap();
        right.read_frame(Duration::from_secs(1)).await.unwrap();
        assert_eq!(
            left.stream.get_ref().1.handshake_kind(),
            Some(rustls::HandshakeKind::Full)
        );
        assert_eq!(
            right.stream.get_ref().1.handshake_kind(),
            Some(rustls::HandshakeKind::Full)
        );
        if let TlsStream::Client(stream) = &mut right.stream {
            assert!(stream.get_mut().1.early_data().is_none());
        }
    }
}

/// Deterministic TCP backpressure after a partial encrypted record; TLS itself
/// still runs over a real loopback connection without a mocked verifier.
struct GatedTcp {
    io: TcpStream,
    allowance: Arc<std::sync::atomic::AtomicUsize>,
}
impl AsyncRead for GatedTcp {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_read(cx, buf)
    }
}
impl AsyncWrite for GatedTcp {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        use std::{sync::atomic::Ordering, task::Poll};
        let allowance = self.allowance.load(Ordering::Relaxed);
        if allowance == 0 {
            return Poll::Pending;
        }
        let result =
            std::pin::Pin::new(&mut self.io).poll_write(cx, &bytes[..bytes.len().min(allowance)]);
        if let Poll::Ready(Ok(written)) = result {
            self.allowance.fetch_sub(written, Ordering::Relaxed);
        }
        result
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

#[tokio::test]
async fn given_partial_write_timeout_or_cancellation_should_poison_both_directions() {
    // given
    for cancel in [true, false] {
        let a = Identity::generate().unwrap();
        let b = Identity::generate().unwrap();
        let server = transport(&a, &b);
        let client = transport(&b, &a);
        let allowance = Arc::new(std::sync::atomic::AtomicUsize::new(usize::MAX));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (left, right) = tokio::join!(
            async {
                server
                    .accept(listener.accept().await.unwrap().0, Role::Bulk)
                    .await
                    .unwrap()
            },
            async {
                let io = GatedTcp {
                    io: TcpStream::connect(listener.local_addr().unwrap())
                        .await
                        .unwrap(),
                    allowance: allowance.clone(),
                };
                client.connect(io, Role::Bulk, a.peer_id()).await.unwrap()
            }
        );
        let mut right = right;
        allowance.store(7, std::sync::atomic::Ordering::Relaxed);
        // when
        if cancel {
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(10),
                    right.write_frame(b"payload", Duration::from_secs(1))
                )
                .await
                .is_err()
            );
        } else {
            assert!(matches!(
                right
                    .write_frame(b"payload", Duration::from_millis(10))
                    .await,
                Err(Error::Timeout)
            ));
        }
        // then
        assert_eq!(allowance.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert!(matches!(
            right.write_frame(b"next", Duration::from_secs(1)).await,
            Err(Error::Poisoned)
        ));
        assert!(matches!(
            right.read_frame(Duration::from_secs(1)).await,
            Err(Error::Poisoned)
        ));
        drop(left);
    }
}

#[tokio::test]
async fn given_peer_without_alpn_should_not_expose_application_stream() {
    // given
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let server = transport(&a, &b);
    let client = transport(&b, &a);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    // when
    let (received, _) = tokio::join!(
        async {
            server
                .accept(listener.accept().await.unwrap().0, Role::Control)
                .await
        },
        async {
            TlsConnector::from(Arc::new(client.client.clone()))
                .connect(
                    ServerName::try_from("mesh.invalid").unwrap(),
                    TcpStream::connect(listener.local_addr().unwrap())
                        .await
                        .unwrap(),
                )
                .await
        }
    );
    // then
    assert!(matches!(received, Err(Error::Protocol)));
}

#[tokio::test]
async fn given_tls12_only_server_should_never_downgrade() {
    // given
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let client = transport(&b, &a);
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS12])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![a.cert.clone()], a.key.clone_key())
            .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    // when
    let (received, sent) = tokio::join!(
        async {
            TlsAcceptor::from(Arc::new(config))
                .accept(listener.accept().await.unwrap().0)
                .await
        },
        async {
            client
                .connect(
                    TcpStream::connect(listener.local_addr().unwrap())
                        .await
                        .unwrap(),
                    Role::Control,
                    a.peer_id(),
                )
                .await
        }
    );
    // then
    assert!(received.is_err());
    assert!(sent.is_err());
}

#[tokio::test]
async fn given_restored_identity_and_changed_address_should_authenticate_same_peer() {
    // given
    let a = Identity::generate().unwrap();
    let b = Identity::generate().unwrap();
    let restored = Identity::restore(a.certificate_der(), a.private_key_der()).unwrap();
    let client = transport(&b, &a);
    // when: attempt creates a new independently allocated TCP endpoint each time.
    for identity in [&a, &restored] {
        let (left, right) = attempt(
            &transport(identity, &b),
            &client,
            Role::Control,
            Role::Control,
            a.peer_id(),
        )
        .await;
        // then
        assert_eq!(left.unwrap().peer_id(), b.peer_id());
        assert_eq!(right.unwrap().peer_id(), a.peer_id());
    }
}

#[test]
fn given_enrolled_public_certificate_should_derive_pin_without_private_key() {
    // given
    let identity = Identity::generate().unwrap();
    // when
    let pin = PeerId::from_certificate_der(identity.certificate_der()).unwrap();
    // then
    assert_eq!(pin, identity.peer_id());
    assert!(matches!(
        PeerId::from_certificate_der(&[0, 1, 2]),
        Err(Error::Material)
    ));
}
