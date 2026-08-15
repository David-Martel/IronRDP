# IronRDP interop with gnome-remote-desktop (GRD) 46 — NLA + Graphics Pipeline

Status: draft note (evidence-based). Not a PR.

## Problem this solves

`gnome-remote-desktop` (GRD) 46.x on Ubuntu 24.04 (host `asuspro13`, tailnet
`100.64.0.3:3389`, NLA required) rejects **every FreeRDP-based Windows client**
during CredSSP with a server-side:

```
[credssp_auth_decrypt]: Encrypted message buffer too small
[transport_accept_nla]:  client authentication failure
```

and rejects Windows `mstsc` with an NTLM MIC failure (`SEC_E_MESSAGE_ALTERED`).
A Linux `xfreerdp3` running *on* the host (localhost) authenticates fine, so the
credential and the server are good — the failure is specific to Windows clients
whose CredSSP/NTLM is implemented by winpr.

IronRDP uses a clean-room Rust CredSSP/NTLM stack (`sspi-rs`), a different code
path. This note records that IronRDP **interoperates where FreeRDP does not**,
and the one small change needed to get from "authenticated" to "session
established" against GRD.

## Findings (both sides of the wire)

### 1. Authentication (NLA / CredSSP): works with zero source changes

Running the stock `ironrdp-client` (NLA on by default) to the GRD host:

- Client reaches capability exchange, then errors
  `unexpected Share Control Pdu (expected ServerDemandActive)`.
- Server journal (`journalctl -u gnome-remote-desktop`) shows the connection
  reaching `CONNECTION_STATE_CAPABILITIES_EXCHANGE_DEMAND_ACTIVE` with **no**
  `credssp_auth_decrypt` / `client authentication failure` / MIC errors.

Reaching `DEMAND_ACTIVE` is only possible *after* CredSSP completes (RDP
sequence: X.224 → CredSSP → MCS → channel join → capabilities exchange). So
IronRDP's NLA succeeds against GRD where FreeRDP fails four phases earlier.

### 2. Session establishment: GRD 46 mandates the Graphics Pipeline

GRD is EGFX-only and does not fall back to legacy bitmap updates. Its FreeRDP
server backend closes the connection at Demand Active with:

```
[RDP] Client did not advertise support for the Graphics Pipeline, closing connection
[CONNECTION_STATE_CAPABILITIES_EXCHANGE_DEMAND_ACTIVE] freerdp_peer::Capabilities() callback failed
```

FreeRDP's server sets `SupportGraphicsPipeline` from the client's
`RNS_UD_CS_SUPPORT_DYNVC_GFX_PROTOCOL` (0x0100) bit in `earlyCapabilityFlags` of
the Client Core Data (`TS_UD_CS_CORE`, MS-RDPBCGR 2.2.1.3.2). IronRDP's
connector never set this bit — even with the EGFX DVC handler registered
(`--egfx`), the flag was absent, so GRD rejected the session before it started.

## The change

`crates/ironrdp-connector`:
- New `Config::enable_graphics_pipeline: bool`.
- When set, `create_gcc_blocks()` ORs `SUPPORT_DYN_VC_GFX_PROTOCOL` into the
  client's `earlyCapabilityFlags`.

`crates/ironrdp-client`:
- Wires `enable_graphics_pipeline = args.egfx`, so the flag is advertised only
  when the client is built (`--features egfx`) and run (`--egfx`) to actually
  service EGFX traffic. Advertising it without an EGFX handler would leave an
  EGFX-only server sending frames the client cannot render (blank session).

Commit: `feat(connector): advertise Graphics Pipeline early-capability flag`
(branch `feat/gfx-early-capability-flag`).

## Evidence after the change

`ironrdp-client --egfx` (built `--features egfx`) to the GRD host now produces,
server-side:

```
[RDP] Sending server redirection
[DaemonSystem] RDP client disconnected during the handover
```

The connection reaches **handover** and **server redirection** — GRD's success
path that hands the client from the login-screen/system daemon to the user
session. No auth errors, no "Graphics Pipeline" rejection. Client-side the
session is active (`Active session error: [decode error]`) and only drops
because this build has `egfx` but not `openh264`, so it cannot decode GRD's
H.264/AVC420 EGFX frames.

## To get a rendered session

Build with H.264 decode: `cargo build --release -p ironrdp-client --features openh264`
(pulls `openh264-bundled`; requires `nasm` at build time). Then run with
`--egfx`. GRD emits AVC420 over EGFX; `openh264` decodes it. Without a decoder
the access/auth problem is still solved, but no pixels are drawn.

## Client flags that matter (summary)

- NLA/CredSSP: **on by default** (no flag). `--no-credssp` disables it.
- `--egfx`: register EGFX DVC + advertise Graphics Pipeline (with this change).
  Requires `--features egfx` at build.
- `--features openh264`: decode GRD's H.264 frames for an actual picture.

## Server side (GRD / Ubuntu) — no changes required

GRD 46.3, RDP backend enabled (`grdctl status`), listening on 3389, TLS cert
auto-generated. Credentials authenticate via the system/PAM handover path
(`grdctl` credential stores report empty; the RDP password is the user's system
password). No `grdctl` or config change was needed — the fix is entirely
client-side.
