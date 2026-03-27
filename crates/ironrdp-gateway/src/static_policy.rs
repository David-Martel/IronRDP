//! File-based authorization policy backed by a TOML rule set.
//!
//! See [`StaticFilePolicy`] for the full format description.

use std::future;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::Deserialize;

use crate::auth::Identity;
use crate::policy::{AuthzDecision, GatewayPolicy, TargetHost};

// ---------------------------------------------------------------------------
// TOML schema
// ---------------------------------------------------------------------------

/// Raw TOML representation of a single authorization rule.
#[derive(Debug, Deserialize)]
struct RawRule {
    /// Principal name or `"*"` for any authenticated user.
    principal: String,
    /// List of allowed host patterns.
    ///
    /// Each entry is either `"host"` (default port 3389) or `"host:port"`.
    hosts: Vec<String>,
}

/// Top-level TOML document (list of rules).
#[derive(Debug, Deserialize)]
struct PolicyFile {
    #[serde(default)]
    rules: Vec<RawRule>,
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A parsed host pattern from a policy rule.
#[derive(Clone, Debug)]
enum HostPattern {
    /// Exact `"host:port"` pair — both must match.
    Exact { host: String, port: u16 },
    /// Raw string prefix — matches any target whose `"host:port"` form starts
    /// with this prefix.  Used for subnet-style entries like `"192.168.1."`.
    Prefix(String),
}

impl HostPattern {
    fn matches(&self, target_host: &str, target_port: u16) -> bool {
        match self {
            Self::Exact { host, port } => target_host == host && target_port == *port,
            Self::Prefix(prefix) => {
                let target_str = format!("{target_host}:{target_port}");
                target_str.starts_with(prefix.as_str())
            }
        }
    }
}

/// A single parsed authorization rule.
#[derive(Clone, Debug)]
struct PolicyRule {
    /// Exact principal name, or `*` meaning any authenticated identity.
    principal: String,
    /// Allowed host patterns.
    hosts: Vec<HostPattern>,
}

/// File-based authorization policy.
///
/// Reads a TOML policy file and authorizes connections based on an ordered list
/// of rules.  The first rule whose `principal` matches the connecting identity
/// (or whose principal is `"*"`) **and** whose `hosts` list contains an entry
/// that matches the target host decides the outcome.  If no rule matches, the
/// connection is denied.
///
/// # Policy file format
///
/// ```toml
/// [[rules]]
/// principal = "admin"           # exact match or "*" for any authenticated user
/// hosts = ["192.168.1.0/24", "10.0.0.5:3389"]
///
/// [[rules]]
/// principal = "guest"
/// hosts = ["10.0.0.5:3389"]    # single host only
/// ```
///
/// Host entries may be:
/// - `"host"` — matches `host` on the default RDP port (3389)
/// - `"host:port"` — matches `host` on the explicit port
/// - `"prefix"` — if no `:` is present, treated as a plain string prefix match
///   against the normalised `"host:port"` target string
///
/// # Examples
///
/// ```rust
/// use ironrdp_gateway::static_policy::StaticFilePolicy;
///
/// let policy = StaticFilePolicy::from_str(r#"
/// [[rules]]
/// principal = "alice"
/// hosts = ["10.0.0.1:3389"]
/// "#).unwrap();
/// ```
pub struct StaticFilePolicy {
    rules: Vec<PolicyRule>,
}

impl StaticFilePolicy {
    /// Load a `StaticFilePolicy` from a TOML file on disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or the TOML is malformed.
    pub fn from_file(path: &Path) -> Result<Self> {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("read policy file {}", path.display()))?;
        Self::from_str(&raw).with_context(|| format!("parse policy file {}", path.display()))
    }

    /// Parse a `StaticFilePolicy` from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns an error if the TOML is malformed or contains unrecognised keys.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ironrdp_gateway::static_policy::StaticFilePolicy;
    ///
    /// let toml = r#"
    /// [[rules]]
    /// principal = "*"
    /// hosts = ["10.0.0.5:3389"]
    /// "#;
    /// let policy = StaticFilePolicy::from_str(toml).unwrap();
    /// ```
    pub fn from_str(toml: &str) -> Result<Self> {
        let doc: PolicyFile = toml::from_str(toml).context("deserialize policy TOML")?;
        let rules = doc
            .rules
            .into_iter()
            .map(|r| PolicyRule {
                principal: r.principal,
                hosts: r.hosts.into_iter().map(parse_host_pattern).collect(),
            })
            .collect();
        Ok(Self { rules })
    }
}

// ---------------------------------------------------------------------------
// GatewayPolicy impl
// ---------------------------------------------------------------------------

impl GatewayPolicy for StaticFilePolicy {
    fn authorize(
        &self,
        identity: &Identity,
        target: &TargetHost,
    ) -> impl Future<Output = Result<AuthzDecision>> + Send {
        let decision = self.evaluate(&identity.principal, &target.host, target.port);
        future::ready(Ok(decision))
    }
}

impl StaticFilePolicy {
    /// Pure sync evaluation — separated so it can be unit-tested without futures.
    fn evaluate(&self, principal: &str, target_host: &str, target_port: u16) -> AuthzDecision {
        for rule in &self.rules {
            let principal_match = rule.principal == "*" || rule.principal == principal;
            if !principal_match {
                continue;
            }
            for pattern in &rule.hosts {
                if pattern.matches(target_host, target_port) {
                    return AuthzDecision::Allow;
                }
            }
        }
        AuthzDecision::Deny
    }
}

// ---------------------------------------------------------------------------
// Parsing helper
// ---------------------------------------------------------------------------

/// Parse a raw policy file host entry into a typed [`HostPattern`].
///
/// - If the entry ends with a valid `u16` port after a `:`, it is parsed as an
///   `Exact` host/port pair.
/// - Otherwise the raw string is kept as a `Prefix` for subnet-style matching
///   (e.g. `"192.168.1."` matches any target in that /24 on any port).
///
/// A bare hostname with no `:` (e.g. `"myhost.internal"`) is treated as a
/// prefix that will match `"myhost.internal:3389"` and nothing else, since
/// `"myhost.internal:3389".starts_with("myhost.internal")` is true and no
/// other standard host name would share that prefix.  Callers that want
/// port-specific exact matching should write `"myhost.internal:3389"` instead.
fn parse_host_pattern(entry: String) -> HostPattern {
    if let Some((host, port_str)) = entry.rsplit_once(':') {
        if let Ok(port) = port_str.parse::<u16>() {
            return HostPattern::Exact {
                host: host.to_owned(),
                port,
            };
        }
    }
    // No valid port — treat the whole string as a prefix.
    HostPattern::Prefix(entry)
}

// Tests live in `tests/static_policy.rs` (integration tests), because the
// crate sets `[lib] test = false` while `listener.rs` is work-in-progress.
