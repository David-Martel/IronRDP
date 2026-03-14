//! Listener setup, channel registration, and transport bootstrap for the Tokio server.
//!
//! The accepted-client session state machine lives in [`crate::session_driver`].

use core::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use ironrdp_acceptor::{Acceptor, BeginResult};
use ironrdp_cliprdr::CliprdrServer;
use ironrdp_cliprdr::backend::ClipboardMessage;
use ironrdp_core::{decode, impl_as_any};
use ironrdp_displaycontrol::pdu::DisplayControlMonitorLayout;
use ironrdp_displaycontrol::server::{DisplayControlHandler, DisplayControlServer};
use ironrdp_pdu::rdp::capability_sets::{BitmapCodecs, CodecProperty};
pub use ironrdp_pdu::rdp::client_info::Credentials;
use ironrdp_pdu::{PduResult, decode_err, nego};
use ironrdp_svc::StaticChannelSet;
use ironrdp_tokio::TokioFramed;
use ironrdp_rdpsnd::server::{RdpsndServer, RdpsndServerMessage};
use tokio::io::AsyncWriteExt as _;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, warn};
use ironrdp_dvc as dvc;

use crate::clipboard::CliprdrServerFactory;
use crate::display::RdpServerDisplay;
use crate::echo::{EchoDvcBridge, EchoServerHandle, EchoServerMessage};
#[cfg(feature = "egfx")]
use crate::gfx::{EgfxServerMessage, GfxServerFactory};
use crate::handler::RdpServerInputHandler;
use crate::{SoundServerFactory, builder, capabilities};

#[derive(Clone)]
pub struct RdpServerOptions {
    pub addr: SocketAddr,
    pub security: RdpServerSecurity,
    pub codecs: BitmapCodecs,
    pub max_request_size: u32,
}

impl RdpServerOptions {
    /// Default [MultifragmentUpdate] max reassembly buffer size (8 MB).
    ///
    /// Advertised to the client during capability exchange as the largest
    /// reassembled Fast-Path Update the server can accept.
    /// Values that are too large cause certain clients (notably mstsc)
    /// to reject the connection.
    ///
    /// [MultifragmentUpdate]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/01717954-716a-424d-af35-28fb2b86df89
    pub(crate) const DEFAULT_MAX_REQUEST_SIZE: u32 = 8 * 1024 * 1024;

    pub(crate) fn has_image_remote_fx(&self) -> bool {
        self.codecs
            .0
            .iter()
            .any(|codec| matches!(codec.property, CodecProperty::ImageRemoteFx(_)))
    }

    pub(crate) fn has_remote_fx(&self) -> bool {
        self.codecs
            .0
            .iter()
            .any(|codec| matches!(codec.property, CodecProperty::RemoteFx(_)))
    }

    #[cfg(feature = "qoi")]
    pub(crate) fn has_qoi(&self) -> bool {
        self.codecs
            .0
            .iter()
            .any(|codec| matches!(codec.property, CodecProperty::Qoi))
    }

    #[cfg(feature = "qoiz")]
    pub(crate) fn has_qoiz(&self) -> bool {
        self.codecs
            .0
            .iter()
            .any(|codec| matches!(codec.property, CodecProperty::QoiZ))
    }
}

#[derive(Clone)]
pub enum RdpServerSecurity {
    None,
    Tls(TlsAcceptor),
    /// Used for both hybrid + hybrid-ex.
    Hybrid((TlsAcceptor, Vec<u8>)),
}

impl RdpServerSecurity {
    pub fn flag(&self) -> nego::SecurityProtocol {
        match self {
            RdpServerSecurity::None => nego::SecurityProtocol::empty(),
            RdpServerSecurity::Tls(_) => nego::SecurityProtocol::SSL,
            RdpServerSecurity::Hybrid(_) => nego::SecurityProtocol::HYBRID | nego::SecurityProtocol::HYBRID_EX,
        }
    }
}

struct AInputHandler {
    handler: Arc<Mutex<Box<dyn RdpServerInputHandler>>>,
}

impl_as_any!(AInputHandler);

impl dvc::DvcProcessor for AInputHandler {
    fn channel_name(&self) -> &str {
        ironrdp_ainput::CHANNEL_NAME
    }

    fn start(&mut self, _channel_id: u32) -> PduResult<Vec<dvc::DvcMessage>> {
        use ironrdp_ainput::{ServerPdu, VersionPdu};

        let pdu = ServerPdu::Version(VersionPdu::default());

        Ok(vec![Box::new(pdu)])
    }

