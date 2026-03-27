# IronRDP Context — 2026-03-23

## Schema Version
2.0

## Project
- **Name:** IronRDP (windows-server-only fork)
- **Root:** C:\codedev\IronRDP
- **Type:** Rust workspace
- **Branch:** windows-server-only
- **Base commit:** 078d4020
- **Owner:** David-Martel

## State Summary

Completed a 6-task subagent-driven development session that advanced both
`codex.TODO.md` and `gateway.TODO.md`. All 6 tasks were implemented by
parallel Rust specialist subagents, then validated against unit tests
(6/6 integration tests pass), doctests (5/5 server doctests pass), and
the live Hyper-V E2E suite (baseline + resize scenarios: both PASSED,
healthy diagnosis, 0 errors, 0 overwritten frames, audio playback-observed).

### Recent Changes (this session)

| Cluster | Files | Summary |
|---------|-------|---------|
| Server single-session contract | `server.rs`, `README.md` | `active_session: Arc<AtomicBool>` + `SessionGuard` RAII, doc contract, reject+log concurrent connections |
| Bitmap early-fail validation | `encoder/mod.rs` | `ensure!` guards: desktop size 0/<8192, stride vs row width, data length vs stride x height |
| Audio path cleanup | `cpal.rs` | Opus errors -> warn + decode_error_count, silent shutdown, 100ms timeout, silence fill, underrun tracking |
| Reconnect/shutdown clarity | `rdp.rs`, `session_driver.rs`, `app.rs` | info!/error! at 6 exit points, GUI channel drop -> hard disconnect not protocol error |
| Session seam tests | `testsuite-extra/tests/mod.rs` | 3 new tests: graceful_disconnect, display_write_failure, double_reactivation |
| Gateway scaffold | new `crates/ironrdp-gateway/` | auth.rs, policy.rs, relay.rs, session.rs, config.rs — trait-based three-plane architecture |

### Work In Progress
- codex.TODO.md Immediate items 1,2,4,5,6 remain (hardware-dependent or design-phase)
- gateway.TODO.md Phase 1 scaffolding done, implementation not started

### Blockers
- WinRM Negotiate auth to Hyper-V guest fails in some sessions (TrustedHosts formatting issue fixed this session, but credential store may need refresh)
- Build artifacts use prior commit binary (078d4020); new code needs fresh `build.ps1 -Mode package` before next Hyper-V validation

## Decisions

### dec-001: Single-session as explicit contract, not limitation
- **Decision:** Enforce single-session with AtomicBool + SessionGuard, not just serialized accepts
- **Rationale:** Makes the constraint programmatic and visible in logs, prevents silent queueing
- **Decided by:** rust-pro subagent, validated by controller

### dec-002: Gateway uses trait-based auth/policy, not hardcoded RADIUS
- **Decision:** GatewayAuthenticator and GatewayPolicy are async traits; RADIUS is a future impl
- **Rationale:** Keeps gateway core decoupled from backend; RADIUS, LDAP, static tokens all valid impls
- **Decided by:** rust-pro subagent per gateway.TODO.md architecture

### dec-003: Audio Opus errors downgraded to warn, not error
- **Decision:** Opus decode failures are warn! with atomic counter, not error!
- **Rationale:** Shutdown-induced channel drops produced false error! messages; real decoder issues tracked via decode_error_count separate from underrun_count

## Agent Work Registry

| Agent | Task | Files Touched | Status | Handoff |
|-------|------|---------------|--------|---------|
| rust-pro (sonnet) | Single-session contract | server.rs, README.md | Complete | Hyper-V validated |
| rust-pro (sonnet) | Bitmap early-fail | encoder/mod.rs | Complete | Unit tested |
| rust-pro (sonnet) | Audio cleanup | cpal.rs | Complete | Hyper-V validated |
| rust-pro (sonnet) | Reconnect/shutdown | rdp.rs, session_driver.rs, app.rs | Complete | Hyper-V validated |
| rust-pro (sonnet) | Session tests | testsuite-extra/tests/mod.rs | Complete | 6/6 pass |
| rust-pro (sonnet) | Gateway scaffold | ironrdp-gateway/* | Complete | Compiles clean |
| controller (opus) | Coordination, fixes, validation | gateway session.rs, config.rs, auth.rs lint | Complete | All verified |

## Hyper-V Validation Baseline (2026-03-23)

| Metric | Baseline | Resize |
|--------|----------|--------|
| Passed | True | True |
| Status | session-rendering | session-rendering |
| Images | 1419 | 2250 |
| Frames | 1419 | 2252 |
| FPS | 57.8 | 90.5 |
| Overwritten | 0 | 0 |
| Errors | 0 | 0 |
| Clipboard | client-handled | client-handled |
| Audio | playback-observed | playback-observed |
| Diagnosis | healthy | healthy |
| CPU avg | 0.3% | 1.04% |
| Copy avg | 345us | 584us |

Shutdown log trail confirmed clean:
1. `User-initiated disconnect: sending graceful shutdown to server`
2. `Server-initiated graceful disconnect received`
3. `Session terminated gracefully`
4. `Session ended: graceful disconnect`
0 ERROR-level messages across both scenarios.

## Roadmap

### Immediate (codex.TODO.md remaining)
1. Hyper-V suite interaction lab improvements (item 1) — needs lab time
2. Diagnosis threshold tightening (item 2) — needs fresh metrics pass
3. Session seam test coverage expansion (item 3) — PARTIALLY DONE (3 new tests added)
4. dtm-p1gen7 deployment mirror (item 4) — needs remote machine access
5. Audio path post-validation cleanup (item 7) — PARTIALLY DONE (Opus/shutdown fixed)
6. Render/transport optimization plan (item 5) — needs measured data review
7. Acceleration track separation (item 6) — docs/roadmap

### Gateway (gateway.TODO.md)
1. Phase 1 scaffold — DONE
2. HTTPS/WSS listener — next
3. RDCleanPath request parsing — next
4. RADIUS auth impl — next
5. Static policy file — next
6. Relay to internal hosts — next
7. Audit logging — next

### Tech Debt
- `num-derive`/`num-traits` phase-out (workspace policy)
- Excluded legacy crates still not compiling (client-glutin, glutin-renderer, replay-client)
- TrustedHosts formatting in Hyper-V lab tooling
