#![allow(unused_crate_dependencies)] // false positives because there is both a library and a binary
#![allow(clippy::unwrap_used, reason = "unwrap is fine in tests")]

use core::time::Duration;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use ironrdp::connector;
use ironrdp::dvc::DrdynvcClient;
use ironrdp::echo::client::EchoClient;
use ironrdp::pdu::rdp::capability_sets::MajorPlatformType;
use ironrdp::pdu::{self, gcc};
use ironrdp::server::{
    self, DesktopSize, DisplayUpdate, KeyboardEvent, MouseEvent, PixelFormat, RdpServer, RdpServerDisplay,
    RdpServerDisplayUpdates, RdpServerInputHandler, ServerEvent, TlsIdentityCtx,
};
use ironrdp::session::image::DecodedImage;
use ironrdp::session::{self, ActiveStage, ActiveStageOutput};
use ironrdp_async::{Framed, FramedWrite as _};
use ironrdp_testsuite_extra as _;
use ironrdp_tls::TlsStream;
use ironrdp_tokio::TokioStream;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::{Mutex, oneshot};
use tracing::debug;

const DESKTOP_WIDTH: u16 = 1024;
const DESKTOP_HEIGHT: u16 = 768;
const USERNAME: &str = "";
const PASSWORD: &str = "";

#[tokio::test]
async fn test_client_server() {
    client_server(default_client_config(), |stage, framed, _display_tx| async {
        (stage, framed)
    })
    .await
}

