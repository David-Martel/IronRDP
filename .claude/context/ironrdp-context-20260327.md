# IronRDP Context — 2026-03-27

## Schema Version
2.0

## Project
- **Name:** IronRDP (windows-server-only fork)
- **Root:** C:\codedev\IronRDP
- **Type:** Rust workspace
- **Branch:** windows-server-only (merged into master)
- **Commit:** 5a39b70c
- **Owner:** David-Martel

## State Summary

Both `master` and `windows-server-only` branches are at the same merge commit
`5a39b70c`. The session completed a full cycle: 6-task subagent-driven
development, Hyper-V validation, 7 fork commits, 13 upstream cherry-picks,
merge into master, and tree cleanup. All pushed to David-Martel/IronRDP.

The fork now includes all upstream commits through `a6b41093` plus fork-specific
items 27-45 from codex.TODO.md plus the ironrdp-gateway scaffold.

### Branch State
- `master` = `windows-server-only` = `5a39b70c` (merge commit)
- Both pushed to `david-martel` remote
- `origin` (Devolutions/IronRDP) master at `a6b41093` — fully integrated

### Recent Changes (this session)

**Fork work (items 40-45):**
- Single-session server contract (AtomicBool + SessionGuard RAII)
- Bitmap early-fail validation (desktop size, stride, data length)
- Audio path cleanup (Opus warn + decode_error_count, silent shutdown)
- Reconnect/shutdown logging clarity (6 exit points)
- Session seam integration tests (3 new, 6/6 pass)
- Gateway crate scaffold (auth, policy, relay, session, config)

**Upstream cherry-picks (13 commits):**
- fix(acceptor): credential None check
- fix(rdpsnd): AudioFormat renegotiation in Ready state
- feat(pdu): Auto-Detect PDU types + ShareDataPdu routing
- feat(dvc): DvcChannelListener multi-instance
- feat(egfx): client-side EGFX + AVC420 surface management
- feat(egfx): openh264 H.264 decoder (feature-gated)
- feat(egfx): QoE statistics on server
- feat(server): generic stream type (merged with SessionGuard)
- feat: complete pixel format support for bitmap updates
- test(rdpsnd): client state machine tests
- fix(ffi): clipboard proxy typo
- build(deps): criterion 0.8.1

### Work In Progress
- None — tree is clean, all changes committed and pushed

### Blockers
- WinRM Negotiate auth to Hyper-V guest intermittent (TrustedHosts fixed, credential store may need refresh)
- Build artifacts at T:\RustCache use prior commit binary; fresh build.ps1 needed before next Hyper-V validation

## Decisions

### dec-001: Merge strategy — merge commit, not rebase
- **Decision:** Used merge commit to integrate windows-server-only into master
- **Rationale:** Preserves fork history separately from upstream cherry-picks; both branches now at same SHA
- **Decided by:** user direction

### dec-002: Cherry-pick selection — feature commits, not lockfile bumps
- **Decision:** Cherry-picked 13/17 upstream commits; skipped 4 lockfile-only dep bumps
- **Rationale:** Lockfile conflicts are mechanical; resolved via cargo update --workspace instead
- **Decided by:** analysis agent recommendation

### dec-003: EGFX client adaptation — logging handler updated
- **Decision:** Updated LoggingEgfxHandler to new upstream trait API (on_bitmap_updated, on_frame_complete)
- **Rationale:** Upstream removed handle_pdu; new API has default impls so handler is minimal
- **Decided by:** merge resolution

### dec-004: Generic server stream — keep SessionGuard
- **Decision:** Took upstream's generic S: AsyncRead+AsyncWrite signature but kept fork's SessionGuard body
- **Rationale:** Generic stream enables gateway relay; SessionGuard is fork's single-session contract

## Agent Work Registry

| Agent | Task | Files Touched | Status |
|-------|------|---------------|--------|
| rust-pro (sonnet) | Single-session contract | server.rs, README.md | Complete |
| rust-pro (sonnet) | Bitmap early-fail | encoder/mod.rs | Complete |
| rust-pro (sonnet) | Audio cleanup | cpal.rs | Complete |
| rust-pro (sonnet) | Reconnect/shutdown | rdp.rs, session_driver.rs, app.rs | Complete |
| rust-pro (sonnet) | Session tests | testsuite-extra/tests/mod.rs | Complete |
| rust-pro (sonnet) | Gateway scaffold | ironrdp-gateway/* | Complete |
| rust-pro (sonnet) | Upstream analysis | research only | Complete |
| controller (opus) | Coordination, cherry-picks, merge | all | Complete |

## Hyper-V Validation Baseline (2026-03-23)

| Metric | Baseline | Resize |
|--------|----------|--------|
| Passed | True | True |
| Frames | 1419 | 2252 |
| FPS | 57.8 | 90.5 |
| Overwritten | 0 | 0 |
| Errors | 0 | 0 |
| Clipboard | client-handled | client-handled |
| Audio | playback-observed | playback-observed |
| Diagnosis | healthy | healthy |
| ERROR logs | 0 | 0 |

## Roadmap

### Immediate
1. Session seam test coverage — single-session rejection, decompressor regression integration
2. Audio Opus decode root cause investigation
3. dtm-p1gen7 deployment mirror
4. Gateway Phase 1 implementation — HTTPS/WSS listener, RDCleanPath handling
5. Hyper-V suite interaction lab improvements

### This Week
6. Direct2D presentation backend experiment
7. Diagnosis threshold tightening
8. Render/transport optimization plan from measured data

### Tech Debt
- num-derive/num-traits phase-out (workspace policy)
- Legacy crates still excluded (client-glutin, glutin-renderer, replay-client)
- Unicode/IME e2e Windows validation (P1.4)

## Validation
- **Last validated:** 2026-03-27
- **Workspace check:** clean (cargo check --workspace)
- **Integration tests:** 6/6 pass
- **Hyper-V E2E:** baseline + resize PASSED
- **Is stale:** false
