//! Data-plane relay between the client and host TCP legs.
//!
//! [`GatewayRelay`] owns both halves of an established bi-directional relay
//! and drives byte copying until either side closes the connection or an error
//! occurs.  All protocol framing has been completed before the relay starts;
//! only raw bytes flow here.

use anyhow::Result;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::debug;

/// Drives a bidirectional byte relay between two async I/O halves.
///
/// Each leg is represented by a pair `(reader, writer)`.  The relay copies
/// bytes from the client reader to the host writer and vice-versa until one
/// side signals EOF or an I/O error occurs.
///
/// # Example (sketch)
///
/// ```rust,no_run
/// use ironrdp_gateway::relay::GatewayRelay;
/// use tokio::net::TcpStream;
///
/// # async fn run() -> anyhow::Result<()> {
/// let client: TcpStream = todo!("accept from listener");
/// let host: TcpStream = todo!("connect to RDP server");
///
/// let (cr, cw) = tokio::io::split(client);
/// let (hr, hw) = tokio::io::split(host);
///
/// GatewayRelay::new(cr, cw, hr, hw).run().await?;
/// # Ok(())
/// # }
/// ```
pub struct GatewayRelay<CR, CW, HR, HW> {
    client_reader: CR,
    client_writer: CW,
    host_reader: HR,
    host_writer: HW,
}

impl<CR, CW, HR, HW> GatewayRelay<CR, CW, HR, HW>
where
    CR: AsyncRead + Unpin + Send + 'static,
    CW: AsyncWrite + Unpin + Send + 'static,
    HR: AsyncRead + Unpin + Send + 'static,
    HW: AsyncWrite + Unpin + Send + 'static,
{
    /// Create a new relay from the four I/O halves.
    #[must_use]
    pub fn new(client_reader: CR, client_writer: CW, host_reader: HR, host_writer: HW) -> Self {
        Self {
            client_reader,
            client_writer,
            host_reader,
            host_writer,
        }
    }

    /// Run the relay until one leg closes or an error occurs.
    ///
    /// Internally spawns two copy tasks (client→host and host→client) and
    /// waits for both to finish.
    ///
    /// # Errors
    ///
    /// Returns an error if either copy task fails with an I/O error other
    /// than a clean EOF.
    pub async fn run(self) -> Result<RelayStats> {
        use tokio::io::copy;

        let Self {
            mut client_reader,
            mut client_writer,
            mut host_reader,
            mut host_writer,
        } = self;

        debug!("relay: starting bidirectional copy");

        let client_to_host = copy(&mut client_reader, &mut host_writer);
        let host_to_client = copy(&mut host_reader, &mut client_writer);

        let (c2h, h2c) = tokio::try_join!(client_to_host, host_to_client)?;

        debug!(client_to_host = c2h, host_to_client = h2c, "relay: finished");

        Ok(RelayStats {
            bytes_client_to_host: c2h,
            bytes_host_to_client: h2c,
        })
    }
}

/// Byte-count summary produced when a relay session finishes cleanly.
#[derive(Clone, Copy, Debug)]
pub struct RelayStats {
    /// Total bytes forwarded from the client leg to the host leg.
    pub bytes_client_to_host: u64,
    /// Total bytes forwarded from the host leg to the client leg.
    pub bytes_host_to_client: u64,
}