    fn close(&mut self, _channel_id: u32) {}

    fn process(&mut self, _channel_id: u32, payload: &[u8]) -> PduResult<Vec<dvc::DvcMessage>> {
        use ironrdp_ainput::ClientPdu;

        match decode(payload).map_err(|e| decode_err!(e))? {
            ClientPdu::Mouse(pdu) => {
                let handler = Arc::clone(&self.handler);
                task::spawn_blocking(move || {
                    handler.blocking_lock().mouse(pdu.into());
                });
            }
        }

        Ok(Vec::new())
    }
}

impl dvc::DvcServerProcessor for AInputHandler {}

struct DisplayControlBackend {
    display: Arc<Mutex<Box<dyn RdpServerDisplay>>>,
}

impl DisplayControlBackend {
    fn new(display: Arc<Mutex<Box<dyn RdpServerDisplay>>>) -> Self {
        Self { display }
    }
}

impl DisplayControlHandler for DisplayControlBackend {
    fn monitor_layout(&self, layout: DisplayControlMonitorLayout) {
        let display = Arc::clone(&self.display);
        task::spawn_blocking(move || display.blocking_lock().request_layout(layout));
    }
}

/// RDP Server
///
/// A server is created to listen for connections.
/// After the connection sequence is finalized using the provided security mechanism, the server can:
///  - receive display updates from a [`RdpServerDisplay`] and forward them to the client
///  - receive input events from a client and forward them to an [`RdpServerInputHandler`]
///
/// # Example
///
/// ```
/// use ironrdp_server::{RdpServer, RdpServerInputHandler, RdpServerDisplay, RdpServerDisplayUpdates};
///
///# use anyhow::Result;
///# use ironrdp_server::{DisplayUpdate, DesktopSize, KeyboardEvent, MouseEvent};
///# use tokio_rustls::TlsAcceptor;
///# struct NoopInputHandler;
///# impl RdpServerInputHandler for NoopInputHandler {
///#     fn keyboard(&mut self, _: KeyboardEvent) {}
///#     fn mouse(&mut self, _: MouseEvent) {}
///# }
///# struct NoopDisplay;
///# #[async_trait::async_trait]
///# impl RdpServerDisplay for NoopDisplay {
///#     async fn size(&mut self) -> DesktopSize {
///#         todo!()
///#     }
///#     async fn updates(&mut self) -> Result<Box<dyn RdpServerDisplayUpdates>> {
///#         todo!()
///#     }
///# }
///# async fn stub() -> Result<()> {
/// fn make_tls_acceptor() -> TlsAcceptor {
///    /* snip */
///#    todo!()
/// }
///
/// fn make_input_handler() -> impl RdpServerInputHandler {
///    /* snip */
///#    NoopInputHandler
/// }
///
/// fn make_display_handler() -> impl RdpServerDisplay {
///    /* snip */
///#    NoopDisplay
/// }
///
/// let tls_acceptor = make_tls_acceptor();
/// let input_handler = make_input_handler();
/// let display_handler = make_display_handler();
///
/// let mut server = RdpServer::builder()
///     .with_addr(([127, 0, 0, 1], 3389))
///     .with_tls(tls_acceptor)
///     .with_input_handler(input_handler)
///     .with_display_handler(display_handler)
///     .build();
///
/// server.run().await;
/// Ok(())
///# }
/// ```
pub struct RdpServer {
    pub(crate) opts: RdpServerOptions,
    // FIXME: replace with a channel and poll/process the handler?
    pub(crate) handler: Arc<Mutex<Box<dyn RdpServerInputHandler>>>,
    pub(crate) display: Arc<Mutex<Box<dyn RdpServerDisplay>>>,
    pub(crate) static_channels: StaticChannelSet,
    pub(crate) sound_factory: Option<Box<dyn SoundServerFactory>>,
    pub(crate) cliprdr_factory: Option<Box<dyn CliprdrServerFactory>>,
    pub(crate) echo_handle: EchoServerHandle,
    #[cfg(feature = "egfx")]
    pub(crate) gfx_factory: Option<Box<dyn GfxServerFactory>>,
    #[cfg(feature = "egfx")]
    pub(crate) gfx_handle: Option<crate::gfx::GfxServerHandle>,
    pub(crate) ev_sender: mpsc::UnboundedSender<ServerEvent>,
    pub(crate) ev_receiver: Arc<Mutex<mpsc::UnboundedReceiver<ServerEvent>>>,
    pub(crate) creds: Option<Credentials>,
    pub(crate) local_addr: Option<SocketAddr>,
}

