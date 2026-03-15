//! Connection setup and reconnect policy for the native client.
//!
//! This module owns transport establishment, gateway/WebSocket/TLS upgrade,
//! static and dynamic channel wiring, and top-level reconnect behavior.
//! The live session loop itself lives in the crate-private `session_driver` module.

use core::num::NonZeroU16;
use core::time::Duration;
use std::sync::Arc;

use ironrdp::cliprdr::backend::{ClipboardMessage, CliprdrBackendFactory};
use ironrdp::connector::{ConnectionResult, ConnectorResult};
use ironrdp::displaycontrol::client::DisplayControlClient;
#[cfg(windows)]
use ironrdp::dvc::DvcProcessor as _;
use ironrdp::echo::client::EchoClient;
use ironrdp::graphics::pointer::DecodedPointer;
use ironrdp::pdu::input::fast_path::FastPathInputEvent;
use ironrdp::pdu::{PduResult, pdu_other_err};
use ironrdp::session::{GracefulDisconnectReason, SessionResult};
use ironrdp::svc::SvcMessage;
use ironrdp::{cliprdr, connector, rdpdr, rdpsnd};
use ironrdp_core::WriteBuf;
#[cfg(windows)]
use ironrdp_dvc_com_plugin::load_dvc_plugin;
use ironrdp_dvc_pipe_proxy::DvcNamedPipeProxy;
use ironrdp_rdpsnd_native::cpal;
use ironrdp_tokio::FramedWrite;
use ironrdp_tokio::reqwest::ReqwestNetworkClient;
use rdpdr::NoopRdpdrBackend;
use smallvec::SmallVec;
use socket2::{SockRef, TcpKeepalive};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};
use winit::event_loop::EventLoopProxy;

use crate::config::{Config, RDCleanPathConfig};
use crate::session_driver::{RdpControlFlow, run_active_session};

#[derive(Debug)]
pub enum RdpOutputEvent {
    Image {
        buffer: Vec<u8>,
        width: NonZeroU16,
        height: NonZeroU16,
    },
    ConnectionFailure(connector::ConnectorError),
    PointerDefault,
    PointerHidden,
    PointerPosition {
        x: u16,
        y: u16,
    },
    PointerBitmap(Arc<DecodedPointer>),
    Terminated(SessionResult<GracefulDisconnectReason>),
}

#[derive(Debug)]
pub enum RdpInputEvent {
    Resize {
        width: u16,
        height: u16,
        scale_factor: u32,
        /// The physical size of the display in millimeters (width, height).
        physical_size: Option<(u32, u32)>,
    },
    FastPath(SmallVec<[FastPathInputEvent; 2]>),
    Close,
    Clipboard(ClipboardMessage),
    RecycleFrameBuffer(Vec<u8>),
    SendDvcMessages {
        channel_id: u32,
        messages: Vec<SvcMessage>,
    },
}

impl RdpInputEvent {
    pub fn create_channel() -> (mpsc::UnboundedSender<Self>, mpsc::UnboundedReceiver<Self>) {
        mpsc::unbounded_channel()
    }
}

pub struct DvcPipeProxyFactory {
    rdp_input_sender: mpsc::UnboundedSender<RdpInputEvent>,
}

impl DvcPipeProxyFactory {
    pub fn new(rdp_input_sender: mpsc::UnboundedSender<RdpInputEvent>) -> Self {
        Self { rdp_input_sender }
    }

    pub fn create(&self, channel_name: String, pipe_name: String) -> DvcNamedPipeProxy {
        let rdp_input_sender = self.rdp_input_sender.clone();

        DvcNamedPipeProxy::new(&channel_name, &pipe_name, move |channel_id, messages| {
            rdp_input_sender
                .send(RdpInputEvent::SendDvcMessages { channel_id, messages })
                .map_err(|_error| pdu_other_err!("send DVC messages to the event loop",))?;

            Ok(())
        })
    }

    /// Get a clone of the underlying RDP input event sender.
    pub fn input_sender(&self) -> mpsc::UnboundedSender<RdpInputEvent> {
        self.rdp_input_sender.clone()
    }
}

pub type WriteDvcMessageFn = Box<dyn Fn(u32, SvcMessage) -> PduResult<()> + Send + 'static>;

