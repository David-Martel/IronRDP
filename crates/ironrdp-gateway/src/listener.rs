//! HTTPS/WSS accept loop and RDCleanPath connection handler.
//!
//! [`GatewayListener`] binds a TCP port, performs a TLS handshake for each
//! incoming connection, upgrades to WebSocket, parses the initial
//! [`ironrdp_rdcleanpath::RDCleanPathPdu`] request, authenticates and
//! authorizes the caller, then hands off the data plane to [`GatewayRelay`].
//!
//! # Flow
//!
//! ```text
//! TCP accept
//!   → TLS handshake  (tokio-rustls)
//!   → WebSocket upgrade  (tokio-tungstenite)
//!   → read RDCleanPath request PDU
//!   → authenticate credentials
//!   → authorize target host
//!   → TCP connect to target RDP host
//!   → start bidirectional GatewayRelay
//! ```

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::{Context as _, Result, bail};
use futures_util::stream::SplitSink;
use futures_util::{SinkExt as _, Stream as _, StreamExt as _};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tracing::{Instrument as _, debug, error, info, warn};

use crate::auth::{Credentials, GatewayAuthenticator, Identity};
use crate::config::GatewayConfig;
use crate::policy::{AuthzDecision, GatewayPolicy, TargetHost};
use crate::relay::GatewayRelay;
use crate::session::GatewaySession;
use ironrdp_rdcleanpath::RDCleanPathPdu;

// ---------------------------------------------------------------------------
// Type-erased wrappers for the `impl Future`-returning traits.
//
// `GatewayAuthenticator` and `GatewayPolicy` return `impl Future` from their
// methods, which makes them not dyn-compatible.  We wrap each in a concrete
// struct that boxes the future, giving us `Arc<dyn>` ergonomics in the
// accept loop without touching the original trait definitions.
// ---------------------------------------------------------------------------

/// Heap-allocated future (object-safe alias).
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Object-safe authenticator wrapper.
trait DynAuthenticator: Send + Sync {
    fn authenticate_boxed(&self, creds: Credentials) -> BoxFuture<'_, Result<Identity>>;
}

impl<A: GatewayAuthenticator> DynAuthenticator for A {
    fn authenticate_boxed(&self, creds: Credentials) -> BoxFuture<'_, Result<Identity>> {
        Box::pin(self.authenticate(creds))
    }
}

/// Object-safe policy wrapper.
trait DynPolicy: Send + Sync {
    fn authorize_boxed<'a>(
        &'a self,
        identity: &'a Identity,
        target: &'a TargetHost,
    ) -> BoxFuture<'a, Result<AuthzDecision>>;
}

impl<P: GatewayPolicy> DynPolicy for P {
    fn authorize_boxed<'a>(
        &'a self,
        identity: &'a Identity,
        target: &'a TargetHost,
    ) -> BoxFuture<'a, Result<AuthzDecision>> {
        Box::pin(self.authorize(identity, target))
    }
}

// ---------------------------------------------------------------------------
// GatewayListener
// ---------------------------------------------------------------------------

/// Accept loop for the RDCleanPath gateway.
///
/// Create one instance per process, then call [`GatewayListener::run`] to
/// start accepting connections.  The listener owns the [`TlsAcceptor`] and
/// dispatches each accepted connection to a dedicated Tokio task so the loop
/// is never blocked by individual connection handling.
///
/// # Example (sketch)
///
/// ```rust,no_run
/// use ironrdp_gateway::listener::GatewayListener;
/// use ironrdp_gateway::config::GatewayConfig;
///
/// # async fn run() -> anyhow::Result<()> {
/// let config = GatewayConfig {
///     listen_addr: "0.0.0.0:443".parse()?,
///     tls_cert_path: "/etc/gateway/cert.pem".into(),
///     tls_key_path: "/etc/gateway/key.pem".into(),
///     tls_identity_path: Default::default(),
/// };
/// let acceptor = config.load_tls_acceptor()?;
/// // supply real auth/policy implementations here
/// let authenticator: Box<dyn std::any::Any> = todo!();
/// let policy: Box<dyn std::any::Any> = todo!();
/// # Ok(())
/// # }
/// ```
pub struct GatewayListener {
    config: GatewayConfig,
    acceptor: TlsAcceptor,
    authenticator: Arc<dyn DynAuthenticator>,
    policy: Arc<dyn DynPolicy>,
}

impl GatewayListener {
    /// Create a new listener.
    ///
    /// The caller is responsible for building the [`TlsAcceptor`]; typically
    /// this is done via [`GatewayConfig::load_tls_acceptor`].
    pub fn new(
        config: GatewayConfig,
        acceptor: TlsAcceptor,
        authenticator: impl GatewayAuthenticator + 'static,
        policy: impl GatewayPolicy + 'static,
    ) -> Self {
        Self {
            config,
            acceptor,
            authenticator: Arc::new(authenticator),
            policy: Arc::new(policy),
        }
    }

