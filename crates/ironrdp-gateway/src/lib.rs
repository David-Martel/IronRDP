//! RDCleanPath-compatible RDP gateway.
//!
//! This crate provides the scaffolding for an RDP relay service that accepts
//! HTTPS/WSS connections from RDP clients, authenticates and authorizes them,
//! then relays traffic between the client and the target RDP host.
//!
//! # Architecture
//!
//! The gateway is organized into three planes:
//!
//! - **Control plane** ([`config`]): static configuration, listen addresses, TLS paths.
//! - **Auth/policy plane** ([`auth`], [`policy`]): trait-based authenticator and
//!   authorization policy, intentionally decoupled from any specific backend.
//! - **Data plane** ([`relay`], [`session`]): byte-level relay between the client
//!   and host TCP legs, plus per-session metadata tracking.
//!
//! The first gateway protocol is [`ironrdp_rdcleanpath`], which defines the
//! PDU framing exchanged over the initial HTTPS/WSS upgrade before raw RDP
//! bytes begin flowing.

// Re-export the PDU crate so callers building the handshake do not need a
// separate direct dependency.
pub use ironrdp_rdcleanpath;

pub mod auth;
pub mod config;
pub mod policy;
pub mod relay;
pub mod session;
