//! Integration-style tests for [`ironrdp_gateway::static_policy::StaticFilePolicy`].
//!
//! These live in `tests/` (rather than inline `#[cfg(test)]` modules) because
//! the crate sets `[lib] test = false` — the lib target's test harness is
//! disabled while `listener.rs` is work-in-progress.  Integration tests are
//! compiled as a separate binary and are unaffected by that flag.

use ironrdp_gateway::policy::AuthzDecision;
use ironrdp_gateway::static_policy::StaticFilePolicy;

// Silence `unused_crate_dependencies` for dev-dependencies that belong to the
// still-WIP `listener.rs` integration surface (the lib's own test harness is
// disabled via `[lib] test = false`), so they are not yet referenced by this
// policy-only integration test target.
use anyhow as _;
use futures_util as _;
use ironrdp_rdcleanpath as _;
use rustls_pemfile as _;
use serde as _;
use tokio as _;
use tokio_rustls as _;
use tokio_tungstenite as _;
use toml as _;
use tracing as _;

// ---------------------------------------------------------------------------
// Helper: shared policy fixture used by most tests.
// ---------------------------------------------------------------------------

const POLICY_TOML: &str = r#"
[[rules]]
principal = "admin"
hosts = ["10.0.0.5:3389", "192.168.1."]

[[rules]]
principal = "guest"
hosts = ["10.0.0.5:3389"]

[[rules]]
principal = "*"
hosts = ["10.0.0.99:3389"]
"#;

fn make_policy() -> StaticFilePolicy {
    StaticFilePolicy::from_toml_str(POLICY_TOML).expect("valid policy fixture")
}

// ---------------------------------------------------------------------------
// Allow cases
// ---------------------------------------------------------------------------

#[test]
fn allow_exact_principal_and_host() {
    let p = make_policy();
    let id = mk_identity("admin");
    let tgt = mk_target("10.0.0.5", 3389);
    assert_eq!(
        block_on_authorize(&p, &id, &tgt),
        AuthzDecision::Allow,
        "admin must be allowed to reach 10.0.0.5:3389"
    );
}

#[test]
fn allow_prefix_match() {
    let p = make_policy();
    let id = mk_identity("admin");
    let tgt = mk_target("192.168.1.42", 3389);
    assert_eq!(
        block_on_authorize(&p, &id, &tgt),
        AuthzDecision::Allow,
        "admin must be allowed via prefix rule"
    );
}

#[test]
fn allow_wildcard_principal() {
    let p = make_policy();
    let id = mk_identity("completely_unknown_user");
    let tgt = mk_target("10.0.0.99", 3389);
    assert_eq!(
        block_on_authorize(&p, &id, &tgt),
        AuthzDecision::Allow,
        "wildcard rule must allow any principal to reach 10.0.0.99:3389"
    );
}

// ---------------------------------------------------------------------------
// Deny cases
// ---------------------------------------------------------------------------

#[test]
fn deny_no_matching_rule() {
    let p = make_policy();
    let id = mk_identity("nobody");
    let tgt = mk_target("10.0.0.1", 3389);
    assert_eq!(
        block_on_authorize(&p, &id, &tgt),
        AuthzDecision::Deny,
        "unknown host must be denied"
    );
}

#[test]
fn deny_wrong_principal() {
    let p = make_policy();
    // guest is restricted to 10.0.0.5:3389 only.
    let id = mk_identity("guest");
    let tgt = mk_target("192.168.1.42", 3389);
    assert_eq!(
        block_on_authorize(&p, &id, &tgt),
        AuthzDecision::Deny,
        "guest must be denied access to admin-only prefix"
    );
}

#[test]
fn deny_wildcard_wrong_host() {
    let p = make_policy();
    // Wildcard rule only covers 10.0.0.99:3389, not 10.0.0.5.
    let id = mk_identity("random");
    let tgt = mk_target("10.0.0.5", 3389);
    assert_eq!(
        block_on_authorize(&p, &id, &tgt),
        AuthzDecision::Deny,
        "wildcard should not grant access beyond its host list"
    );
}