#[tokio::test]
async fn test_deactivation_reactivation() {
    let client_config = default_client_config();
    let mut image = DecodedImage::new(
        PixelFormat::RgbA32,
        client_config.desktop_size.width,
        client_config.desktop_size.height,
    );
    client_server(client_config, |mut stage, mut framed, display_tx| async move {
        display_tx
            .send(DisplayUpdate::Resize(DesktopSize {
                width: 2048,
                height: 2048,
            }))
            .unwrap();
        {
            let (action, payload) = framed.read_pdu().await.expect("valid PDU");
            let outputs = stage.process(&mut image, action, &payload).expect("stage process");
            let out = outputs.into_iter().next().unwrap();
            match out {
                ActiveStageOutput::DeactivateAll(mut connection_activation) => {
                    // TODO: factor this out in common client code
                    // Execute the Deactivation-Reactivation Sequence:
                    // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/dfc234ce-481a-4674-9a5d-2a7bafb14432
                    debug!("Received Server Deactivate All PDU, executing Deactivation-Reactivation Sequence");
                    let mut buf = pdu::WriteBuf::new();
                    'activation_seq: loop {
                        let written = ironrdp_async::single_sequence_step_read(
                            &mut framed,
                            &mut *connection_activation,
                            &mut buf,
                        )
                        .await
                        .map_err(|e| session::custom_err!("read deactivation-reactivation sequence step", e))
                        .unwrap();

                        if written.size().is_some() {
                            framed
                                .write_all(buf.filled())
                                .await
                                .map_err(|e| session::custom_err!("write deactivation-reactivation sequence step", e))
                                .unwrap();
                        }

                        if let connector::connection_activation::ConnectionActivationState::Finalized {
                            io_channel_id,
                            user_channel_id,
                            desktop_size,
                            share_id,
                            enable_server_pointer,
                            pointer_software_rendering,
                        } = connection_activation.connection_activation_state()
                        {
                            debug!(?desktop_size, "Deactivation-Reactivation Sequence completed");
                            // Update image size with the new desktop size.
                            // image = DecodedImage::new(PixelFormat::RgbA32, desktop_size.width, desktop_size.height);
                            // Update the active stage with the new channel IDs and pointer settings
                            // while preserving any negotiated decompressor state.
                            stage.reactivate_fastpath_processor(
                                io_channel_id,
                                user_channel_id,
                                share_id,
                                enable_server_pointer,
                                pointer_software_rendering,
                            );
                            stage.set_share_id(share_id);
                            stage.set_enable_server_pointer(enable_server_pointer);
                            break 'activation_seq;
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
        (stage, framed)
    })
    .await
}

#[tokio::test]
async fn test_echo_virtual_channel_end_to_end() {
    let payload = b"ironrdp echo e2e".to_vec();
    let echo_payload = payload.clone();

    client_server_with_connector(
        default_client_config(),
        |connector| connector.with_static_channel(DrdynvcClient::new().with_dynamic_channel(EchoClient::new())),
        move |mut stage, mut framed, display_tx, echo_handle| async move {
            let _display_tx = display_tx;
            let mut image = DecodedImage::new(PixelFormat::RgbA32, DESKTOP_WIDTH, DESKTOP_HEIGHT);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut matched_measurement = None;

            while Instant::now() < deadline {
                echo_handle
                    .send_request(echo_payload.clone())
                    .expect("send echo request");

                for _ in 0..20 {
                    let measurements = echo_handle.take_measurements();
                    if let Some(measurement) = measurements.into_iter().find(|m| m.payload == echo_payload) {
                        matched_measurement = Some(measurement);
                        break;
                    }

                    let read_result = tokio::time::timeout(Duration::from_millis(150), framed.read_pdu()).await;
                    let Ok(Ok((action, frame))) = read_result else {
                        continue;
                    };

                    let outputs = stage.process(&mut image, action, &frame).expect("stage process");
                    for output in outputs {
                        if let ActiveStageOutput::ResponseFrame(frame) = output {
                            framed.write_all(&frame).await.expect("write response frame");
                        }
                    }
                }

                if matched_measurement.is_some() {
                    break;
                }
            }

            let measurement = matched_measurement.expect("echo RTT measurement was not produced");
            assert_eq!(measurement.payload, echo_payload);

            (stage, framed)
        },
    )
    .await
}

type DisplayUpdatesRx = Arc<Mutex<UnboundedReceiver<DisplayUpdate>>>;

struct TestDisplayUpdates {
    rx: DisplayUpdatesRx,
}

#[async_trait::async_trait]
impl RdpServerDisplayUpdates for TestDisplayUpdates {
    async fn next_update(&mut self) -> Result<Option<DisplayUpdate>> {
        let mut rx = self.rx.lock().await;

        Ok(rx.recv().await)
    }
}

struct TestDisplay {
    rx: DisplayUpdatesRx,
}

#[async_trait::async_trait]
impl RdpServerDisplay for TestDisplay {
    async fn size(&mut self) -> DesktopSize {
        DesktopSize {
            width: DESKTOP_WIDTH,
            height: DESKTOP_HEIGHT,
        }
    }

    async fn updates(&mut self) -> Result<Box<dyn RdpServerDisplayUpdates>> {
        Ok(Box::new(TestDisplayUpdates {
            rx: Arc::clone(&self.rx),
        }))
    }
}

struct TestInputHandler;
impl RdpServerInputHandler for TestInputHandler {
    fn keyboard(&mut self, _: KeyboardEvent) {}
    fn mouse(&mut self, _: MouseEvent) {}
}

async fn client_server<F, Fut>(client_config: connector::Config, clientfn: F)
where
    F: FnOnce(ActiveStage, Framed<TokioStream<TlsStream<TcpStream>>>, UnboundedSender<DisplayUpdate>) -> Fut + 'static,
    Fut: Future<Output = (ActiveStage, Framed<TokioStream<TlsStream<TcpStream>>>)>,
{
    client_server_with_connector(
        client_config,
        |connector| connector,
        move |stage, framed, display_tx, _echo_handle| clientfn(stage, framed, display_tx),
    )
    .await;
}

async fn client_server_with_connector<F, Fut, C>(client_config: connector::Config, connector_factory: C, clientfn: F)
where
    F: FnOnce(
            ActiveStage,
            Framed<TokioStream<TlsStream<TcpStream>>>,
            UnboundedSender<DisplayUpdate>,
            server::EchoServerHandle,
        ) -> Fut
        + 'static,
    Fut: Future<Output = (ActiveStage, Framed<TokioStream<TlsStream<TcpStream>>>)>,
    C: FnOnce(connector::ClientConnector) -> connector::ClientConnector + 'static,
{
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let cert_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/certs/server-cert.pem");
    let key_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/certs/server-key.pem");
    let identity = TlsIdentityCtx::init_from_paths(&cert_path, &key_path).expect("failed to init TLS identity");
    let acceptor = identity.make_acceptor().expect("failed to build TLS acceptor");

    let (display_tx, display_rx) = mpsc::unbounded_channel();
    let mut server = RdpServer::builder()
        .with_addr(([127, 0, 0, 1], 0))
        .with_tls(acceptor)
        .with_input_handler(TestInputHandler)
        .with_display_handler(TestDisplay {
            rx: Arc::new(Mutex::new(display_rx)),
        })
        .build();
    server.set_credentials(Some(server::Credentials {
        username: USERNAME.into(),
        password: PASSWORD.into(),
        domain: None,
    }));
    let ev = server.event_sender().clone();
    let echo_handle = server.echo_handle().clone();

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let server = tokio::task::spawn_local(async move {
                server.run().await.unwrap();
            });

            let client = tokio::task::spawn_local(async move {
                let (tx, rx) = oneshot::channel();
                ev.send(ServerEvent::GetLocalAddr(tx)).unwrap();
                let server_addr = rx.await.unwrap().unwrap();
                let tcp_stream = TcpStream::connect(server_addr).await.expect("TCP connect");
                let client_addr = tcp_stream.local_addr().expect("local_addr");
                let mut framed = ironrdp_tokio::TokioFramed::new(tcp_stream);
                let connector = connector::ClientConnector::new(client_config, client_addr);
                let mut connector = connector_factory(connector);
                let should_upgrade = ironrdp_async::connect_begin(&mut framed, &mut connector)
                    .await
                    .expect("begin connection");
                let initial_stream = framed.into_inner_no_leftover();
                let (upgraded_stream, tls_cert) = ironrdp_tls::upgrade(initial_stream, "localhost")
                    .await
                    .expect("TLS upgrade");
                let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
                let mut upgraded_framed = ironrdp_tokio::TokioFramed::new(upgraded_stream);
                let server_public_key =
                    ironrdp_tls::extract_tls_server_public_key(&tls_cert).expect("extract server public key");
                let connection_result = ironrdp_async::connect_finalize(
                    upgraded,
                    connector,
                    &mut upgraded_framed,
                    &mut ironrdp_tokio::reqwest::ReqwestNetworkClient::new(),
                    "localhost".into(),
                    server_public_key.to_owned(),
                    None,
                )
                .await
                .expect("finalize connection");

                let active_stage = ActiveStage::new(connection_result);
                let (active_stage, mut upgraded_framed) =
                    clientfn(active_stage, upgraded_framed, display_tx, echo_handle).await;
                let outputs = active_stage.graceful_shutdown().expect("shutdown");
                for out in outputs {
                    match out {
                        ActiveStageOutput::ResponseFrame(frame) => {
                            upgraded_framed.write_all(&frame).await.expect("write frame");
                        }
                        _ => unimplemented!(),
                    }
                }

                // server should probably send TLS close_notify
                while let Ok(pdu) = upgraded_framed.read_pdu().await {
                    debug!(?pdu);
                }
                ev.send(ServerEvent::Quit("bye".into())).unwrap();
            });

            tokio::try_join!(server, client).expect("join");
        })
        .await;
}

/// Variant of [`client_server`] that additionally passes the server [`ServerEvent`] sender to the
/// callback.
///
/// This lets a test trigger server-side events (such as [`ServerEvent::Quit`]) from within the
/// client body without altering the existing `client_server` helper signature.  The function
/// builds its own complete server+client pair so the callback receives the `ev` sender directly.
async fn client_server_with_ev_inner<F, Fut>(client_config: connector::Config, clientfn: F)
where
    F: FnOnce(
            ActiveStage,
            Framed<TokioStream<TlsStream<TcpStream>>>,
            UnboundedSender<DisplayUpdate>,
            UnboundedSender<ServerEvent>,
        ) -> Fut
        + 'static,
    Fut: Future<Output = (ActiveStage, Framed<TokioStream<TlsStream<TcpStream>>>)>,
{
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let cert_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/certs/server-cert.pem");
    let key_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/certs/server-key.pem");
    let identity = TlsIdentityCtx::init_from_paths(&cert_path, &key_path).expect("failed to init TLS identity");
    let acceptor = identity.make_acceptor().expect("failed to build TLS acceptor");

    let (display_tx, display_rx) = mpsc::unbounded_channel();
    let mut server = RdpServer::builder()
        .with_addr(([127, 0, 0, 1], 0))
        .with_tls(acceptor)
        .with_input_handler(TestInputHandler)
        .with_display_handler(TestDisplay {
            rx: Arc::new(Mutex::new(display_rx)),
        })
        .build();
    server.set_credentials(Some(server::Credentials {
        username: USERNAME.into(),
        password: PASSWORD.into(),
        domain: None,
    }));
    let ev_for_client = server.event_sender().clone();

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let server_task = tokio::task::spawn_local(async move {
                server.run().await.unwrap();
            });

            let client_task = tokio::task::spawn_local(async move {
                let (tx, rx) = oneshot::channel();
                ev_for_client.send(ServerEvent::GetLocalAddr(tx)).unwrap();
                let server_addr = rx.await.unwrap().unwrap();
                let tcp_stream = TcpStream::connect(server_addr).await.expect("TCP connect");
                let client_addr = tcp_stream.local_addr().expect("local_addr");
                let mut framed = ironrdp_tokio::TokioFramed::new(tcp_stream);
                let mut connector = connector::ClientConnector::new(client_config, client_addr);
                let should_upgrade = ironrdp_async::connect_begin(&mut framed, &mut connector)
                    .await
                    .expect("begin connection");
                let initial_stream = framed.into_inner_no_leftover();
                let (upgraded_stream, tls_cert) = ironrdp_tls::upgrade(initial_stream, "localhost")
                    .await
                    .expect("TLS upgrade");
                let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
                let mut upgraded_framed = ironrdp_tokio::TokioFramed::new(upgraded_stream);
                let server_public_key =
                    ironrdp_tls::extract_tls_server_public_key(&tls_cert).expect("extract server public key");
                let connection_result = ironrdp_async::connect_finalize(
                    upgraded,
                    connector,
                    &mut upgraded_framed,
                    &mut ironrdp_tokio::reqwest::ReqwestNetworkClient::new(),
                    "localhost".into(),
                    server_public_key.to_owned(),
                    None,
                )
                .await
                .expect("finalize connection");

                let active_stage = ActiveStage::new(connection_result);
                let (active_stage, mut upgraded_framed) =
                    clientfn(active_stage, upgraded_framed, display_tx, ev_for_client.clone()).await;
                let outputs = active_stage.graceful_shutdown().expect("shutdown");
                for out in outputs {
                    match out {
                        ActiveStageOutput::ResponseFrame(frame) => {
                            upgraded_framed.write_all(&frame).await.expect("write frame");
                        }
                        _ => unimplemented!(),
                    }
                }

                while let Ok(pdu) = upgraded_framed.read_pdu().await {
                    debug!(?pdu);
                }
                // The test body may already have sent a Quit; ignore a closed-channel error here.
                let _ = ev_for_client.send(ServerEvent::Quit("bye".into()));
            });

            tokio::try_join!(server_task, client_task).expect("join");
        })
        .await;
}

