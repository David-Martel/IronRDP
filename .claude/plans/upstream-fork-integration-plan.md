# Upstream Fork Integration Plan

Created: 2026-03-27
Status: proposed
Branch: master (David-Martel/IronRDP)

## Overview

This plan maps worthwhile upstream fork contributions to three integration
tracks: **device redirection**, **graphics acceleration**, and **authentication
/ gateway**. Each track is organized into phases with clear dependencies,
conflict risks, and the specific upstream commits or PRs to integrate.

## Already Integrated

| Source | What | Status |
|--------|------|--------|
| elmarco | RLGR encoder fixes (3 commits) | Merged |
| JuSiZeLa | Smart card PDU improvements (3 commits) | Merged |
| Devolutions upstream | 13 cherry-picks (EGFX client, pixel format, autodetect, etc.) | Merged |

## Integration Strategy

**Manual port > cherry-pick** for diverged forks. The glamberson, formalco,
and gabrielbauman forks are all based on older upstream states and cherry-pick
attempts produce 5+ file conflicts. The right approach is:

1. Read the PR diff on GitHub
2. Understand the design and API changes
3. Manually port the logic onto our current codebase
4. Preserve our fork's conventions (pub(crate), SessionGuard, etc.)
5. Test integration with Hyper-V suite

---

## Track 1: Device Redirection

### Current state
- `ironrdp-rdpdr` has `RdpdrBackend` trait with `handle_drive_io_request`,
  `handle_scard_call`, `handle_server_device_announce_response`
- Client uses `NoopRdpdrBackend` — all device redirection is explicitly unsupported
- `ironrdp-rdpeusb` is a stub crate (54 bytes, just `lib.rs`)
- Smart card PDU structures are now improved (JuSiZeLa integration done)
- `ironrdp-rdpdr-native` exists with a Windows-native backend skeleton

### Phase 1A: Drive redirection backend (Priority: HIGH)
**Goal:** Allow the client to expose a local folder as a redirected drive.

**Upstream source:** None — this needs original implementation.

**Work:**
- Implement `RdpdrBackend::handle_drive_io_request()` in `ironrdp-rdpdr-native`
  to handle `IRP_MJ_CREATE`, `IRP_MJ_READ`, `IRP_MJ_WRITE`, `IRP_MJ_CLOSE`,
  `IRP_MJ_QUERY_INFORMATION`, `IRP_MJ_DIRECTORY_CONTROL`
- Add `--redirect-drive <name>=<path>` CLI flag to `ironrdp-client`
- Wire device announcement during connection setup
- Hyper-V validation: copy a file from host to guest via redirected drive

**Risk:** Medium — the RDPDR protocol state machine is complex but well-documented
in MS-RDPEFS. The backend trait already exists; this is implementation work.

**Effort:** Large (2-3 sessions)

### Phase 1B: Clipboard file transfer (Priority: MEDIUM)
**Goal:** Extend clipboard beyond text to support file copy/paste.

**Upstream source:** PR #1166 (gabrielbauman, 93 files, 17k lines)
- Too large and diverged for cherry-pick
- Key API additions: `request_file_contents()`, `SendFileContentsResponse`,
  clipboard data locking
- Also includes web and FFI surface changes we don't need

**Integration approach:**
- Port only the `ironrdp-cliprdr` backend additions (data locking, file contents)
- Skip web/FFI changes
- Implement the native backend side using Windows clipboard APIs
- Reuse our existing `ironrdp-cliprdr-native` crate

**Effort:** Medium (1-2 sessions)

### Phase 1C: USB redirection PDUs (Priority: LOW)
**Goal:** Populate the rdpeusb crate with proper PDU definitions.

**Upstream source:** PR #1165 (playbahn, 13 files, 2134 lines)
- Adds URBDRC PDU definitions per MS-RDPEUSB
- PDU-only — no runtime or backend implementation

**Integration approach:**
- Cherry-pick or manual port — low conflict risk since rdpeusb is a stub
- This is foundation work; actual USB redirection requires a Windows USB
  backend (WinUSB / USBDK) which is a separate effort

**Effort:** Small (1 session for PDUs, large for actual USB backend later)