pub struct RdpClient {
    pub config: Config,
    pub event_loop_proxy: EventLoopProxy<RdpOutputEvent>,
    pub input_event_receiver: mpsc::UnboundedReceiver<RdpInputEvent>,
    pub cliprdr_factory: Option<Box<dyn CliprdrBackendFactory + Send>>,
    pub dvc_pipe_proxy_factory: DvcPipeProxyFactory,
}

impl RdpClient {
    pub async fn run(mut self) {
        let mut same_size_reconnects = 0;

        loop {
            let (connection_result, framed) = if let Some(rdcleanpath) = self.config.rdcleanpath.as_ref() {
                match connect_ws(
                    &self.config,
                    rdcleanpath,
                    self.cliprdr_factory.as_deref(),
                    &self.dvc_pipe_proxy_factory,
                )
                .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        let _ = self.event_loop_proxy.send_event(RdpOutputEvent::ConnectionFailure(e));
                        break;
                    }
                }
            } else {
                match connect(
                    &self.config,
                    self.cliprdr_factory.as_deref(),
                    &self.dvc_pipe_proxy_factory,
                )
                .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        let _ = self.event_loop_proxy.send_event(RdpOutputEvent::ConnectionFailure(e));
                        break;
                    }
                }
            };

            match run_active_session(
                framed,
                connection_result,
                &self.event_loop_proxy,
                &mut self.input_event_receiver,
            )
            .await
            {
                Ok(RdpControlFlow::ReconnectWithNewSize { width, height }) => {
                    let current_width = self.config.connector.desktop_size.width;
                    let current_height = self.config.connector.desktop_size.height;

                    match update_resize_reconnect_state(
                        current_width,
                        current_height,
                        width,
                        height,
                        same_size_reconnects,
                    ) {
                        Ok(next_same_size_reconnects) => {
                            same_size_reconnects = next_same_size_reconnects;
                        }
                        Err(error) => {
                            self.send_terminal_event(Err(error));
                            break;
                        }
                    }

                    info!(
                        current_width,
                        current_height,
                        next_width = width,
                        next_height = height,
                        same_size_reconnects,
                        "Restarting session with updated desktop size"
                    );
                    self.config.connector.desktop_size.width = width;
                    self.config.connector.desktop_size.height = height;
                }
                Ok(RdpControlFlow::TerminatedGracefully(reason)) => {
                    self.send_terminal_event(Ok(reason));
                    break;
                }
                Err(e) => {
                    self.send_terminal_event(Err(e));
                    break;
                }
            }
        }
    }

    fn send_terminal_event(&self, result: SessionResult<GracefulDisconnectReason>) {
        let _ = self.event_loop_proxy.send_event(RdpOutputEvent::Terminated(result));
    }
}

trait AsyncReadWrite: AsyncRead + AsyncWrite {}

impl<T> AsyncReadWrite for T where T: AsyncRead + AsyncWrite {}

type UpgradedFramed = ironrdp_tokio::TokioFramed<Box<dyn AsyncReadWrite + Unpin + Send + Sync>>;

const TCP_KEEPALIVE_TIME: Duration = Duration::from_secs(30);
const TCP_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);
const MAX_SAME_SIZE_RECONNECTS: u8 = 3;

fn update_resize_reconnect_state(
    current_width: u16,
    current_height: u16,
    next_width: u16,
    next_height: u16,
    same_size_reconnects: u8,
) -> SessionResult<u8> {
    if current_width == next_width && current_height == next_height {
        let next_same_size_reconnects = same_size_reconnects.saturating_add(1);

        if next_same_size_reconnects > MAX_SAME_SIZE_RECONNECTS {
            return Err(ironrdp::session::general_err!(
                "too many resize reconnects without a desktop size change"
            ));
        }

        return Ok(next_same_size_reconnects);
    }

    Ok(0)
}

fn configure_tcp_stream(stream: &TcpStream) -> ConnectorResult<()> {
    stream
        .set_nodelay(true)
        .map_err(|e| connector::custom_err!("set TCP_NODELAY", e))?;

    let keepalive = TcpKeepalive::new()
        .with_time(TCP_KEEPALIVE_TIME)
        .with_interval(TCP_KEEPALIVE_INTERVAL);

    SockRef::from(stream)
        .set_tcp_keepalive(&keepalive)
        .map_err(|e| connector::custom_err!("set TCP keepalive", e))?;

    debug!(
        keepalive_time = ?TCP_KEEPALIVE_TIME,
        keepalive_interval = ?TCP_KEEPALIVE_INTERVAL,
        "Configured TCP socket"
    );

    Ok(())
}

