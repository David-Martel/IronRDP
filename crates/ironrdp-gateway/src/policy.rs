//! Gateway authorization policy trait and decision types.
//!
//! Authorization is deliberately separated from authentication: an
//! [`Identity`](crate::auth::Identity) must be obtained first, then the
//! [`GatewayPolicy`] is consulted to decide whether that identity may reach a
//! specific target host.

use anyhow::Result;

use crate::auth::Identity;

/// A target host the client wishes to reach.
///
/// Parsed from the `destination` field of the incoming
/// [`ironrdp_rdcleanpath::RDCleanPathPdu`] request.
#[derive(Clone, Debug)]
pub struct TargetHost {
    /// Hostname or IP address of the RDP server.
    pub host: String,
    /// TCP port (typically 3389).
    pub port: u16,
}

impl TargetHost {
    /// Build a `TargetHost` by splitting a `"host:port"` destination string.
    ///
    /// Returns `None` when the string cannot be parsed.
    ///
    /// # Example
    ///
    /// ```rust
    /// use ironrdp_gateway::policy::TargetHost;
    ///
    /// let t = TargetHost::from_destination("rdp-server.corp:3389").unwrap();
    /// assert_eq!(t.host, "rdp-server.corp");
    /// assert_eq!(t.port, 3389);
    /// ```
    #[must_use]
    pub fn from_destination(destination: &str) -> Option<Self> {
        let (host, port_str) = destination.rsplit_once(':')?;
        let port = port_str.parse().ok()?;
        Some(Self {
            host: host.to_owned(),
            port,
        })
    }
}

/// Authorization decision returned by [`GatewayPolicy::authorize`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthzDecision {
    /// The identity is permitted to connect to the requested target.
    Allow,
    /// The identity is not permitted; the gateway must refuse the connection.
    Deny,
}

/// Determines whether an authenticated identity may reach a target host.
///
/// Implementations encode site-specific access rules (ACLs, allow-lists, etc.)
/// and must be cheap to call per connection since they run in the gateway
/// accept loop.
///
/// # Errors
///
/// Returns an error only for unrecoverable backend failures (e.g. policy store
/// is unavailable).  A policy violation must be expressed as
/// [`AuthzDecision::Deny`], not as an error.
pub trait GatewayPolicy: Send + Sync + 'static {
    /// Check whether `identity` may connect to `target`.
    ///
    /// # Errors
    ///
    /// Returns an error if the policy backend cannot be reached.
    fn authorize(
        &self,
        identity: &Identity,
        target: &TargetHost,
    ) -> impl Future<Output = Result<AuthzDecision>> + Send;
}
