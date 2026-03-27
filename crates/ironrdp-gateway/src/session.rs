//! Per-session metadata tracking for active gateway relay sessions.
//!
//! Each [`GatewaySession`] records which identity connected to which host, when
//! the session started, and byte-count summaries from the relay.  This is the
//! anchor for future audit logging, idle timeouts, and admin visibility.

use std::time::Instant;

use crate::auth::Identity;
use crate::policy::TargetHost;

/// Metadata for a single active relay session.
#[derive(Clone, Debug)]
pub struct GatewaySession {
    /// Authenticated identity that owns this session.
    pub identity: Identity,
    /// Target host being relayed.
    pub target: TargetHost,
    /// Wall-clock instant when the relay started.
    pub started_at: Instant,
}

impl GatewaySession {
    /// Create a new session record.
    #[must_use]
    pub fn new(identity: Identity, target: TargetHost) -> Self {
        Self {
            identity,
            target,
            started_at: Instant::now(),
        }
    }

    /// Elapsed time since the session was established.
    #[must_use]
    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }
}