async fn connect(
    config: &Config,
    cliprdr_factory: Option<&(dyn CliprdrBackendFactory + Send)>,
    dvc_pipe_proxy_factory: &DvcPipeProxyFactory,
) -> ConnectorResult<(ConnectionResult, UpgradedFramed)> {
    let dest = format!("{}:{}", config.destination.name(), config.destination.port());

    let (client_addr, stream) = if let Some(ref gw_config) = config.gw {
        let (gw, client_addr) = ironrdp_mstsgu::GwClient::connect(gw_config, &config.connector.client_name)
            .await
            .map_err(|e| connector::custom_err!("GW Connect", e))?;
        (client_addr, tokio_util::either::Either::Left(gw))
    } else {
        let stream = TcpStream::connect(dest)
            .await
            .map_err(|e| connector::custom_err!("TCP connect", e))?;
        configure_tcp_stream(&stream)?;
        let client_addr = stream
            .local_addr()
            .map_err(|e| connector::custom_err!("get socket local address", e))?;
        (client_addr, tokio_util::either::Either::Right(stream))
    };
    let mut framed = ironrdp_tokio::TokioFramed::new(stream);

    let mut drdynvc = ironrdp::dvc::DrdynvcClient::new()
        .with_dynamic_channel(DisplayControlClient::new(|_| Ok(Vec::new())))
        .with_dynamic_channel(EchoClient::new());

    // Instantiate all DVC proxies
    for proxy in config.dvc_pipe_proxies.iter() {
        let channel_name = proxy.channel_name.clone();
        let pipe_name = proxy.pipe_name.clone();

        trace!(%channel_name, %pipe_name, "Creating DVC proxy");

        drdynvc = drdynvc.with_dynamic_channel(dvc_pipe_proxy_factory.create(channel_name, pipe_name));
    }

    // Load DVC COM plugins (Windows only)
    #[cfg(windows)]
    {
        let sender = dvc_pipe_proxy_factory.input_sender();
        for plugin_path in config.dvc_plugins.iter() {
            info!(dll = %plugin_path.display(), "Loading DVC COM plugin");

            let sender_clone = sender.clone();
            match load_dvc_plugin(plugin_path, move || {
                let sender = sender_clone.clone();
                Box::new(move |channel_id, messages| {
                    sender
                        .send(RdpInputEvent::SendDvcMessages { channel_id, messages })
                        .map_err(|_error| pdu_other_err!("send COM DVC messages to the event loop"))?;
                    Ok(())
                })
            }) {
                Ok(channels) => {
                    for channel in channels {
                        info!(channel_name = %channel.channel_name(), "Registered COM DVC channel");
                        drdynvc = drdynvc.with_dynamic_channel(channel);
                    }
                }
                Err(e) => {
                    error!(dll = %plugin_path.display(), error = %e, "Failed to load DVC COM plugin");
                }
            }
        }
    }

    let mut connector = connector::ClientConnector::new(config.connector.clone(), client_addr)
        .with_static_channel(drdynvc)
        .with_static_channel(rdpsnd::client::Rdpsnd::new(Box::new(cpal::RdpsndBackend::new())))
        .with_static_channel(rdpdr::Rdpdr::new(Box::new(NoopRdpdrBackend {}), "IronRDP".to_owned()).with_smartcard(0));

    if let Some(builder) = cliprdr_factory {
        let backend = builder.build_cliprdr_backend();

        let cliprdr = cliprdr::Cliprdr::new(backend);

        connector.attach_static_channel(cliprdr);
    }

    let should_upgrade = ironrdp_tokio::connect_begin(&mut framed, &mut connector).await?;

    debug!("TLS upgrade");

    // Ensure there is no leftover
    let (initial_stream, leftover_bytes) = framed.into_inner();

    let (upgraded_stream, tls_cert) = ironrdp_tls::upgrade(initial_stream, config.destination.name())
        .await
        .map_err(|e| connector::custom_err!("TLS upgrade", e))?;

    let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);

    let erased_stream: Box<dyn AsyncReadWrite + Unpin + Send + Sync> = Box::new(upgraded_stream);
    let mut upgraded_framed = ironrdp_tokio::TokioFramed::new_with_leftover(erased_stream, leftover_bytes);

    let server_public_key = ironrdp_tls::extract_tls_server_public_key(&tls_cert)
        .ok_or_else(|| connector::general_err!("unable to extract tls server public key"))?;
    let connection_result = ironrdp_tokio::connect_finalize(
        upgraded,
        connector,
        &mut upgraded_framed,
        &mut ReqwestNetworkClient::new(),
        (&config.destination).into(),
        server_public_key.to_owned(),
        None,
    )
    .await?;

    info!(
        desktop_width = connection_result.desktop_size.width,
        desktop_height = connection_result.desktop_size.height,
        io_channel_id = connection_result.io_channel_id,
        user_channel_id = connection_result.user_channel_id,
        "Connection established"
    );
    debug!(?connection_result);

    Ok((connection_result, upgraded_framed))
}

