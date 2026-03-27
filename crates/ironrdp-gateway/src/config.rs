//! Gateway static configuration.
//!
//! [`GatewayConfig`] captures the listen address, TLS identity path, and
//! references to the auth/policy implementations.  It is intended to be
//! built once at startup and shared immutably.

use core::net::SocketAddr;
use std::path::PathBuf;

/// Static configuration for the gateway service.
#[derive(Clone, Debug)]
pub struct GatewayConfig {
    /// Address the gateway listens on (e.g. `0.0.0.0:443`).
    pub listen_addr: SocketAddr,
    /// Path to a PEM or PKCS#12 file containing the TLS server identity.
    pub tls_identity_path: PathBuf,
}
