//! Gateway authentication traits and credential types.
//!
//! This module defines the `GatewayAuthenticator` trait that decouples the
//! gateway core from any specific authentication backend (e.g. RADIUS, LDAP,
//! static token).  Implementations live outside this crate.

use anyhow::Result;

/// Opaque credential bundle presented by a connecting client.
///
/// The gateway extracts this from the `proxy_auth` field of the incoming
/// [`ironrdp_rdcleanpath::RDCleanPathPdu`] request and passes it verbatim to
/// the configured [`GatewayAuthenticator`].
#[derive(Clone, Debug)]
pub struct Credentials {
    /// Raw token or encoded credential string from the client.
    pub token: String,
}

/// Identity returned by a successful authentication call.
///
/// Carries the authenticated principal name and any opaque claims the
/// policy layer may inspect (e.g. group memberships, expiry).
#[derive(Clone, Debug)]
pub struct Identity {
    /// Human-readable principal name (username, service account, etc.).
    pub principal: String,
    /// Arbitrary key-value claims produced by the authenticator.
    pub claims: std::collections::HashMap<String, String>,
}

/// Authenticates incoming gateway clients.
///
/// Implementations are responsible for validating `Credentials` and
/// returning an [`Identity`] on success.  All I/O (e.g. RADIUS round-trips)
/// must be performed asynchronously so the gateway event loop is not blocked.
///
/// # Errors
///
/// Returns an error when authentication fails or when the backend is
/// unreachable.
pub trait GatewayAuthenticator: Send + Sync + 'static {
    /// Validate `credentials` and resolve the caller's [`Identity`].
    ///
    /// # Errors
    ///
    /// Returns an error if the credentials are invalid or the backend returns
    /// an error.
    fn authenticate(
        &self,
        credentials: Credentials,
    ) -> impl Future<Output = Result<Identity>> + Send;
}