// ---------------------------------------------------------------------------
// New focused tests
// ---------------------------------------------------------------------------

/// Verifies that the server handles a `ServerEvent::Quit` sent while a client session is active.
///
/// The server's `dispatch_server_events` path converts `ServerEvent::Quit` into
/// `RunState::Disconnect`, which causes `accept_finalize` to return and the connection to close
/// cleanly. The client side should drain to EOF without errors.
#[tokio::test]
async fn test_graceful_disconnect() {
    client_server_with_ev_inner(
        default_client_config(),
        |stage, framed, _display_tx, ev| async move {
            // Ask the server to quit while we are in the active-session phase.
            // The server's client_loop will see the Quit event and return RunState::Disconnect.
            ev.send(ServerEvent::Quit("test graceful disconnect".into()))
                .expect("send Quit");
            (stage, framed)
        },
    )
    .await;
}

/// Verifies that dropping the display sender while the session is active does not panic.
///
/// When the `UnboundedSender<DisplayUpdate>` is dropped the server's `TestDisplayUpdates::next_update`
/// returns `Ok(None)`, which is interpreted as `RunState::Disconnect`.  The server and client
/// should both wind down cleanly without any unwrap panic or error.
#[tokio::test]
async fn test_server_display_write_failure() {
    client_server(
        default_client_config(),
        |stage, framed, display_tx| async move {
            // Dropping the sender closes the display channel.  The server observes Ok(None) from
            // next_update and disconnects the session gracefully.
            drop(display_tx);
            (stage, framed)
        },
    )
    .await;
}