    /// Bind the listen address and process incoming connections until the
    /// future is cancelled.
    ///
    /// Each accepted connection is handled in its own Tokio task; errors in
    /// individual connections are logged and do not terminate the loop.
    ///
    /// # Errors
    ///
    /// Returns an error if binding the TCP listener fails.
    pub async fn run(self) -> Result<()> {
        let listener = TcpListener::bind(self.config.listen_addr)
            .await
            .with_context(|| format!("binding gateway listener on {}", self.config.listen_addr))?;

        info!(listen_addr = %self.config.listen_addr, "gateway listener started");

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    let acceptor = self.acceptor.clone();
                    let authenticator = Arc::clone(&self.authenticator);
                    let policy = Arc::clone(&self.policy);

                    let span = tracing::info_span!("connection", %peer_addr);
                    tokio::spawn(
                        async move {
                            if let Err(err) =
                                handle_connection(stream, peer_addr, acceptor, authenticator, policy).await
                            {
                                warn!(error = %err, "connection ended with error");
                            }
                        }
                        .instrument(span),
                    );
                }
                Err(err) => {
                    error!(error = %err, "accept error; continuing");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Connection handler
// ---------------------------------------------------------------------------

/// Drive a single client connection through the full RDCleanPath handshake
/// and then relay bytes until the session ends.
async fn handle_connection(
    stream: TcpStream,
    _peer_addr: SocketAddr,
    acceptor: TlsAcceptor,
    authenticator: Arc<dyn DynAuthenticator>,
    policy: Arc<dyn DynPolicy>,
) -> Result<()> {
    debug!("starting TLS handshake");

    // --- TLS handshake ---
    let tls_stream = acceptor
        .accept(stream)
        .await
        .context("TLS handshake failed")?;

    debug!("TLS handshake complete, upgrading to WebSocket");

    // --- WebSocket upgrade ---
    let ws_stream = tokio_tungstenite::accept_async(tls_stream)
        .await
        .context("WebSocket upgrade failed")?;

    debug!("WebSocket upgrade complete, waiting for RDCleanPath request");

    // --- Read the initial RDCleanPath PDU ---
    let (mut ws_sink, mut ws_source) = ws_stream.split();

    let request_pdu = read_rdcleanpath_request(&mut ws_source).await?;

    let destination = request_pdu
        .destination
        .as_deref()
        .context("RDCleanPath request missing destination field")?
        .to_owned();

    let proxy_auth = request_pdu
        .proxy_auth
        .as_deref()
        .context("RDCleanPath request missing proxy_auth field")?
        .to_owned();

    debug!(destination = %destination, "received RDCleanPath request");

    // --- Parse target host ---
    let target = TargetHost::from_destination(&destination)
        .with_context(|| format!("invalid destination `{destination}`"))?;

    // --- Authenticate ---
    let credentials = Credentials { token: proxy_auth };

    let identity = match authenticator.authenticate_boxed(credentials).await {
        Ok(id) => id,
        Err(err) => {
            warn!(error = %err, "authentication failed");
            send_error_response(&mut ws_sink, 401).await;
            bail!("authentication failed: {err:#}");
        }
    };

    info!(principal = %identity.principal, "authenticated");

    // --- Authorize ---
    let decision = match policy.authorize_boxed(&identity, &target).await {
        Ok(d) => d,
        Err(err) => {
            error!(error = %err, "policy backend error");
            send_error_response(&mut ws_sink, 500).await;
            bail!("policy check failed: {err:#}");
        }
    };

    if decision == AuthzDecision::Deny {
        warn!(
            principal = %identity.principal,
            target_host = %target.host,
            "connection denied by policy"
        );
        send_error_response(&mut ws_sink, 403).await;
        bail!(
            "policy denied connection for `{}` to `{}`",
            identity.principal,
            target.host
        );
    }

    info!(
        principal = %identity.principal,
        target_host = %target.host,
        target_port = target.port,
        "authorization granted, connecting to target"
    );

    // Reunite sink + source now that the handshake is complete; we no longer
    // need them separately for the relay phase.
    let ws_full = ws_sink
        .reunite(ws_source)
        .expect("sink and source always originate from the same stream");

    // --- Connect to target RDP host ---
    let target_addr = format!("{}:{}", target.host, target.port);
    let host_stream = TcpStream::connect(&target_addr)
        .await
        .with_context(|| format!("connecting to target RDP host `{target_addr}`"))?;

    // --- Start session tracking ---
    let session = GatewaySession::new(identity.clone(), target.clone());
    info!(
        principal = %identity.principal,
        target_host = %target.host,
        target_port = target.port,
        "relay session started"
    );

    // Wrap the WebSocket as a raw byte stream so GatewayRelay sees plain
    // AsyncRead + AsyncWrite without any framing awareness.
    let (ws_reader, ws_writer) = tokio::io::split(WsCompat::new(ws_full));
    let (host_reader, host_writer) = tokio::io::split(host_stream);

    // --- Bidirectional relay ---
    let stats = GatewayRelay::new(ws_reader, ws_writer, host_reader, host_writer)
        .run()
        .await
        .context("relay error")?;

    info!(
        principal = %identity.principal,
        target_host = %target.host,
        bytes_client_to_host = stats.bytes_client_to_host,
        bytes_host_to_client = stats.bytes_host_to_client,
        elapsed_secs = session.elapsed().as_secs_f64(),
        "relay session ended"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// RDCleanPath PDU helpers
// ---------------------------------------------------------------------------

/// Read the first binary WebSocket message and decode it as an
/// [`RDCleanPathPdu`].
async fn read_rdcleanpath_request<S>(ws_source: &mut S) -> Result<RDCleanPathPdu>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match ws_source.next().await {
            None => bail!("WebSocket closed before receiving RDCleanPath request"),
            Some(Err(err)) => {
                return Err(anyhow::Error::new(err).context("WebSocket receive error"));
            }
            Some(Ok(Message::Binary(data))) => {
                return RDCleanPathPdu::from_der(&data)
                    .map_err(|e| anyhow::anyhow!("failed to decode RDCleanPath PDU: {e}"));
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {
                // Skip control frames; tungstenite handles pong replies automatically.
                continue;
            }
            Some(Ok(Message::Text(_))) => {
                bail!("unexpected text WebSocket message (expected binary RDCleanPath PDU)");
            }
            Some(Ok(Message::Close(_))) => {
                bail!("client closed WebSocket before sending RDCleanPath request");
            }
            Some(Ok(Message::Frame(_))) => {
                unreachable!("raw frames are never returned when reading")
            }
        }
    }
}

/// Send an [`RDCleanPathPdu`] HTTP-error response over the WebSocket.
///
/// Send errors are suppressed — the connection is closing anyway.
async fn send_error_response<S>(ws_sink: &mut SplitSink<WebSocketStream<S>, Message>, http_status: u16)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let pdu = RDCleanPathPdu::new_http_error(http_status);
    match pdu.to_der() {
        Ok(bytes) => {
            if let Err(err) = ws_sink.send(Message::Binary(bytes.into())).await {
                debug!(error = %err, "failed to send error PDU (connection already closing)");
            }
        }
        Err(err) => {
            debug!(error = %err, "failed to encode error PDU");
        }
    }
}

// ---------------------------------------------------------------------------
// WsCompat: thin AsyncRead + AsyncWrite adapter over a WebSocketStream.
//
// GatewayRelay operates on raw bytes; this adapter bridges the WebSocket
// framing layer.  Binary frames are the only meaningful payload; all other
// frame types are skipped on reads.  Writes produce a single Binary frame per
// call.
// ---------------------------------------------------------------------------

/// Wraps a [`WebSocketStream`] as a plain [`AsyncRead`] + [`AsyncWrite`] byte
/// stream for use by [`GatewayRelay`].
struct WsCompat<S> {
    inner: WebSocketStream<S>,
    /// Unconsumed bytes from the most recently received binary frame.
    read_buf: Vec<u8>,
    read_pos: usize,
}

impl<S> WsCompat<S> {
    fn new(ws: WebSocketStream<S>) -> Self {
        Self {
            inner: ws,
            read_buf: Vec::new(),
            read_pos: 0,
        }
    }
}

impl<S> AsyncRead for WsCompat<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();

        loop {
            // Drain leftover bytes from the previous frame first.
            if this.read_pos < this.read_buf.len() {
                let available = &this.read_buf[this.read_pos..];
                let to_copy = available.len().min(buf.remaining());
                buf.put_slice(&available[..to_copy]);
                this.read_pos += to_copy;
                return Poll::Ready(Ok(()));
            }

            // Poll the next WebSocket frame.
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(Ok(())), // clean EOF
                Poll::Ready(Some(Err(err))) => {
                    return Poll::Ready(Err(io::Error::other(err)));
                }
                Poll::Ready(Some(Ok(Message::Binary(data)))) => {
                    let to_copy = data.len().min(buf.remaining());
                    buf.put_slice(&data[..to_copy]);
                    if to_copy < data.len() {
                        // Stash the remainder for the next read call.
                        this.read_buf = data.into();
                        this.read_pos = to_copy;
                    } else {
                        this.read_buf.clear();
                        this.read_pos = 0;
                    }
                    return Poll::Ready(Ok(()));
                }
                // Skip all non-binary frames silently.
                Poll::Ready(Some(Ok(_))) => continue,
            }
        }
    }
}

impl<S> AsyncWrite for WsCompat<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        use futures_util::Sink as _;

        let this = self.get_mut();

        // Ensure the sink has capacity before queuing a new message.
        match Pin::new(&mut this.inner).poll_ready(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(io::Error::other(err))),
            Poll::Ready(Ok(())) => {}
        }

        let msg = Message::Binary(buf.to_vec().into());
        if let Err(err) = Pin::new(&mut this.inner).start_send(msg) {
            return Poll::Ready(Err(io::Error::other(err)));
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        use futures_util::Sink as _;

        Pin::new(&mut self.get_mut().inner)
            .poll_flush(cx)
            .map_err(io::Error::other)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        use futures_util::Sink as _;

        Pin::new(&mut self.get_mut().inner)
            .poll_close(cx)
            .map_err(io::Error::other)
    }
}
