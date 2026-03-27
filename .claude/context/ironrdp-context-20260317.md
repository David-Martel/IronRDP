# IronRDP Session Context — 2026-03-17

## Project State

- **Branch:** `windows-server-only` @ `078d4020`
- **Type:** Rust (Windows-native RDP client/server fork)
- **Root:** `C:\codedev\IronRDP`
- **Tests:** 27/27 pass (`cargo test -p ironrdp-client --lib`)
- **Build:** Clean (`cargo check -p ironrdp-client` + `--features egfx`)

## Summary

Comprehensive session covering resize crash fix, audio fix, frame pacing, dirty-rectangle optimization, GPU/EGFX strategy analysis, build pipeline modernization, and Hyper-V e2e validation. All work validated against live Hyper-V Windows Server 2025 guest. 14 distinct implementations delivered across 4 phases using 15+ specialist agents coordinated via Redis-backed agent-bus.

## Recent Commits (this session's work)

```
078d4020 build: improve client packaging and hyper-v lab automation
c4251b2e test: move hyper-v lab guest control to winrm
2ceec05f fix: improve client present and audio behavior
6e657806 test: improve hyper-v diagnosis reporting
776f2c17 test: add hyper-v scenario health reporting
395ef3b0 runtime: preserve fast-path state across reactivation
```

## Agent Work Registry

| Agent | Task | Files | Status |
|-------|------|-------|--------|
| rust-pro | Decompressor reactivation fix | session_driver.rs | Complete |
| rust-pro | Frame pacing (4ms timer) | session_driver.rs | Complete |
| rust-pro | Decompressor regression test | session_driver.rs | Complete |
| rust-pro | Audio stream.play() fix | cpal.rs | Complete |
| rust-pro | PointerPosition physical fix | app.rs | Complete |
| rust-pro | Dirty rectangle forwarding | session_driver.rs | Complete |
| rust-pro | Dynamic window title | app.rs, main.rs | Complete |
| rust-pro | Fullscreen toggle | app.rs | Complete |
| rust-pro | version.rs + build.rs | New files | Complete |
| rust-pro | EGFX capability experiment | rdp.rs, config.rs, Cargo.toml | Complete |
| rust-pro | SIMD RGBA conversion | presentation.rs | Complete |
| performance-engineer | Frame overwrite analysis | Read-only analysis | Complete |
| performance-engineer | GPU backend design | Read-only analysis | Complete |
| backend-architect | GPU/EGFX strategy review | Read-only analysis | Complete |
| deployment-engineer | Build/deploy tooling review | Read-only analysis | Complete |
| frontend-developer | Usability/feature parity review | Read-only analysis | Complete |
| test-automator | Session seam test gap analysis | Read-only analysis | Complete |
| powershell-pro | Hyper-V suite gap proposals | Read-only analysis | Complete |
| powershell-pro | Auto-deploy build mode | build.ps1 | Complete |
| powershell-pro | Remote deploy script | Deploy-IronRdpRemote.ps1 | Complete |
| powershell-pro | Doctor validation report | build.ps1 | Complete |
| code-reviewer | Decompressor fix review | Read-only analysis | Complete |
| architect-reviewer | P0-P2 priority mapping | Read-only analysis | Complete |

## Key Decisions

### D1: Preserve decompressor across reactivation (not fresh)
- **Decision:** Use `reactivate_fastpath_processor()` to preserve compression history
- **Rationale:** Windows Server 2025 does NOT reset its compressor on reactivation despite MS-RDPBCGR §3.1.5.5
- **Evidence:** Fresh decompressor causes "XCRUSH L1: match output offset out of order" crash

### D2: Direct2D over wgpu for GPU backend
- **Decision:** Recommend Direct2D/DXGI for the Windows-only fork
- **Rationale:** Zero new dependencies (uses existing `windows` crate), ~80 lines unsafe COM vs ~200 lines safe wgpu + shader + 4MB compile
- **Status:** Design complete, not yet implemented

### D3: Dirty rectangles are the #1 optimization
- **Decision:** Forward GraphicsUpdate(InclusiveRectangle) through to partial frame copy
- **Rationale:** copy_rgba_frame was unconditionally copying 7.9 MB/frame at 1080p (~475 MB/s wasted)
- **Status:** Implemented and validated

### D4: EGFX client is a no-op stub; server is complete
- **Decision:** First step is capability experiment (advertise AVC420), not full implementation
- **Rationale:** Need to observe whether server switches from bitmap to EGFX before investing in H.264 decode
- **Status:** --egfx flag implemented, awaiting live test

## Hyper-V Validation Baseline

| Metric | Baseline | Resize | Outage |
|--------|----------|--------|--------|
| Status | session-rendering | session-rendering | session-rendering |
| Frames | 453 | 326 | 145 |
| Overwritten | 0 | 0 | 0 |
| Audio | playback-observed | playback-observed | playback-observed |
| Clipboard | client-handled | client-handled | client-handled |
| Audio underruns | 9 | 2537 | 2832 |
| Health | PASSED | PASSED | PASSED |

## Roadmap

### Immediate
- Audio buffer sizing tuning (P2.5) — underruns need fixing
- EGFX experiment live test — run with --egfx, observe server behavior
- dtm-p1gen7 mirroring — Deploy-IronRdpRemote.ps1 ready

### This Week
- Clipboard e2e text transfer validation
- Direct2D presentation backend prototype
- Session seam integration tests in testsuite-extra

### Tech Debt
- FFI layer has same bulk_decompressor: None bug (ffi/src/session/mod.rs:178)
- Duplicated to_bulk_compression_type helper (session_driver.rs + active_stage.rs)
- Audio format compatibility (sample rate validation against device)

### Performance TODOs
- Direct2D backend eliminates RGBA→RGB conversion (60-70% frame reduction)
- Dirty-region partial GPU uploads (90% reduction for incremental updates)
- 16-bpp→RGBA SIMD-accelerated expand in fast_path.rs