// ---------------------------------------------------------------------------
// Port matching
// ---------------------------------------------------------------------------

#[test]
fn deny_wrong_port() {
    let p = make_policy();
    // Rule specifies :3389 explicitly; a different port must not match.
    let id = mk_identity("guest");
    let tgt = mk_target("10.0.0.5", 3390);
    assert_eq!(
        block_on_authorize(&p, &id, &tgt),
        AuthzDecision::Deny,
        "wrong port must not match"
    );
}

#[test]
fn bare_hostname_matches_via_prefix() {
    // A policy entry without a port (e.g. "myhost.internal") becomes a
    // Prefix pattern that matches "myhost.internal:<any-port>".
    // This is intentional — callers that need port-specific exact matching
    // should write "myhost.internal:3389" explicitly.
    let toml = r#"
[[rules]]
principal = "svc"
hosts = ["myhost.internal"]
"#;
    let p = StaticFilePolicy::from_toml_str(toml).unwrap();
    let id = mk_identity("svc");
    assert_eq!(
        block_on_authorize(&p, &id, &mk_target("myhost.internal", 3389)),
        AuthzDecision::Allow,
        "bare hostname must match via prefix"
    );
    assert_eq!(
        block_on_authorize(&p, &id, &mk_target("myhost.internal", 3390)),
        AuthzDecision::Allow,
        "bare hostname prefix matches any port"
    );
    assert_eq!(
        block_on_authorize(&p, &id, &mk_target("other.host", 3389)),
        AuthzDecision::Deny,
        "non-matching host must be denied"
    );
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn empty_policy_denies_everything() {
    let p = StaticFilePolicy::from_toml_str("").unwrap();
    let id = mk_identity("admin");
    let tgt = mk_target("10.0.0.1", 3389);
    assert_eq!(
        block_on_authorize(&p, &id, &tgt),
        AuthzDecision::Deny,
        "empty policy must deny all"
    );
}

#[test]
fn malformed_toml_returns_error() {
    let result = StaticFilePolicy::from_toml_str("this is not valid toml = [[[");
    assert!(result.is_err(), "malformed TOML must return an error");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

use core::future::Future as _;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::pin::pin;

use ironrdp_gateway::auth::Identity;
use ironrdp_gateway::policy::{GatewayPolicy as _, TargetHost};

fn mk_identity(principal: &str) -> Identity {
    Identity {
        principal: principal.to_owned(),
        claims: Default::default(),
    }
}

fn mk_target(host: &str, port: u16) -> TargetHost {
    TargetHost {
        host: host.to_owned(),
        port,
    }
}

/// Poll a `future::ready`-backed authorize future to completion synchronously.
///
/// `StaticFilePolicy::authorize` wraps a sync decision in `future::ready`, so
/// a single poll call always yields `Poll::Ready`.
fn block_on_authorize(policy: &StaticFilePolicy, id: &Identity, tgt: &TargetHost) -> AuthzDecision {
    fn no_op_waker() -> Waker {
        fn no_op(_: *const ()) {}
        fn clone(data: *const ()) -> RawWaker {
            RawWaker::new(data, &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        // SAFETY: vtable operations are all no-ops; the pointer is never dereferenced.
        unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
    }

    let waker = no_op_waker();
    let mut cx = Context::from_waker(&waker);
    let mut fut = pin!(policy.authorize(id, tgt));
    #[expect(
        clippy::panic,
        reason = "test invariant: authorize wraps a sync decision in future::ready, so the first poll is always Ready"
    )]
    let Poll::Ready(result) = fut.as_mut().poll(&mut cx) else {
        panic!("StaticFilePolicy::authorize must resolve immediately (uses future::ready)");
    };
    result.expect("authorize returned an error")
}
