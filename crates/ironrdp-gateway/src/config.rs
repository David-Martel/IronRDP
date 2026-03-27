//! Gateway static configuration.
//!
//! [`GatewayConfig`] captures the listen address, TLS identity path, and
//! references to the auth/policy implementations.  It is intended to be
//! built once at startup and shared immutably.

use core::net::SocketAddr;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use tokio_rustls::rustls::pki_types::pem::PemObject as _;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::{TlsAcceptor, rustls};

/// Static configuration for the gateway service.
#[derive(Clone, Debug)]
pub struct GatewayConfig {
    /// Address the gateway listens on (e.g. `0.0.0.0:443`).
    pub listen_addr: SocketAddr,
    /// Path to a PEM file containing the TLS server certificate chain.
    pub tls_cert_path: PathBuf,
    /// Path to a PEM file containing the TLS server private key.
    pub tls_key_path: PathBuf,
    /// Convenience field kept for callers that supply a single combined path;
    /// `load_tls_acceptor` ignores this and uses the split cert/key fields.
    pub tls_identity_path: PathBuf,
}

impl GatewayConfig {
    /// Build a [`TlsAcceptor`] from the PEM cert and key files referenced by
    /// this configuration.
    ///
    /// When `tls_cert_path` and `tls_key_path` are non-empty they are used
    /// directly.  Otherwise the method falls back to treating
    /// `tls_identity_path` as a combined PEM file that contains both the
    /// certificate chain and the private key.
    ///
    /// # Errors
    ///
    /// Returns an error if any file cannot be opened, if the PEM data is
    /// malformed, or if rustls rejects the certificate/key pair.
    pub fn load_tls_acceptor(&self) -> anyhow::Result<TlsAcceptor> {
        // Prefer explicit split paths; fall back to the combined identity path.
        let (cert_src, key_src) = if self.tls_cert_path.as_os_str().is_empty()
            || self.tls_key_path.as_os_str().is_empty()
        {
            (self.tls_identity_path.as_path(), self.tls_identity_path.as_path())
        } else {
            (self.tls_cert_path.as_path(), self.tls_key_path.as_path())
        };

        let certs: Vec<CertificateDer<'static>> = if cert_src
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pem"))
        {
            CertificateDer::pem_file_iter(cert_src)
                .with_context(|| format!("reading TLS cert `{cert_src:?}`"))?
                .collect::<Result<Vec<_>, _>>()
                .with_context(|| format!("collecting TLS cert `{cert_src:?}`"))?
        } else {
            rustls_pemfile::certs(&mut BufReader::new(
                File::open(cert_src).with_context(|| format!("opening TLS cert `{cert_src:?}`"))?,
            ))
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| format!("collecting TLS cert `{cert_src:?}`"))?
        };

        let priv_key = if key_src
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pem"))
        {
            PrivateKeyDer::from_pem_file(key_src)
                .with_context(|| format!("reading TLS key `{key_src:?}`"))?
        } else {
            rustls_pemfile::pkcs8_private_keys(&mut BufReader::new(
                File::open(key_src).with_context(|| format!("opening TLS key `{key_src:?}`"))?,
            ))
            .next()
            .context("no private key found in TLS key file")?
            .map(PrivateKeyDer::from)?
        };

        let mut server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, priv_key)
            .context("invalid TLS certificate/key pair")?;

        // Support SSLKEYLOGFILE for Wireshark traffic analysis during development.
        server_config.key_log = Arc::new(rustls::KeyLogFile::new());

        Ok(TlsAcceptor::from(Arc::new(server_config)))
    }
}