#[derive(Debug)]
pub enum ServerEvent {
    Quit(String),
    Clipboard(ClipboardMessage),
    Rdpsnd(RdpsndServerMessage),
    Echo(EchoServerMessage),
    SetCredentials(Credentials),
    GetLocalAddr(oneshot::Sender<Option<SocketAddr>>),
    #[cfg(feature = "egfx")]
    Egfx(EgfxServerMessage),
}

pub trait ServerEventSender {
    fn set_sender(&mut self, sender: mpsc::UnboundedSender<ServerEvent>);
}

impl ServerEvent {
    pub fn create_channel() -> (mpsc::UnboundedSender<Self>, mpsc::UnboundedReceiver<Self>) {
        mpsc::unbounded_channel()
    }
}

impl RdpServer {
    pub fn new(
        opts: RdpServerOptions,
        handler: Box<dyn RdpServerInputHandler>,
        display: Box<dyn RdpServerDisplay>,
        mut sound_factory: Option<Box<dyn SoundServerFactory>>,
        mut cliprdr_factory: Option<Box<dyn CliprdrServerFactory>>,
        #[cfg(feature = "egfx")] mut gfx_factory: Option<Box<dyn GfxServerFactory>>,
    ) -> Self {
        let (ev_sender, ev_receiver) = ServerEvent::create_channel();
        if let Some(cliprdr) = cliprdr_factory.as_mut() {
            cliprdr.set_sender(ev_sender.clone());
        }
        if let Some(snd) = sound_factory.as_mut() {
            snd.set_sender(ev_sender.clone());
        }
        #[cfg(feature = "egfx")]
        if let Some(gfx) = gfx_factory.as_mut() {
            gfx.set_sender(ev_sender.clone());
        }
        Self {
            opts,
            handler: Arc::new(Mutex::new(handler)),
            display: Arc::new(Mutex::new(display)),
            static_channels: StaticChannelSet::new(),
            sound_factory,
            cliprdr_factory,
            echo_handle: EchoServerHandle::new(ev_sender.clone()),
            #[cfg(feature = "egfx")]
            gfx_factory,
            #[cfg(feature = "egfx")]
            gfx_handle: None,
            ev_sender,
            ev_receiver: Arc::new(Mutex::new(ev_receiver)),
            creds: None,
            local_addr: None,
        }
    }

    pub fn builder() -> builder::RdpServerBuilder<builder::WantsAddr> {
        builder::RdpServerBuilder::new()
    }

    pub fn event_sender(&self) -> &mpsc::UnboundedSender<ServerEvent> {
        &self.ev_sender
    }

    /// Returns the shared ECHO server handle for runtime probe requests and RTT measurements.
    pub fn echo_handle(&self) -> &EchoServerHandle {
        &self.echo_handle
    }

    /// Returns the shared EGFX server handle for proactive frame submission.
    ///
    /// Available after `build_server_with_handle()` returns `Some` during
    /// channel setup. Display handlers use this to call
    /// `send_avc420_frame()` / `send_avc444_frame()` and then signal the
    /// event loop via `ServerEvent::Egfx`.
    #[cfg(feature = "egfx")]
    pub fn gfx_handle(&self) -> Option<&crate::gfx::GfxServerHandle> {
        self.gfx_handle.as_ref()
    }