async fn connect_ws(
    config: &Config,
    rdcleanpath: &RDCleanPathConfig,
    cliprdr_factory: Option<&(dyn CliprdrBackendFactory + Send)>,
    dvc_pipe_proxy_factory: &DvcPipeProxyFactory,
) -> ConnectorResult<(ConnectionResult, UpgradedFramed)> {
    let hostname = rdcleanpath
        .url
        .host_str()
        .ok_or_else(|| connector::general_err!("host missing from the URL"))?;

    let port = rdcleanpath.url.port_or_known_default().unwrap_or(443);

    let socket = TcpStream::connect((hostname, port))
        .await
        .map_err(|e| connector::custom_err!("TCP connect", e))?;

    configure_tcp_stream(&socket)?;

    let client_addr = socket
        .local_addr()
        .map_err(|e| connector::custom_err!("get socket local address", e))?;

    let (ws, _) = tokio_tungstenite::client_async_tls(rdcleanpath.url.as_str(), socket)
        .await
        .map_err(|e| connector::custom_err!("WS connect", e))?;

    let ws = crate::ws::websocket_compat(ws);

    let mut framed = ironrdp_tokio::TokioFramed::new(ws);

    let mut drdynvc = ironrdp::dvc::DrdynvcClient::new()
        .with_dynamic_channel(DisplayControlClient::new(|_| Ok(Vec::new())))
        .with_dynamic_channel(EchoClient::new());

    // Instantiate all DVC proxies
    for proxy in config.dvc_pipe_proxies.iter() {
        let channel_name = proxy.channel_name.clone();
        let pipe_name = proxy.pipe_name.clone();

        trace!(%channel_name, %pipe_name, "Creating DVC proxy");

        drdynvc = drdynvc.with_dynamic_channel(dvc_pipe_proxy_factory.create(channel_name, pipe_name));
    }

    // Load DVC COM plugins (Windows only)
    #[cfg(windows)]
    {
        let sender = dvc_pipe_proxy_factory.input_sender();
        for plugin_path in config.dvc_plugins.iter() {
            info!(dll = %plugin_path.display(), "Loading DVC COM plugin");

            let sender_clone = sender.clone();
            match load_dvc_plugin(plugin_path, move || {
                let sender = sender_clone.clone();
                Box::new(move |channel_id, messages| {
                    sender
                        .send(RdpInputEvent::SendDvcMessages { channel_id, messages })
                        .map_err(|_error| pdu_other_err!("send COM DVC messages to the event loop"))?;
                    Ok(())
                })
            }) {
                Ok(channels) => {
                    for channel in channels {
                        info!(channel_name = %channel.channel_name(), "Registered COM DVC channel");
                        drdynvc = drdynvc.with_dynamic_channel(channel);
                    }
                }
                Err(e) => {
                    error!(dll = %plugin_path.display(), error = %e, "Failed to load DVC COM plugin");
                }
            }
        }
    }

    let mut connector = connector::ClientConnector::new(config.connector.clone(), client_addr)
        .with_static_channel(drdynvc)
        .with_static_channel(rdpsnd::client::Rdpsnd::new(Box::new(cpal::RdpsndBackend::new())))
        .with_static_channel(rdpdr::Rdpdr::new(Box::new(NoopRdpdrBackend {}), "IronRDP".to_owned()).with_smartcard(0));

    if let Some(builder) = cliprdr_factory {
        let backend = builder.build_cliprdr_backend();

        let cliprdr = cliprdr::Cliprdr::new(backend);

        connector.attach_static_channel(cliprdr);
    }

    let destination = format!("{}:{}", config.destination.name(), config.destination.port());

    let (upgraded, server_public_key) = connect_rdcleanpath(
        &mut framed,
        &mut connector,
        destination,
        rdcleanpath.auth_token.clone(),
        None,
    )
    .await?;

    let connection_result = ironrdp_tokio::connect_finalize(
        upgraded,
        connector,
        &mut framed,
        &mut ReqwestNetworkClient::new(),
        (&config.destination).into(),
        server_public_key,
        None,
    )
    .await?;

    let (ws, leftover_bytes) = framed.into_inner();
    let erased_stream: Box<dyn AsyncReadWrite + Unpin + Send + Sync> = Box::new(ws);
    let upgraded_framed = ironrdp_tokio::TokioFramed::new_with_leftover(erased_stream, leftover_bytes);

    Ok((connection_result, upgraded_framed))
}