### Phase 1D: Smart card backend (Priority: LOW)
**Goal:** Wire the now-improved smart card PDUs to Windows smart card APIs.

**Upstream source:** Already integrated (JuSiZeLa PDU improvements)

**Work:**
- Implement `handle_scard_call()` in a Windows-native backend using
  `winscard.dll` / `SCardEstablishContext` APIs
- This enables smart card authentication pass-through to remote sessions

**Effort:** Medium-Large (Windows smart card API is well-documented but verbose)

---

## Track 2: Graphics Acceleration

### Current state
- Client uses `softbuffer` for software rendering (RGBA → packed pixel conversion)
- EGFX client-side surface management and AVC420 decode merged from upstream
- openh264 feature-gated H.264 decoder available (feature `openh264`)
- Server has EGFX pipeline with AVC420/444 + ZGFX compression
- Presentation backend seam exists (`presentation.rs`) for GPU experiments
- 16-bpp RLE bitmap streams still dominate Hyper-V traffic

### Phase 2A: Enable H.264 decode in client (Priority: HIGH)
**Goal:** Wire openh264 decoder to the EGFX client pipeline so the
LoggingEgfxHandler becomes a rendering handler.

**Upstream source:** Already integrated (commits b6200c7a + 5e316bba)

**Work:**
- Enable `openh264` feature in `ironrdp-client/Cargo.toml`
- Create an `EgfxRenderHandler` that implements `GraphicsPipelineHandler`
  with `on_bitmap_updated` forwarding decoded frames to the presentation layer
- Instantiate `GraphicsPipelineClient::new(handler, Some(decoder))` instead
  of `(handler, None)`
- Feed EGFX surface updates into the existing `DecodedImage` → frame buffer path
- Hyper-V validation: run with `--egfx` and observe server codec switch

**Depends on:** Nothing — all code is already in the repo
**Effort:** Medium (1 session)

### Phase 2B: ClearCodec client decode (Priority: MEDIUM)
**Goal:** Decode ClearCodec bitmap data sent by Windows Server for
text/UI regions alongside RLE and H.264.

**Upstream source:** PRs #1174 + #1175 (glamberson, ~5600 lines total)
- #1174: ClearCodec codec implementation in `ironrdp-graphics` + PDU additions
- #1175: Client-side EGFX dispatch for ClearCodec frames

**Integration approach:**
- Port the `ClearCodec` decoder from #1174 into `ironrdp-graphics`
- Port the EGFX client dispatch additions from #1175
- Both are additive — new files and new match arms, minimal conflict

**Effort:** Medium (1-2 sessions)

### Phase 2C: Uncompressed frame path for V8 clients (Priority: LOW)
**Goal:** Support EGFX V8 uncompressed bitmap frames for clients
that don't negotiate compression.

**Upstream source:** PR #1182 (glamberson, 2 files, 100 lines)
- Small, focused addition to server-side EGFX
- Low conflict risk

**Integration approach:** Manual port (small enough to do inline)
**Effort:** Small

### Phase 2D: ZGFX compression optimization (Priority: MEDIUM)
**Goal:** Improve server-side ZGFX compression performance with O(1) hash.

**Upstream source:** glamberson fork (commits a0eacc50, 4a93ffae, 57608dad)
- Too diverged for cherry-pick — ZGFX module was restructured
- Key optimization: replace linear hash scan with hash table

**Integration approach:**
- Read the hash table implementation from the fork
- Port the optimization into our current `ironrdp-graphics/src/zgfx/` layout
- Also port the duplicate-entry fix and size limits

**Effort:** Medium (1 session — algorithmic port, not line-by-line merge)

### Phase 2E: Slow-path graphics handling (Priority: LOW)
**Goal:** Handle slow-path (non-FastPath) graphics and pointer updates.

**Upstream source:** PR #1132 (glamberson, 8 files, 595 lines)
- Adds ShareDataPdu handling for bitmap updates, pointer updates
- Relevant for servers that don't support FastPath

**Integration approach:** Manual port
**Effort:** Medium

