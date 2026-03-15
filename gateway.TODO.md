# Gateway TODO

This file tracks a separate gateway/control-plane line of work for the
Windows-native `windows-server-only` fork. It is intentionally separated from
the main client/server/runtime roadmap because direct machine-to-machine RDP
quality, GPU/render acceleration, multitransport, and reconnect stability are
currently higher priority.

## Current conclusion

- A gateway is useful, but it is not the next critical path.
- The shortest-path implementation for this fork is an RDCleanPath-compatible
  gateway service, not a full Microsoft RD Gateway clone.
- The local UDMPRO RADIUS service can reasonably be used as the gateway's user
  authentication backend.
- Tailscale/Headscale should stay optional for control-plane reachability or
  NAT fallback, not the default RDP data plane.

## Reusable code already in this repo

- [crates/ironrdp-mstsgu](C:/codedev/IronRDP/crates/ironrdp-mstsgu/src/lib.rs)
  contains a client-side MS-TSGU transport implementation.
- [crates/ironrdp-rdcleanpath](C:/codedev/IronRDP/crates/ironrdp-rdcleanpath/src/lib.rs)
  contains the RDCleanPath request/response/error PDU model already used by the
  client and related gateway integrations.
- [crates/ironrdp-client/src/rdp.rs](C:/codedev/IronRDP/crates/ironrdp-client/src/rdp.rs)
  already supports direct TCP, `ironrdp-mstsgu`, and RDCleanPath/WebSocket
  connection paths.
- [crates/ironrdp-acceptor](C:/codedev/IronRDP/crates/ironrdp-acceptor/src/lib.rs)
  is useful if deeper mediation of the RDP bootstrap/authentication path is
  needed later.

## Constraints and findings

- `ironrdp-mstsgu` is explicitly MVP-only:
  - HTTPS/WebSocket only
  - no legacy HTTP-RPC
  - no UDP transport
  - no reconnection/reauthentication
  - basic auth only
  Ref: [crates/ironrdp-mstsgu/README.md](C:/codedev/IronRDP/crates/ironrdp-mstsgu/README.md)
- RDCleanPath is already a better fit for this fork's gateway work than
  implementing full RD Gateway parity first.
- The gateway should authenticate and authorize, but the host should still
  perform normal RDP NLA/CredSSP logon for the desktop session.
- The current client TLS path still needs stricter host identity validation
  before a hardened gateway story should be called production-ready.

## External integration assumptions

- The local UniFi UDMPRO at `192.168.1.1` can plausibly serve as a RADIUS
  backend for gateway user authentication.
- Public gateway exposure can use the existing DDNS/public IP plus a valid
  Cloudflare-provisioned certificate.
- Tailscale/Headscale remain optional:
  - host discovery
  - admin/control-plane access
  - NAT traversal fallback
  - not the preferred first data plane for normal RDP sessions

## Recommended architecture

```text
Internet client
  -> HTTPS/WSS to IronRDP Gateway
  -> Gateway authenticates user via RADIUS
  -> Gateway authorizes requested host via local policy
  -> Gateway relays RDP traffic to host
  -> Host still performs CredSSP/NLA logon

Optional later:
Host connector/agent
  -> outbound WSS to Gateway
  -> enables NAT/firewall traversal when direct host reachability is unavailable
```

## Checklist

- [ ] Keep gateway work off the primary critical path until direct Windows
      client/server quality goals are met.
- [ ] Design a new `ironrdp-gateway` crate with clean separation between:
      control plane, auth/policy, and data plane relay.
- [ ] Reuse `ironrdp-rdcleanpath` as the first gateway protocol.
- [ ] Implement gateway-side TLS termination on `443`.
- [ ] Implement RADIUS client auth against `192.168.1.1`.
- [ ] Define local authorization policy:
      user/group -> allowed hosts.
- [ ] Issue short-lived internal session tokens after successful auth.
- [ ] Relay raw RDP bytes between client leg and host leg.
- [ ] Preserve host-side CredSSP/NLA as the desktop-session auth boundary.
- [ ] Add audit logging and RADIUS accounting start/stop/interim updates.
- [ ] Add session inventory, idle timeout, and concurrency controls.
- [ ] Add host-connector mode only if NAT traversal is actually required.
- [ ] Revisit richer IdP integration only if RADIUS becomes too limiting.
- [ ] Revisit full MS-TSGU server compatibility only after RDCleanPath gateway
      is stable and useful.

## Preliminary implementation plan

### Phase 1: Minimal useful gateway

- New crate: `crates/ironrdp-gateway`
- HTTPS/WSS listener
- RDCleanPath request parsing and response generation
- RADIUS auth
- local static policy file
- relay to reachable internal hosts
- audit log for who connected to what

### Phase 2: Operational maturity

- short-lived signed gateway session tokens
- idle/session timeout enforcement
- session inventory and admin visibility
- RADIUS accounting
- structured metrics

### Phase 3: Border traversal

- optional host agent/connector
- reverse tunnel for hosts not reachable from the gateway
- host registration and health model

### Phase 4: Feature expansion

- richer IdP support
- policy service instead of static file
- optional MS-TSGU server-compatibility track
- optional UDP/QUIC transport experiments

## References

- Repo:
  - [crates/ironrdp-mstsgu/README.md](C:/codedev/IronRDP/crates/ironrdp-mstsgu/README.md)
  - [crates/ironrdp-mstsgu/src/lib.rs](C:/codedev/IronRDP/crates/ironrdp-mstsgu/src/lib.rs)
  - [crates/ironrdp-rdcleanpath/README.md](C:/codedev/IronRDP/crates/ironrdp-rdcleanpath/README.md)
  - [crates/ironrdp-rdcleanpath/src/lib.rs](C:/codedev/IronRDP/crates/ironrdp-rdcleanpath/src/lib.rs)
  - [crates/ironrdp-client/src/rdp.rs](C:/codedev/IronRDP/crates/ironrdp-client/src/rdp.rs)
  - [ARCHITECTURE.md](C:/codedev/IronRDP/ARCHITECTURE.md)
- External:
  - UniFi RADIUS: https://help.ui.com/hc/en-us/articles/360015268353-Configuring-a-RADIUS-Server-in-UniFi
  - Tailscale overview: https://tailscale.com/docs/concepts/what-is-tailscale
  - Tailscale routing behavior: https://tailscale.com/docs/concepts/traffic-routing-through-tailscale
  - Headscale TLS: https://headscale.net/stable/ref/tls/
  - Headscale OIDC: https://headscale.net/stable/ref/oidc/