async fn connect_rdcleanpath<S>(
    framed: &mut ironrdp_tokio::Framed<S>,
    connector: &mut connector::ClientConnector,
    destination: String,
    proxy_auth_token: String,
    pcb: Option<String>,
) -> ConnectorResult<(ironrdp_tokio::Upgraded, Vec<u8>)>
where
    S: ironrdp_tokio::FramedRead + FramedWrite,
{
    use ironrdp::connector::Sequence as _;
    use x509_cert::der::Decode as _;

    #[derive(Clone, Copy, Debug)]
    struct RDCleanPathHint;

    const RDCLEANPATH_HINT: RDCleanPathHint = RDCleanPathHint;

    impl ironrdp::pdu::PduHint for RDCleanPathHint {
        fn find_size(&self, bytes: &[u8]) -> ironrdp::core::DecodeResult<Option<(bool, usize)>> {
            match ironrdp_rdcleanpath::RDCleanPathPdu::detect(bytes) {
                ironrdp_rdcleanpath::DetectionResult::Detected { total_length, .. } => Ok(Some((true, total_length))),
                ironrdp_rdcleanpath::DetectionResult::NotEnoughBytes => Ok(None),
                ironrdp_rdcleanpath::DetectionResult::Failed => Err(ironrdp::core::other_err!(
                    "RDCleanPathHint",
                    "detection failed (invalid PDU)"
                )),
            }
        }
    }

    let mut buf = WriteBuf::new();

    info!("Begin connection procedure");

    {
        // RDCleanPath request

        let connector::ClientConnectorState::ConnectionInitiationSendRequest = connector.state else {
            return Err(connector::general_err!("invalid connector state (send request)"));
        };

        debug_assert!(connector.next_pdu_hint().is_none());

        let written = connector.step_no_input(&mut buf)?;
        let x224_pdu_len = written.size().expect("written size");
        debug_assert_eq!(x224_pdu_len, buf.filled_len());
        let x224_pdu = buf.filled().to_vec();

        let rdcleanpath_req =
            ironrdp_rdcleanpath::RDCleanPathPdu::new_request(x224_pdu, destination, proxy_auth_token, pcb)
                .map_err(|e| connector::custom_err!("new RDCleanPath request", e))?;
        debug!(message = ?rdcleanpath_req, "Send RDCleanPath request");
        let rdcleanpath_req = rdcleanpath_req
            .to_der()
            .map_err(|e| connector::custom_err!("RDCleanPath request encode", e))?;

        framed
            .write_all(&rdcleanpath_req)
            .await
            .map_err(|e| connector::custom_err!("couldn't write RDCleanPath request", e))?;
    }

    {
        // RDCleanPath response

        let rdcleanpath_res = framed
            .read_by_hint(&RDCLEANPATH_HINT)
            .await
            .map_err(|e| connector::custom_err!("read RDCleanPath request", e))?;

        let rdcleanpath_res = ironrdp_rdcleanpath::RDCleanPathPdu::from_der(&rdcleanpath_res)
            .map_err(|e| connector::custom_err!("RDCleanPath response decode", e))?;

        debug!(message = ?rdcleanpath_res, "Received RDCleanPath PDU");

        let (x224_connection_response, server_cert_chain) = match rdcleanpath_res
            .into_enum()
            .map_err(|e| connector::custom_err!("invalid RDCleanPath PDU", e))?
        {
            ironrdp_rdcleanpath::RDCleanPath::Request { .. } => {
                return Err(connector::general_err!(
                    "received an unexpected RDCleanPath type (request)",
                ));
            }
            ironrdp_rdcleanpath::RDCleanPath::Response {
                x224_connection_response,
                server_cert_chain,
                server_addr: _,
            } => (x224_connection_response, server_cert_chain),
            ironrdp_rdcleanpath::RDCleanPath::GeneralErr(error) => {
                return Err(connector::custom_err!("received an RDCleanPath error", error));
            }
            ironrdp_rdcleanpath::RDCleanPath::NegotiationErr {
                x224_connection_response,
            } => {
                // Try to decode as X.224 Connection Confirm to extract negotiation failure details.
                if let Ok(x224_confirm) = ironrdp_core::decode::<
                    ironrdp::pdu::x224::X224<ironrdp::pdu::nego::ConnectionConfirm>,
                >(&x224_connection_response)
                    && let ironrdp::pdu::nego::ConnectionConfirm::Failure { code } = x224_confirm.0
                {
                    // Convert to negotiation failure instead of generic RDCleanPath error.
                    let negotiation_failure = connector::NegotiationFailure::from(code);
                    return Err(connector::ConnectorError::new(
                        "RDP negotiation failed",
                        connector::ConnectorErrorKind::Negotiation(negotiation_failure),
                    ));
                }

                // Fallback to generic error if we can't decode the negotiation failure.
                return Err(connector::general_err!("received an RDCleanPath negotiation error"));
            }
        };

        let connector::ClientConnectorState::ConnectionInitiationWaitConfirm { .. } = connector.state else {
            return Err(connector::general_err!("invalid connector state (wait confirm)"));
        };

        debug_assert!(connector.next_pdu_hint().is_some());

        buf.clear();
        let written = connector.step(x224_connection_response.as_bytes(), &mut buf)?;

        debug_assert!(written.is_nothing());

        let server_cert = server_cert_chain
            .into_iter()
            .next()
            .ok_or_else(|| connector::general_err!("server cert chain missing from rdcleanpath response"))?;

        let cert = x509_cert::Certificate::from_der(server_cert.as_bytes())
            .map_err(|e| connector::custom_err!("server cert chain missing from rdcleanpath response", e))?;

        let server_public_key = cert
            .tbs_certificate
            .subject_public_key_info
            .subject_public_key
            .as_bytes()
            .ok_or_else(|| connector::general_err!("subject public key BIT STRING is not aligned"))?
            .to_owned();

        let should_upgrade = ironrdp_tokio::skip_connect_begin(connector);

        // At this point, proxy established the TLS session.

        let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, connector);

        Ok((upgraded, server_public_key))
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_SAME_SIZE_RECONNECTS, update_resize_reconnect_state};

    #[test]
    fn resize_reconnect_resets_counter_when_size_changes() {
        let counter = update_resize_reconnect_state(1024, 768, 1600, 900, MAX_SAME_SIZE_RECONNECTS)
            .expect("size change should be accepted");

        assert_eq!(counter, 0);
    }

    #[test]
    fn resize_reconnect_counts_unchanged_size_retries() {
        let counter =
            update_resize_reconnect_state(1024, 768, 1024, 768, 1).expect("same size retry should be tracked");

        assert_eq!(counter, 2);
    }

    #[test]
    fn resize_reconnect_rejects_excessive_unchanged_size_retries() {
        let error = update_resize_reconnect_state(1024, 768, 1024, 768, MAX_SAME_SIZE_RECONNECTS)
            .expect_err("same size retry limit should be enforced");

        assert!(
            error
                .to_string()
                .contains("too many resize reconnects without a desktop size change")
        );
    }
}