### Phase 2F: Direct2D presentation backend (Priority: MEDIUM)
**Goal:** Replace softbuffer with Direct2D for zero-copy Windows rendering.

**Upstream source:** None — fork-specific work

**Work:**
- Implement `PresentationBackend` trait using `ID2D1HwndRenderTarget`
- RGBA → BGRA conversion in D2D bitmap, or negotiate BGRA pixel format
- Eliminate the `softbuffer` conversion step
- This is the biggest single present-path win after dirty rectangles

**Effort:** Medium-Large (1-2 sessions)

---

## Track 3: Authentication & Gateway

### Current state
- `ironrdp-gateway` crate scaffolded with trait-based architecture
- `GatewayAuthenticator`, `GatewayPolicy`, `GatewayRelay` traits defined
- `ironrdp-rdcleanpath` integrated for PDU framing
- Acceptor has `creds: Option<Credentials>` with None-skip fix
- CredSSP/NLA works with static credentials
- No dynamic credential provider yet (formalco sspi upgrade blocked by rand_core)

### Phase 3A: CredentialValidator trait for server (Priority: HIGH)
**Goal:** Allow the server to validate credentials dynamically instead
of comparing against a static `Option<Credentials>`.

**Upstream source:** PR #1172 (glamberson, 1 file, 49 lines)
- Adds `CredentialValidator` trait to `ironrdp-acceptor`
- Clean, tiny PR — single file addition with no conflicts

**Integration approach:**
- Manual port — 49 lines, additive
- Wire into `RdpServer` builder
- This is a prerequisite for the gateway's auth story

**Relation to gateway.TODO.md:** Directly enables the gateway to validate
credentials per-connection using RADIUS or any async backend.

**Effort:** Small (30 minutes)

### Phase 3B: Dynamic credential provider (Priority: HIGH)
**Goal:** Allow CredSSP/NLA path to resolve credentials at runtime.

**Upstream source:** formalco fork (commit 1993dad4)
- The code changes are correct but the sspi git rev pins picky 7.0.0-rc.22
  which breaks rand_core resolution
- The `CredentialProvider` trait and acceptor wiring are independently useful

**Integration approach:**
- Port the `CredentialProvider` trait definition and `set_credential_provider()`
  method from the formalco code
- Port the CredSSP server-side credential resolution changes
- Do NOT take the sspi version upgrade — wait for sspi to publish a stable release
  compatible with current picky/rand_core versions
- This works alongside 3A: `CredentialValidator` for TLS-mode,
  `CredentialProvider` for CredSSP/NLA-mode

**Relation to gateway.TODO.md:** Enables the gateway to provide credentials
dynamically during CredSSP negotiation, supporting RADIUS-backed auth.

**Effort:** Medium (need to port code without the sspi upgrade)

### Phase 3C: NTLM fallback when Kerberos unavailable (Priority: MEDIUM)
**Goal:** Server-side NLA gracefully falls back to NTLM in
environments without Kerberos/domain controllers.

**Upstream source:** formalco (commit 7cf649fd) + upstream PR #1143 (ramnes)
- Both attempt the same fix; formalco's is cleaner
- Blocked by sspi version — same rand_core issue as 3B

**Integration approach:**
- Port the NtlmConfig-only server mode setup
- Wait for sspi release or find a compatible rev

**Relation to gateway.TODO.md:** NTLM fallback is essential for the gateway
authenticating standalone Windows hosts without domain membership.

**Effort:** Small (once sspi version is resolved)

### Phase 3D: Gateway RADIUS implementation (Priority: HIGH)
**Goal:** Implement `GatewayAuthenticator` backed by RADIUS against UDMPRO.

**Upstream source:** None — fork-specific work

**Work:**
- Add `radius-client` crate dependency to `ironrdp-gateway`
- Implement `GatewayAuthenticator` for RADIUS using Access-Request/Accept/Reject
- Configure against `192.168.1.1` (local UDMPRO)
- Add RADIUS accounting (start/stop/interim-update) for audit trail

**Depends on:** Phase 3A (CredentialValidator for TLS-mode auth bridging)
**Effort:** Medium (1 session)