    fn attach_channels(&mut self, acceptor: &mut Acceptor) {
        if let Some(cliprdr_factory) = self.cliprdr_factory.as_deref() {
            let backend = cliprdr_factory.build_cliprdr_backend();

            let cliprdr = CliprdrServer::new(backend);

            acceptor.attach_static_channel(cliprdr);
        }

        if let Some(factory) = self.sound_factory.as_deref() {
            let backend = factory.build_backend();

            acceptor.attach_static_channel(RdpsndServer::new(backend));
        }

        let dcs_backend = DisplayControlBackend::new(Arc::clone(&self.display));
        let dvc = dvc::DrdynvcServer::new()
            .with_dynamic_channel(AInputHandler {
                handler: Arc::clone(&self.handler),
            })
            .with_dynamic_channel(DisplayControlServer::new(Box::new(dcs_backend)));

        let dvc = {
            let echo_handle = self.echo_handle.clone();
            dvc.with_dynamic_channel(EchoDvcBridge::new(echo_handle))
        };

        #[cfg(feature = "egfx")]
        let dvc = {
            let mut dvc = dvc;
            if let Some(gfx_factory) = self.gfx_factory.as_deref() {
                if let Some((bridge, handle)) = gfx_factory.build_server_with_handle() {
                    self.gfx_handle = Some(handle);
                    dvc = dvc.with_dynamic_channel(bridge);
                } else {
                    let handler = gfx_factory.build_gfx_handler();
                    let gfx_server = ironrdp_egfx::server::GraphicsPipelineServer::new(handler);
                    dvc = dvc.with_dynamic_channel(gfx_server);
                }
            }
            dvc
        };

        acceptor.attach_static_channel(dvc);
    }

    pub async fn run_connection(&mut self, stream: TcpStream) -> Result<()> {
        let framed = TokioFramed::new(stream);

        let size = self.display.lock().await.size().await;
        let capabilities = capabilities::capabilities(&self.opts, size);
        let mut acceptor = Acceptor::new(self.opts.security.flag(), size, capabilities, self.creds.clone());

        self.attach_channels(&mut acceptor);

        let res = ironrdp_acceptor::accept_begin(framed, &mut acceptor)
            .await
            .context("accept_begin failed")?;

        match res {
            BeginResult::ShouldUpgrade(stream) => {
                let tls_acceptor = match &self.opts.security {
                    RdpServerSecurity::Tls(acceptor) => acceptor,
                    RdpServerSecurity::Hybrid((acceptor, _)) => acceptor,
                    RdpServerSecurity::None => unreachable!(),
                };
                let accept = match tls_acceptor.accept(stream).await {
                    Ok(accept) => accept,
                    Err(e) => {
                        warn!("Failed to TLS accept: {}", e);
                        return Ok(());
                    }
                };
                let mut framed = TokioFramed::new(accept);

                acceptor.mark_security_upgrade_as_done();

                if let RdpServerSecurity::Hybrid((_, pub_key)) = &self.opts.security {
                    // how to get the client name?
                    // doesn't seem to matter yet
                    let client_name = framed.get_inner().0.get_ref().0.peer_addr()?.to_string();

                    ironrdp_acceptor::accept_credssp(
                        &mut framed,
                        &mut acceptor,
                        &mut ironrdp_tokio::reqwest::ReqwestNetworkClient::new(),
                        client_name.into(),
                        pub_key.clone(),
                        None,
                    )
                    .await?;
                }

                let framed = self.accept_finalize(framed, acceptor).await?;
                debug!("Shutting down TLS connection");
                let (mut tls_stream, _) = framed.into_inner();
                if let Err(e) = tls_stream.shutdown().await {
                    debug!(?e, "TLS shutdown error");
                }
            }

            BeginResult::Continue(framed) => {
                self.accept_finalize(framed, acceptor).await?;
            }
        };

        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        let listener = TcpListener::bind(self.opts.addr).await?;
        let local_addr = listener.local_addr()?;

        debug!("Listening for connections on {local_addr}");
        self.local_addr = Some(local_addr);

        loop {
            let ev_receiver = Arc::clone(&self.ev_receiver);
            let mut ev_receiver = ev_receiver.lock().await;
            tokio::select! {
                Some(event) = ev_receiver.recv() => {
                    match event {
                        ServerEvent::Quit(reason) => {
                            debug!("Got quit event {reason}");
                            break;
                        }
                        ServerEvent::GetLocalAddr(tx) => {
                            let _ = tx.send(self.local_addr);
                        }
                        ServerEvent::SetCredentials(creds) => {
                            self.set_credentials(Some(creds));
                        }
                        ev => {
                            debug!("Unexpected event {:?}", ev);
                        }
                    }
                },
                Ok((stream, peer)) = listener.accept() => {
                    debug!(?peer, "Received connection");
                    drop(ev_receiver);
                    if let Err(error) = self.run_connection(stream).await {
                        error!(?error, "Connection error");
                    }
                    self.static_channels = StaticChannelSet::new();
                }
                else => break,
            }
        }

        Ok(())
    }

    pub fn set_credentials(&mut self, creds: Option<Credentials>) {
        debug!(?creds, "Changing credentials");
        self.creds = creds
    }
}