/// Verifies that two consecutive resize / reactivation sequences complete without error.
///
/// This exercises the decompressor-state preservation across multiple reactivations: after the
/// first reactivation `stage.reactivate_fastpath_processor` is called to update channel IDs and
/// pointer settings while retaining the decompressor context; the second resize must then also
/// complete successfully, proving that the preserved state is compatible with a further
/// reactivation cycle.
#[tokio::test]
async fn test_double_reactivation() {
    let client_config = default_client_config();
    let mut image = DecodedImage::new(
        PixelFormat::RgbA32,
        client_config.desktop_size.width,
        client_config.desktop_size.height,
    );

    client_server(client_config, |mut stage, mut framed, display_tx| async move {
        // Helper that drives a single deactivation-reactivation sequence to completion and
        // updates `stage` in-place.  Returns `stage` and `framed` for continued use.
        async fn run_reactivation(
            stage: &mut ActiveStage,
            framed: &mut Framed<TokioStream<TlsStream<TcpStream>>>,
            image: &mut DecodedImage,
        ) {
            let (action, payload) = framed.read_pdu().await.expect("valid PDU");
            let outputs = stage.process(image, action, &payload).expect("stage process");
            let out = outputs.into_iter().next().unwrap();
            match out {
                ActiveStageOutput::DeactivateAll(mut connection_activation) => {
                    let mut buf = pdu::WriteBuf::new();
                    'seq: loop {
                        let written = ironrdp_async::single_sequence_step_read(
                            framed,
                            &mut *connection_activation,
                            &mut buf,
                        )
                        .await
                        .map_err(|e| session::custom_err!("read deactivation-reactivation sequence step", e))
                        .unwrap();

                        if written.size().is_some() {
                            framed
                                .write_all(buf.filled())
                                .await
                                .map_err(|e| {
                                    session::custom_err!("write deactivation-reactivation sequence step", e)
                                })
                                .unwrap();
                        }

                        if let connector::connection_activation::ConnectionActivationState::Finalized {
                            io_channel_id,
                            user_channel_id,
                            desktop_size,
                            share_id,
                            enable_server_pointer,
                            pointer_software_rendering,
                        } = connection_activation.connection_activation_state()
                        {
                            debug!(?desktop_size, "Deactivation-Reactivation Sequence completed");
                            stage.reactivate_fastpath_processor(
                                io_channel_id,
                                user_channel_id,
                                share_id,
                                enable_server_pointer,
                                pointer_software_rendering,
                            );
                            stage.set_share_id(share_id);
                            stage.set_enable_server_pointer(enable_server_pointer);
                            break 'seq;
                        }
                    }
                }
                _ => unreachable!("expected DeactivateAll"),
            }
        }

        // First resize → first reactivation.
        display_tx
            .send(DisplayUpdate::Resize(DesktopSize {
                width: 1280,
                height: 1024,
            }))
            .unwrap();
        run_reactivation(&mut stage, &mut framed, &mut image).await;

        // Second resize → second reactivation, verifying decompressor state is preserved.
        display_tx
            .send(DisplayUpdate::Resize(DesktopSize {
                width: 800,
                height: 600,
            }))
            .unwrap();
        run_reactivation(&mut stage, &mut framed, &mut image).await;

        (stage, framed)
    })
    .await;
}