### Phase 3E: Gateway HTTPS/WSS listener (Priority: HIGH)
**Goal:** Implement the gateway listener that terminates TLS and speaks
RDCleanPath over WebSocket.

**Upstream source:** PR #855 (irvingoujAtDevolution, 31 files, 1533 lines)
- Exposes RDCleanPath API via FFI
- Some design patterns are reusable but the PR is FFI-focused

**Integration approach:**
- Use `tokio-tungstenite` for WebSocket
- Use `tokio-rustls` for TLS termination (already a dependency)
- Parse `RDCleanPathPdu` requests using existing `ironrdp-rdcleanpath`
- Connect to target host and start `GatewayRelay`
- This is original work informed by the PR's design patterns

**Effort:** Medium-Large (1-2 sessions)

### Phase 3F: Auto-Detect RTT for transport quality (Priority: LOW)
**Goal:** Use auto-detect PDUs for round-trip time measurement and
bandwidth estimation between client and server/gateway.

**Upstream source:** PRs #1177 + #1178 (glamberson)
- #1177: Server-side auto-detect RTT measurement (379 lines)
- #1178: Client-side auto-detect PDU handling (285 lines)
- Both depend on the autodetect PDU types we already integrated

**Integration approach:** Manual port — both are focused additions
**Effort:** Small-Medium (1 session for both)

---

## Recommended Execution Order

### Sprint 1: Auth foundations + H.264 (immediate value)
1. **3A** CredentialValidator trait (30 min, unblocks gateway auth)
2. **2A** Enable H.264 decode in client (1 session, biggest visual improvement)
3. **3B** Dynamic credential provider (medium, port without sspi upgrade)

### Sprint 2: Device redirection start + EGFX pipeline
4. **1A** Drive redirection backend (2-3 sessions, user-visible feature)
5. **2B** ClearCodec client decode (1-2 sessions, rendering quality)
6. **1C** USB redirection PDUs (1 session, foundation work)

### Sprint 3: Gateway implementation
7. **3D** Gateway RADIUS auth (1 session, depends on 3A)
8. **3E** Gateway HTTPS/WSS listener (1-2 sessions)
9. **3F** Auto-Detect RTT (1 session, transport quality)

### Sprint 4: Performance + polish
10. **2D** ZGFX compression optimization (1 session, server perf)
11. **2F** Direct2D presentation backend (1-2 sessions, client perf)
12. **1B** Clipboard file transfer (1-2 sessions)

### Deferred (wait for upstream resolution)
- **3C** NTLM fallback — blocked on sspi/picky rand_core conflict
- **2C** Uncompressed V8 frame path — low priority
- **2E** Slow-path graphics — low priority
- **1D** Smart card backend — needs Windows smart card API work

---

## Conflict Risk Map

| Area | Our changes | Upstream changes | Risk |
|------|------------|-----------------|------|
| `server.rs` | SessionGuard, active_session, generic stream | CredentialValidator, credential_provider | MEDIUM — same struct, additive fields |
| `rdp.rs` | LoggingEgfxHandler, shutdown logging, keyboard layout | Clipboard, drive redirection wiring | LOW — different sections |
| `cliprdr/` | No fork changes | File transfer (gabrielbauman, 17k lines) | HIGH — large PR, many files |
| `ironrdp-egfx/client.rs` | Already updated to new API | ClearCodec dispatch | LOW — additive match arms |
| `ironrdp-acceptor/` | received_credentials, None-skip fix | CredentialValidator, CredentialProvider | MEDIUM — same credential path |
| `ironrdp-rdpdr/` | Smart card PDU improvements | Drive IO handling | LOW — different subsystems |
| `zgfx/` | No fork changes | Hash table restructure | HIGH — module reorganized |

## References

- Upstream PRs: #1172, #1174, #1175, #1177, #1178, #1182, #1166, #1165, #1143, #1132, #1098, #1161, #841, #855
- Fork commits: elmarco (integrated), JuSiZeLa (integrated), glamberson (17 ahead), formalco (2 ahead, sspi blocked)
- codex.TODO.md: Immediate items 1-7, P1.2/P1.4, P2.1-P2.4
- gateway.TODO.md: Phase 1-4 checklist