// Maybe implement Default for Config
fn default_client_config() -> connector::Config {
    connector::Config {
        desktop_size: DesktopSize {
            width: DESKTOP_WIDTH,
            height: DESKTOP_HEIGHT,
        },
        desktop_scale_factor: 0, // Default to 0 per FreeRDP
        enable_tls: true,
        enable_credssp: true,
        credentials: connector::Credentials::UsernamePassword {
            username: USERNAME.into(),
            password: PASSWORD.into(),
        },
        domain: None,
        client_build: semver::Version::parse(env!("CARGO_PKG_VERSION"))
            .map(|version| version.major * 100 + version.minor * 10 + version.patch)
            .unwrap_or(0)
            .try_into()
            .unwrap(),
        client_name: "ironrdp".into(),
        keyboard_type: gcc::KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_layout: 0,
        keyboard_functional_keys_count: 12,
        ime_file_name: "".into(),
        bitmap: None,
        dig_product_id: "".into(),
        // NOTE: hardcode this value like in freerdp
        // https://github.com/FreeRDP/FreeRDP/blob/4e24b966c86fdf494a782f0dfcfc43a057a2ea60/libfreerdp/core/settings.c#LL49C34-L49C70
        client_dir: "C:\\Windows\\System32\\mstscax.dll".into(),
        #[cfg(windows)]
        platform: MajorPlatformType::WINDOWS,
        #[cfg(target_os = "linux")]
        platform: MajorPlatformType::UNIX,
        #[cfg(target_os = "freebsd")]
        platform: MajorPlatformType::UNIX,
        #[cfg(target_os = "dragonfly")]
        platform: MajorPlatformType::UNIX,
        #[cfg(target_os = "openbsd")]
        platform: MajorPlatformType::UNIX,
        #[cfg(target_os = "netbsd")]
        platform: MajorPlatformType::UNIX,
        hardware_id: None,
        request_data: None,
        autologon: false,
        enable_audio_playback: true,
        license_cache: None,
        compression_type: None,
        enable_server_pointer: true,
        pointer_software_rendering: true,
        multitransport_flags: None,
        performance_flags: Default::default(),
        timezone_info: Default::default(),
        alternate_shell: String::new(),
        work_dir: String::new(),
    }
}
