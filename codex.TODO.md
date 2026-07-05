# Codex TODO

This file tracks David-Martel-owned follow-up work for the Windows-native
`windows-server-only` fork. Upstream IronRDP is now source material, not the
product contract. Priorities below are ordered for:

- Windows-native runtime quality first
- repeatable multi-machine deployment second
- Intel x64 CPU baseline first, optional GPU acceleration second
- GPU/render, multitransport, and reconnect quality ahead of gateway work
- measured reliability/performance wins before speculative feature breadth

## Current product contract

- Fork owner: `David-Martel`
- Branch: `windows-server-only`
- Primary product surfaces:
  - native Rust client: `crates/ironrdp-client`
  - native Rust server skeleton: `crates/ironrdp-server`
  - Windows FFI/.NET surface: `ffi/`, `ffi/dotnet/`
  - Windows build/package entrypoint: `build.ps1`
- Primary deployment target: `dtm-p1gen7`
- Primary validation environment:
  - Intel x64 CPU baseline
  - optional Intel iGPU
  - optional NVIDIA discrete GPU
  - mixed 1 GbE / 10 GbE / virtual NIC / VPN paths
  - fallback multi-agent coordination mailbox: `tmp/agent-ipc/messages.ndjson` until the Redis-backed agent-bus wrapper scripts are restored on this machine

## Platform assumptions

These assumptions are deliberate and should drive implementation choices.

1. CPU baseline: Intel x64 first.
Meaning:
- portable release artifacts must run on normal modern Intel Windows systems
- workstation-only `target-cpu=native` builds remain opt-in
- build/reliability work should assume high core-count Intel hosts are common

2. GPU posture: software render/decode remains the default shipping path.
Meaning:
- Intel iGPU and NVIDIA GPU are acceleration opportunities, not requirements
- the branch must stay usable on CPU-only systems
- GPU work must not become a hidden build/runtime dependency

3. Toolchain posture: MSVC is the default shipping toolchain.
Meaning:
- CargoTools + MSVC + .NET is the primary supported path
- LLVM/lld is a preferred acceleration overlay when installed
- oneAPI and CUDA are optional measured overlays, not required setup

4. Network posture: both LAN and WAN behavior matter.
Meaning:
- this fork is no longer “local workstation only”
- session stability, keepalive, jitter tolerance, reconnect behavior, and packet sizing matter
- demo flows between this machine and `dtm-p1gen7` are a first-class target

5. Packaging posture: portable artifacts must stay distinct from host-tuned artifacts.
Meaning:
- portable `win-x64` builds should remain conservative
- host-tuned builds may use `NativeCpu` and machine-local toolchain advantages
- manifests and docs must keep the two classes explicit

## Recently completed

1. Platform-specific CI/build drift cleanup.
Refs: `xtask/src/main.rs`, `xtask/src/cov.rs`, `xtask/src/check.rs`, `ARCHITECTURE.md`, `Cargo.toml`.
Status: done.

2. Excluded crates were reclassified as legacy/unmaintained surfaces instead of fake “fix compilation” debt.
Refs: `Cargo.toml`, `AGENTS.md`, `ARCHITECTURE.md`.
Status: done.

3. `build.ps1` became the optimized Windows build entrypoint using CargoTools-managed environment/config.
Refs: `build.ps1`, `.cargo/config.toml`, `xtask/src/ffi.rs`.
Status: done.

4. FFI boundary hardening landed.
Refs: `ffi/src/log.rs`, `ffi/src/connector/mod.rs`, `ffi/dotnet/Devolutions.IronRdp/src/Connection.cs`.
Status: mostly done.

5. The old absolute `no_std` messaging was corrected.
Refs: `ARCHITECTURE.md`, `CLAUDE.md`, `AGENTS.md`, foundational crate manifests.
Status: done.

6. Native client window/render lifetime handling was fixed.
Refs: `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-client/src/main.rs`.
Status: done.

7. Client and server runtimes were split into bootstrap vs session-driver boundaries.
Refs: `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-server/src/server.rs`, `crates/ironrdp-server/src/session_driver.rs`.
Status: done.

8. The Windows FFI/package path became quieter and more reproducible.
Refs: `build.ps1`, `xtask/src/ffi.rs`, `ffi/README.md`, `ffi/dotnet/NuGet.Config`, `ffi/dotnet/Devolutions.IronRdp/*.csproj`.
Status: done.

9. Native client bootstrap/runtime polish moved forward.
Refs: `crates/ironrdp-client/src/config.rs`, `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-client/src/main.rs`, `crates/ironrdp-client/src/rdp.rs`.
Status: done.

10. Server reliability and encoder scratch-buffer reuse improved.
Refs: `crates/ironrdp-server/src/session_driver.rs`, `crates/ironrdp-server/src/encoder/mod.rs`.
Status: done.

11. Build manifests now record machine-scoped artifact metadata.
Refs: `build.ps1`.
Status: done.

12. Windows client socket setup now applies both `TCP_NODELAY` and TCP keepalive on direct TCP and WebSocket bootstrap paths.
Refs: `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-client/Cargo.toml`.
Status: done.

13. The build framework now records hardware and toolchain profile data and can emit a manifest without compiling via `build.ps1 -Mode doctor`.
Refs: `build.ps1`, `README.md`.
Status: done.

14. Native client frame presentation now reuses packed frame buffers instead of allocating a fresh packed buffer on every image update.
Refs: `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-client/src/app.rs`.
Status: done.

15. Native client Unicode text entry now uses `winit` IME commit events while suppressing conflicting raw key forwarding during composition.
Refs: `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-client/src/rdp.rs`.
Status: done.

16. Client/session/server documentation was updated to reflect the Windows-native runtime split, software presentation path, and current protocol/runtime responsibilities.
Refs: `README.md`, `ARCHITECTURE.md`, `crates/ironrdp-client/README.md`, `crates/ironrdp-session/README.md`, `crates/ironrdp-server/README.md`.
Status: done.

17. The Windows toolchain and crate patch lines were refreshed around Rust 1.94, CargoTools/sccache wrapper behavior was aligned with the module's current daemon/queue model, and stable-format noise was removed from `rustfmt.toml`.
Refs: `rust-toolchain.toml`, `clippy.toml`, `Cargo.lock`, `build.ps1`, `rustfmt.toml`, crate manifests, targeted protocol/runtime fixes.
Status: done.

18. A no-repo Windows deployment path now exists as a portable bundle with install and smoke-test helpers.
Refs: `build.ps1`, `scripts/windows/Install-IronRdpPackage.ps1`, `scripts/windows/Invoke-IronRdpSmokeTest.ps1`, `docs/windows-native-install.md`, `README.md`, `xtask/README.md`.
Status: done for local package/install/smoke validation and Hyper-V Windows Server 2025 guest validation; remote `dtm-p1gen7` copy/install remains.

19. Lightweight client frame-path diagnostics now trace frame packing, surface present timing, and resize/reconnect churn to guide deeper render work.
Refs: `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-client/README.md`.
Status: done.

20. The direct Windows-native runtime now treats repeated resize reconnects without any desktop-size change as a bounded error, and the server's single-session posture is now documented explicitly.
Refs: `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-server/src/server.rs`, `crates/ironrdp-server/README.md`, `ARCHITECTURE.md`.
Status: partially done; broader reconnect and single-session integration coverage still remains.

21. The native client now exposes experimental multitransport advertising and replies to unsupported server-side multitransport requests with an explicit TCP-side `E_ABORT` instead of silently dropping them.
Refs: `crates/ironrdp-client/src/config.rs`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-session/src/active_stage.rs`, `crates/ironrdp-session/src/x224/mod.rs`, `crates/ironrdp-client/README.md`.
Status: groundwork done; real UDP sideband transport is still not implemented.

22. The native client now has an internal presentation-backend seam and passes reusable RGBA frames directly to the backend, removing the extra packed `Vec<u32>` staging buffer from the software render path.
Refs: `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-client/src/presentation.rs`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-client/README.md`.
Status: done; `softbuffer` remains the default backend and still performs one backend-local surface conversion.

23. Focused multitransport unit coverage now pins client-side advertisement mapping and IO-channel request/abort wrapping, and the `softbuffer` presenter now uses a simpler validated RGBA packing loop.
Refs: `crates/ironrdp-client/src/config.rs`, `crates/ironrdp-session/src/x224/mod.rs`, `crates/ironrdp-client/src/presentation.rs`.
Status: done; real UDP sideband transport and end-to-end runtime coverage still remain.

24. Portable package and publish builds now embed a static MSVC CRT for the native Windows artifacts, and the no-repo install/smoke flow has been validated on a clean Hyper-V Windows Server 2025 guest.
Refs: `build.ps1`, `docs/windows-native-install.md`, `README.md`, local Hyper-V validation logs.
Status: done; `dtm-p1gen7` still needs the same deployment flow mirrored remotely.

25. The Windows deployment tooling now includes bounded live-connect validation against the running Hyper-V guest, and the native client emits explicit connection/first-frame markers for log-driven smoke automation.
Refs: `build.ps1`, `scripts/windows/Invoke-IronRdpSmokeTest.ps1`, `scripts/windows/Invoke-HyperVLiveConnectTest.ps1`, `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-client/src/app.rs`, local Hyper-V live-connect logs.
Status: done. Current observed baseline:
- host reaches the guest over the Hyper-V Default Switch address, not the current `dtm-net-switch` address
- `session-rendering` is reliable on the Hyper-V path
- software bitmap traffic still dominates, including many `16`-bpp RLE bitmap updates
- `softbuffer` conversion remains the dominant client-side present cost
- experimental multitransport advertising did not trigger a UDP sideband request from the guest in this environment

26. The Hyper-V lab now includes a richer e2e scenario suite that captures connection latency, frame cadence, compression mix, overwritten-frame counts, bounded resize/input behavior, and workload-launch metadata from packaged artifacts.
Refs: `build.ps1`, `scripts/windows/Invoke-HyperVE2ESuite.ps1`, `scripts/windows/Invoke-IronRdpSmokeTest.ps1`, `docs/windows-native-install.md`, local Hyper-V suite logs under `%TEMP%\ironrdp-hyperv-suite-*`.
Status: done for the first regression-ready pass. Current observed baseline and feature coverage:
- connection establishment is roughly `~130 ms`
- first-image and first-frame latencies are roughly `~700 ms`
- the guest still prefers `Rdp61`/bitmap traffic, especially `16`-bpp RLE streams
- the native client is overwriting almost every queued frame under this workload, so present-path pacing/coalescing is now the highest-value render optimization
- resize scenarios do not currently trigger reconnects, but they do amplify backend-total present spikes
- the default guest workload is now a direct WinRM-backed file write, so the suite no longer relies on Notepad or session-`0` fallback as the primary guest-activity signal
- a fully interactive in-session workload is still missing; the remaining blocker is that an `Interactive` scheduled task can report success without creating a visible process in the active RDP session
- CLIPRDR initializes successfully on the Hyper-V path, and resize scenarios now observe remote format-list acknowledgement
- host-side clipboard mutation is now part of the suite, but end-to-end text clipboard transfer is not proven yet because the current run did not produce local forwarded/handled clipboard events
- guest audio services are running and the client audio channel is wired, but the suite still needs a guest-side sound workload before playback-path assertions are honest
- USB / drive / printer / generic device redirection remain explicitly unsupported because this branch still uses `NoopRdpdrBackend`
- the initial resize scenario exposed a real post-reactivation issue: rebuilding the FastPath processor dropped the live bulk decompressor state and led to compressed FastPath decode failure

27. The resize / deactivation-reactivation fault is fixed at the session layer: `ActiveStage` now reactivates the existing FastPath processor in place so the negotiated bulk-decompression state survives reactivation, instead of rebuilding the processor and dropping live compression history.
Refs: `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-session/src/active_stage.rs`, `crates/ironrdp-session/src/fast_path.rs`, `crates/ironrdp-testsuite-extra/tests/mod.rs`, Hyper-V suite logs under `%TEMP%\\ironrdp-hyperv-suite-20260316-202736`.
Status: done and Hyper-V revalidated. The resize scenario now completes without:
- `Received compressed FastPath data but no decompressor is configured`
- `bulk decompression failed`
- post-reactivation pointer decode faults

28. Frame pacing was added to the session driver to reduce overwritten-frame waste: a 4 ms coalescing timer in the `tokio::select!` loop defers frame emission after each presentation ack, absorbing server-side update bursts into a single composite frame.
Refs: `crates/ironrdp-client/src/session_driver.rs`, Hyper-V suite logs under `%TEMP%\\ironrdp-hyperv-suite-20260316-202736`.
Status: done and Hyper-V revalidated. Under the current resize workload:
- overwritten-frame count dropped from the previous “nearly every queued frame” baseline to `0`
- resize scenario render cadence stabilized around `~60 fps`
- backend-total present cost is still the main client-side render bottleneck, not queue churn

29. The Hyper-V e2e suite now reports explicit scenario health, failures, warnings, and staged clipboard/audio observations instead of count-only summaries.
Refs: `scripts/windows/Invoke-HyperVE2ESuite.ps1`, `README.md`, `docs/windows-native-install.md`.
Status: done. Current suite output now includes:
- per-scenario `health.passed`, `health.failures`, and `health.warnings`
- staged `clipboardStage` / `audioStage` reporting
- derived pacing metrics such as overwrite-per-presented-frame and first-image-to-first-frame latency
- top-level suite rollups for baseline/resize pass state and worst-case latency/overwrite ratios

30. The Hyper-V e2e suite now classifies scenario workload quality and primary diagnosis instead of leaving the operator to infer it from raw metrics alone.
Refs: `scripts/windows/Invoke-HyperVE2ESuite.ps1`, `README.md`, `docs/windows-native-install.md`, Hyper-V suite logs under `%TEMP%\\ironrdp-hyperv-suite-*`.
Status: done. Current suite output now includes:
- per-scenario `health.diagnosis.primary` for `healthy`, `transport-limited`, `decode-limited`, or `present-limited`
- per-scenario `health.diagnosis.workloadStage` so session-`0` fallback is explicit
- diagnosis signals that call out the dominant reason a scenario is degraded
- top-level rollups for `workloadObservedStage` and dominant diagnosis class

31. The Hyper-V harness now enables and validates guest WinRM, stores reusable lab credentials in Windows Credential Manager, and drives a deliberate guest-side audio pulse through the WinRM path.
Refs: `scripts/windows/Invoke-HyperVE2ESuite.ps1`, `README.md`, `docs/windows-native-install.md`, local Credential Manager entries for `IronRDP-HyperV-*`, `WSMAN/*`, and `TERMSRV/*`.
Status: done. Current harness behavior now includes:
- WinRM enablement and reachability validation for the selected guest IP before scenarios begin
- reuse-friendly stored credentials for the Hyper-V lab targets
- guest-side audio pulse attempts that can move `audioStage` from `channel-wired` to `playback-observed`

32. Audio playback was silently broken: the `cpal` output stream was built but `stream.play()` was never called, leaving the stream in a paused state for every session. Fixed by adding the `play()` call and importing `StreamTrait`.
Refs: `crates/ironrdp-rdpsnd-native/src/cpal.rs`.
Status: done. Needs Hyper-V revalidation with a guest-side audio workload.

33. Server pointer position updates now use `PhysicalPosition` instead of `LogicalPosition`, fixing cursor misplacement on HiDPI displays (125%, 150%, etc.) where the DPI scaling factor was being applied twice.
Refs: `crates/ironrdp-client/src/app.rs`.
Status: done.

34. The native client now emits finer-grained present-path diagnostics: `acquire_micros` for surface buffer acquisition and an explicit `pending_after_immediate_draw_count` signal for frames that still require a redraw after an immediate draw attempt.
Refs: `crates/ironrdp-client/src/presentation.rs`, `crates/ironrdp-client/src/app.rs`, Hyper-V suite log parsing in `scripts/windows/Invoke-HyperVE2ESuite.ps1`.
Status: done. The new client traces now expose:
- surface-buffer acquisition cost separately from conversion/present time
- a direct “immediate draw still pending” pressure signal instead of inferring it only from overwritten-frame counts
- enough data for the suite to distinguish workload cadence from true present-path lag

35. The Hyper-V suite now reports `interactiveWorkloadPassed`, `workloadLaunchModes`, pending-after-draw pressure, and tighter present-path attribution. It no longer treats “present interval p95 > 16 ms” as present-limited on its own when image cadence is already slower than 60 FPS.
Refs: `scripts/windows/Invoke-HyperVE2ESuite.ps1`, local Hyper-V suite logs under `%TEMP%\\ironrdp-hyperv-suite-*`.
Status: done. The suite now:
- reports when guest workloads required fallback even if the scenario otherwise passed
- records launch mode explicitly instead of collapsing all workload failures into a generic warning
- uses cadence comparison and immediate-draw pressure instead of a blunt 16 ms threshold alone

36. The Hyper-V lab now has a stable default guest workload path: direct WinRM-backed file creation inside the guest user profile replaces the older Notepad/session-`0` fallback contract for baseline suite runs.
Refs: `scripts/windows/Invoke-HyperVE2ESuite.ps1`, WinRM probes against `172.23.187.173`, local Hyper-V suite logs under `%TEMP%\\ironrdp-hyperv-suite-*`.
Status: done and revalidated. Current implications:
- baseline and resize scenarios can prove guest-side activity without relying on GUI application launch
- suite summaries now expose `remote-file-write` as a first-class workload stage and `winrm-file-write` as the launch mode
- interactive workload launch remains a follow-up item, not a blocker for the default regression path

37. Audio buffer underruns reduced: the cpal backend now requests a ~40ms hardware buffer, uses a 100ms recv timeout (was 4 seconds), fills silence on underrun instead of leaving stale data, and tracks underrun count via an atomic counter for diagnostics.
Refs: `crates/ironrdp-rdpsnd-native/src/cpal.rs`.
Status: done. Needs Hyper-V revalidation to measure underrun reduction.

38. Keyboard layout auto-detection: the client now reads the active Windows keyboard layout via `GetKeyboardLayout` on startup and passes it to the server during connection. A `--keyboard-layout` CLI flag allows manual override.
Refs: `crates/ironrdp-client/src/config.rs`, `crates/ironrdp-client/Cargo.toml`.
Status: done.

39. Connection errors and session failures now keep the window open with the error message in the title bar instead of silently vanishing. The window stays open until the user closes it manually, giving time to read the error. Graceful disconnects still auto-close.
Refs: `crates/ironrdp-client/src/app.rs`.
Status: done. 8 new tests for title-formatting helpers.

40. Single-session server behavior is now an explicit programmatic contract. The server tracks active sessions via `Arc<AtomicBool>` with RAII `SessionGuard`, rejects concurrent connections with a clear log message, and documents the invariant in both the struct doc comment and README.
Refs: `crates/ironrdp-server/src/server.rs`, `crates/ironrdp-server/README.md`.
Status: done.

41. Display and bitmap constraints now fail early and clearly. `UpdateEncoder::new()` validates desktop dimensions (non-zero, <=8192), and `BitmapUpdater::handle()` validates stride vs row-width consistency and data-length vs stride x height before any encoding begins.
Refs: `crates/ironrdp-server/src/encoder/mod.rs`.
Status: done.

42. Audio path shutdown noise eliminated. Opus decode errors are now `warn!`-level with a separate `decode_error_count` atomic counter, closed-channel teardown exits silently instead of logging errors, and the cpal recv timeout was reduced from 4s to 100ms with silence fill on underrun.
Refs: `crates/ironrdp-rdpsnd-native/src/cpal.rs`.
Status: done. Hyper-V revalidated: 0 ERROR-level messages across baseline and resize scenarios.

43. Client reconnect and shutdown logging now explicitly distinguishes user-initiated disconnect, server-initiated graceful disconnect, connection failure, and resize-triggered reconnect at `info!`/`error!` level. GUI channel drop is handled as a hard disconnect with a debug log instead of a protocol error.
Refs: `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-client/src/app.rs`.
Status: done. Hyper-V live connect logs show clean 4-step shutdown trail.

44. Focused session seam integration tests added: `test_graceful_disconnect`, `test_server_display_write_failure`, and `test_double_reactivation` cover server-initiated shutdown, display backend failure, and consecutive resize/reactivation with decompressor state preservation. All 6 integration tests pass.
Refs: `crates/ironrdp-testsuite-extra/tests/mod.rs`.
Status: done.

45. Initial `ironrdp-gateway` crate scaffolded with trait-based three-plane architecture: `GatewayAuthenticator` (auth), `GatewayPolicy` + `AuthzDecision` (policy), `GatewayRelay` with bidirectional byte copy and `RelayStats` (data plane), `GatewaySession` (session metadata), and `GatewayConfig` (static configuration). Reuses `ironrdp-rdcleanpath` as the first gateway protocol.
Refs: `crates/ironrdp-gateway/`.
Status: done (scaffold only — no listener, RADIUS, or TLS implementation yet).

46. Piecemeal upstream import pass (Devolutions/IronRDP master, fork was 139
behind). Cherry-picked with `-x` where clean, file-level-adapted where the fork
had reworked the area. All ported into `feat/gfx-early-capability-flag`. Full
`cargo test --workspace` passes; `cargo build --release -p ironrdp-client`
passes; none of the changed files produce any clippy finding (the fork's
workspace `-D warnings` gate is pre-existingly red on unrelated crates —
`ironrdp-client`, `ironrdp-gateway`, `ironrdp-rdpeusb`, `ffi`,
`ironrdp-session/active_stage.rs`, `ironrdp-testsuite-extra` — base commit
5b849c64 also fails clippy, independent of this work).
Panic / correctness fixes:
- #1293 (0dd7c94b) xcrush off-by-one forward-match panic — clean cherry-pick.
- #1392 (d6990d81) propagate `#[track_caller]` through error constructors — clean.
- #1256 (905a1486) rdpsnd Opus PCM alignment panic — adapted (fork reworked cpal;
  kept the fork's decode-error counter + silent-teardown, adopted `Vec<i16>` alloc).
- #1276 (6e847976) rdpsnd keep-newest-waves on overflow — adapted (fork moved the
  dispatch into `session_driver.rs`; ported the drop-oldest semantics there).
Connector / protocol correctness:
- #1371 (a4fde9fc) stay in CapabilitiesExchange on activation DeactivateAll — core
  cherry-pick; fork-deleted test module dropped.
- #1254 (9cb5439b) skip ServerDeactivateAll during CapabilitiesExchange — the inner
  half of the same fix, which the fork lacked; directly serves the GRD 46 handover
  (GRD sends ServerDeactivateAll before DemandActive). Core ported; test dropped.
- #1382 (3f96d002) set COMPRESSION_USED on FastPath update header — clean.
- #1313 (a71567e3) cover BitmapCacheV3 in CapabilitySet encoder (fixes a reachable
  `unreachable!()` panic) — core + testsuite-core test; fork's richer fuzz oracle kept.
- #1231 (2fa7c648) advertise all colour depths + derive highColorDepth per
  max_color_depth (modern Windows hosts reset 24bpp-only clients) — clean cherry-pick;
  fits this branch's earlyCapabilityFlags theme.
- Restored the upstream `connection_activation` test module (dropped by the fork)
  so #1254/#1371 are covered: 3 tests exercise the ServerDeactivateAll path
  (only adaptation was the fork's added `enable_graphics_pipeline` Config field).
Graphics robustness:
- #1298 (67f3c635) tolerate unknown EGFX capability versions — adapted (fork's
  `try_from` only mapped one sentinel; broadened to any unrecognized version →
  `CapabilitySet::Unknown`).
- #1341 (ef20ea4e) decode RGBA QOI bitmaps instead of dropping the frame — clean.
- #1344 (4e11a176) bound ZGFX compressor hash table — clean.
Also: collapsed a pre-existing `collapsible_if` in the server credential-validator
path (touched while porting #1276) into a let-chain.
Deferred (too entangled with the fork's egfx/rfx/rdpeusb/dvc rework, or tracked
separately):
- #1305 (91ea46bd) RawCapabilitySet vs typed CapabilitySet split — 392-line breaking
  rework of the fork's most-diverged file; its client benefit (tolerate unknown caps)
  is already delivered by the #1298 adaptation.
- Tier-4 features: clipboard file-copy (#1388/#1375/#1372, tracked as Track C item 9),
  DVC accessors (#1368/#1358, breaking dvc changes), agent resize (#1401, ironrdp-agent
  crate not present in fork).

## GRD render RESOLVED — root cause was a build-feature footgun, NOT a capability rejection (2026-07-04, later pass)

**The "ERRINFO_BAD_CAPABILITIES / Confirm Active capability rejection" framing was
a red herring.** There is no rejected capability. The handover failed because the
test binaries were built WITHOUT the `egfx` feature.

Root cause (proven with `ironrdp_dvc=trace`, live GRD 46.3 handover):
- `cargo build --release -p ironrdp-client` does NOT enable `egfx` (client
  `default = ["rustls"]`; `egfx = ["ironrdp-egfx"]`, `openh264 = ["egfx", ...]`).
  The prebuilt "stable reference" and a plain release build are byte-identical
  (10,224,640 bytes) — both featureless.
- A featureless build still accepted `--egfx` and set the connector's
  `enable_graphics_pipeline`, so the client advertised
  `Microsoft::Windows::RDS::Graphics`. GRD's handover instance opened the RDPGFX
  DVC; the client had NO registered listener (feature compiled out) and answered
  the Create Request with `CreationStatus(0xC0000001)` = NO_LISTENER. GRD logged
  `[RDP.RDPGFX] Failed to open channel (CreationStatus -1073741823). Terminating
  session` and set ERRINFO_BAD_CAPABILITIES during teardown — which the client
  saw as `read deactivation-reactivation sequence step / disconnect provider
  ultimatum: UserRequested`. So the "cap rejection" was DVC teardown fallout.
- The DVC trace shows it plainly: on the handover reconnect, `Graphics` →
  `CreationStatus(3221225473)` (NO_LISTENER); `DisplayControl` →
  `CreationStatus(0)` (OK). DisplayControl is registered unconditionally; EGFX is
  gated behind `#[cfg(feature = "egfx")]`.

Fix (committed):
1. `fix(client): do not advertise Graphics Pipeline without the egfx feature`
   (config.rs) — `enable_graphics_pipeline` / client `egfx` now derive from an
   `egfx_enabled` value that is true only when EGFX is requested AND compiled in;
   a featureless build warns and falls back to the classic bitmap path instead of
   dead-ending the handover.

RENDER VERIFIED (built with `--features openh264`): connecting
`100.64.0.3:3389 -u damartel --egfx` completes the GRD 46.3 handover and renders
a full 1920x1080 desktop on the first try. `IRONRDP_EGFX_DUMP` produced an
8,294,400-byte (1920x1080x4) frame; converted to PNG and visually confirmed as
the Ubuntu GDM greeter (clock "Jul 4 19:49", "David Martel" login field, "Not
listed?", Ubuntu logo with correct orange/red RGB, top-right status icons). Note:
this is the GDM greeter, not a post-login desktop, but it fully exercises the
EGFX handover + progressive-decode pipeline. Server render node is healthy after
the gdm→render/video group fix (no more ZINK "failed to choose pdev").

dtm-work regression: PASS — classic Windows RDP path unaffected, first frame
presented 1920x1080 (`-u david` and `-u davidmartel07@gmail.com` both connect).

DECISION MADE (2026-07-04, gap 1): `openh264` is now a DEFAULT client feature.
`crates/ironrdp-client/Cargo.toml` `default = ["rustls", "openh264"]` (openh264
enables egfx transitively), so `cargo build --release -p ironrdp-client` renders
GRD/H.264 servers out of the box. The classic bitmap/RFX path is unchanged: the
connector only advertises the Graphics Pipeline when `--egfx` is passed AND the
feature is compiled in (config.rs `egfx_enabled`), so a default build that is NOT
given `--egfx` behaves exactly as before (dtm-work baseline unaffected). A
patent/H.264-clean build is still available via
`--no-default-features --features rustls`.

H.264 (bundled-OpenH264) licensing consideration — DOCUMENTED, not resolved:
`openh264 -> ironrdp-egfx/openh264-bundled` selects `openh264/source`, which
COMPILES Cisco's OpenH264 from source rather than downloading Cisco's signed
binary. Cisco's royalty-free H.264/AVC patent grant only covers *Cisco's own
binary distribution* of OpenH264; a from-source build falls OUTSIDE that grant.
Consequence: any party distributing this default-built artifact is responsible
for its own H.264/AVC (MPEG-LA/Via LA pool) patent posture — either holding a
license or accepting the risk. This is a distribution/legal decision, not a code
bug. Packaging follow-ups that are now IMPLIED but deliberately NOT done in gap 1
(left for the packaging owner): decide whether `build.ps1 -Mode package|publish`
and `.github/workflows/windows-release.yml` ship the H.264 default or the
`--no-default-features --features rustls` patent-clean portable class, and mark
the class explicitly in artifact manifests. Scope of gap 1 was intentionally
Cargo.toml default + this note + build/live validation only.

Two remaining EGFX-quality follow-ups (NOT attempted this session — deferred with
concrete, containment-first designs so neither can threaten the two banked wins,
dtm-work classic RDP and asuspro13 GRD AVC420 render):

- EGFX bounding-box (dirty-rect) delivery [prior gap 2 / Priority 2.1]. Status:
  **IMPLEMENTED 2026-07-04 on branch `gap/egfx-dirtyrect` (off 60bee85a).** The
  design below was followed. Summary of what shipped:
    * `BitmapUpdate` (ironrdp-egfx `client.rs`) gained `surface_width` /
      `surface_height` so the presenting handler can distinguish a full-surface
      update from a sub-rect and size its own persistent framebuffer. All three
      construction sites (`decode_avc420`, `handle_uncompressed`,
      `handle_wire_to_surface2`) populate them.
    * `handle_wire_to_surface2` now tracks the changed-tile bounding box while
      compositing (min/max of `tile.x_idx/y_idx*64`, clipped to the surface),
      then `crop_region()` crops just that bbox out of the persistent
      `progressive_framebuffers[surface_id]` accumulator and delivers it as a
      sub-rect `BitmapUpdate` (dest_rect = bbox, width/height = bbox size). The
      accumulator STAYS in egfx (lifecycle tied to ResetGraphics/DeleteSurface);
      the full clone-every-frame is gone.
    * New parallel render event `RdpOutputEvent::ImageRegion { buffer, x, y, w, h,
      surface_w, surface_h }` (rdp.rs). `EgfxRenderHandler::on_bitmap_updated`
      routes full-surface updates (origin + surface-sized) to the UNCHANGED
      `Image` path (buffer moved, byte-identical — dtm-work/AVC420 banked wins
      untouched) and everything else to `ImageRegion`.
    * `app.rs` keeps a persistent surface-sized `self.buffer`; `blit_image_region`
      (re)allocates it to `surface_w*surface_h` opaque-black on size change, then
      blits the region at (x,y) with hard clipping to surface bounds (a malformed
      server rect cannot panic or OOB-write). The small region buffer is recycled
      via the existing `RecycleFrameBuffer` channel.
    * `destination_rectangle` is now HONORED for placement on both codecs: a
      future AVC420 sub-rect update (GRD currently sends full-surface AVC420, so
      it stays on the `Image` path) would also route to `ImageRegion` and be
      placed at its offset instead of corrupting the frame to the top-left.
  Validation: `cargo test --workspace` green; unit tests are the acceptance gate
  (`crop_region_offset_subrect_is_byte_exact`,
  `crop_region_then_blit_back_matches_full_frame_delivery` in egfx — offset region
  x>0,y>0,w<W,h<H so source/dest strides differ; `blit_image_region_*` in
  ironrdp-client covering placement, out-of-bounds clipping, and unchanged-pixel
  preservation). Clippy clean on touched files. Release build (default openh264)
  and the `--no-default-features --features rustls,egfx` (no-H264) build both
  compile. DEFERRED live validation: a running GRD session on the no-H264 build
  (progressive path) and a Windows AVC420-sub-rect host were NOT exercised (no
  live GRD access this phase — comprehensive live validation is the dedicated
  later phase). Original finding/design retained below for reference:

  DEFERRED — not cleanly validatable under the new openh264 default. Key finding:
  `handle_wire_to_surface2` (RemoteFX **Progressive**) is the code path that marks
  the whole surface dirty and clones the full framebuffer each frame — but with
  `openh264` now DEFAULT, GRD sends AVC420 via `WireToSurface1` and STOPS sending
  progressive RFX, so `handle_wire_to_surface2` is not exercised against GRD in the
  default build. Validating a progressive dirty-rect change therefore requires a
  `--no-default-features --features rustls,egfx` build (no H.264) connected to GRD
  (which then falls back to progressive RFX), OR a Windows host that sends
  progressive. Concrete containment-first design when picked up:
    1. Add a PARALLEL render event `RdpOutputEvent::ImageRegion { buffer, x, y, w,
       h, surface_w, surface_h }` — do NOT modify the shared `RdpOutputEvent::Image`
       full-frame path (that path carries BOTH banked wins; the naive sub-rect-in-
       Image approach shrinks the frame to the top-left = corruption).
    2. `app.rs` keeps its own PERSISTENT full-surface `self.buffer` (sized
       surface_w*surface_h) and BLITS the sub-rect at (x,y) into it, then presents,
       instead of `queue_image_buffer` swapping the whole buffer. Recycle small
       sub-rect buffers back to egfx via the existing `RecycleFrameBuffer` channel.
    3. `egfx/client.rs` computes the changed-tile bounding box (min/max of
       `tile.x_idx/y_idx * 64`, clipped to surface) in `handle_wire_to_surface2`,
       crops that rect out of `progressive_framebuffers[surface_id]`, and delivers
       it as the ImageRegion. KEEP the accumulator in egfx — do NOT move
       `progressive_framebuffers` to app.rs (its reset/delete lifecycle is tied to
       ResetGraphics/DeleteSurface that app.rs cannot see).
    4. Acceptance gate: dump both full-frame (pre) and dirty-rect (post) framebuffers
       via `IRONRDP_EGFX_DUMP` on the no-openh264 build and diff to PNG — no visual
       corruption vs the whole-surface delivery. dtm-work `Image` path stays
       byte-identical by construction, so that banked win is untouchable.
  NOTE: the AVC420 (`WireToSurface1`) path already delivers only `dest_rect`-sized
  data, but the renderer ignores `destination_rectangle` and treats every update as
  full-frame at origin (0,0) — see `rdp.rs::on_bitmap_updated` -> `RdpOutputEvent::
  Image`. If GRD ever sends AVC420 sub-rect updates (it currently sends full-surface
  frames), that path would ALSO need the ImageRegion offset treatment. Same event
  design covers both codecs.

- AVC444 dual-stream decode [prior gap 4 / Track B refinement]. Status: DEFERRED.
  Currently the `Avc444|Avc444v2` arm in `egfx/client.rs::handle_wire_to_surface1`
  forwards to `on_unhandled_pdu`, and `rdp.rs::EgfxRenderHandler::capabilities`
  deliberately caps advertisement at V8.1/AVC420 so servers never select AVC444.
  Containment-first design: implement dual-stream decode (parse the AVC444 bitmap
  stream = an `LC` field + up to two `Avc420BitmapStream`s: the main/luma view and
  the chroma-auxiliary view per [MS-RDPEGFX] 2.2.4.4/2.2.4.5; decode BOTH via the
  existing `openh264` AVC420 path, then reconstruct YUV444 from the two YUV420
  planes and convert to RGBA) BEHIND AN OPT-IN FLAG (mirror `--network-autodetect`:
  add `Config::avc444` + `--avc444`, and only re-raise `capabilities()` to include
  V10.7/AVC444 when the flag is set). NEVER make AVC444 default — a buggy dual-stream
  decode reachable by default would break the banked GRD render. Live-validate by
  flipping the flag against GRD/Windows and visually confirming the frame; keep the
  default (V8.1/AVC420) advertisement so the banked render is never at risk. Note
  GRD's AVC444 encode support is unconfirmed (its handover instance advertised only
  AVC420 acceptance in these runs); a Windows RDS host is the more reliable AVC444
  peer for validation.

- Reconnect-into-existing-session + CredSSP InvalidToken handover [prior gap 5].
  Status: DEFERRED (feature-sized; auth/reconnect only — does NOT touch the render
  path, so lowest regression risk of the deferred items). Two distinct pieces:
  (a) Reconnect-into-existing-session is NOT the same as the GRD handover redirect
      (that one — reconnect carrying LoadBalanceInfo + LB_USERNAME/LB_PASSWORD as an
      X.224 routing token — is already RESOLVED, see Track B item 0). This is RDP
      *auto-reconnect* ([MS-RDPBCGR] 2.2.4): the server sends a Server Auto-Reconnect
      Cookie (ARC) in the Save Session Info PDU; on an unexpected drop the client
      reconnects sending the ARC_CS_PRIVATE_PACKET (Client Auto-Reconnect Packet) so
      the server re-attaches the existing session instead of starting a new one. The
      client currently sends `reconnect_cookie: None` (confirmed in connect logs) and
      discards SaveSessionInfo (`x224/mod.rs` logs+drops it). Concrete next step:
      capture the ARC cookie from `ShareDataPdu::SaveSessionInfo` (LogonInfoExtended
      -> ServerAutoReconnect), store it on the session/config, and populate
      `reconnect_cookie` in `ExtendedClientOptionalInfo` on the reconnect attempt in
      `RdpClient::run`'s reconnect loop. Regression-safe: default `None` preserves
      today's behavior. CORRECTION (2026-07-05, see "Upstream import re-scan"
      section): "populate `reconnect_cookie`" is NOT a copy of `random_bits` — the
      client cookie's SecurityVerifier = HMAC-MD5(ArcRandomBits, ClientRandom) per
      [MS-RDPBCGR] 5.5 (crypto derivation). Upstream also only TODOs this
      (`rdp.rs:774 TODO(#271)`), so there is no import; it is from-scratch and gate
      it behind an opt-in `--auto-reconnect` flag rather than a bare default `None`.
  (b) CredSSP `InvalidToken` (nstatus 0xc00700ea) on the GRD handover reconnect is a
      FLAKY, server-side/environment auth race (winpr NTLM SAM not ready vs the
      client's NTLMSSP), present at both old and new commits and not an import/client
      regression. It is NOT the NTLM-SAM *username/password* mismatch (that was fixed
      by sending LB_USERNAME/LB_PASSWORD as UTF-16LE). Concrete next step: it is
      unreproducible-on-demand from the client; if it must be chased, add a bounded
      CredSSP retry on `InvalidToken` during the handover reconnect ONLY (do not
      touch the initial-connect CredSSP path, which is the dtm-work banked win), and
      confirm against a freshly-restarted GRD where the handover SAM is warm.

## Upstream import re-scan for the deferred gaps (2026-07-05)

Targeted re-scan of `master..upstream/master` (139 commits, upstream HEAD
069786c9) specifically to import commits that resolve the deferred gaps above.
**Headline: only gap 5 (robustness) was closable by import. The premise that
upstream already implements AVC444 decode (gap "1"/AVC444) and the ARC
reconnect-cookie wiring (gap "5"/reconnect) is FALSE — upstream stubs/TODOs both,
so there is nothing to cherry-pick for them.** Primary-source evidence:

- AVC444 dual-stream decode: **no upstream source exists.**
  `upstream/master crates/ironrdp-egfx/src/client.rs:720` is byte-for-intent
  identical to the fork — `Codec1Type::Avc444 | Codec1Type::Avc444v2 => { debug!(
  "AVC444 codec not yet implemented, forwarding to handler") }`. Upstream ships
  the `Avc444BitmapStream` PDU parser (`pdu/avc.rs`) but NOT the luma+chroma-aux
  → YUV444 decode. Remains DEFERRED and from-scratch; additionally UNVERIFIABLE in
  this environment (GRD's handover instance advertised AVC420 acceptance only, so
  there is no AVC444 peer to visually validate against). Keeping it deferred is
  correct — a from-scratch dual-stream decoder reachable by default would threaten
  the banked GRD render; it must stay behind the opt-in `--avc444` flag if ever
  built. Design in the AVC444 bullet above still stands.

- ARC auto-reconnect cookie: **no upstream source exists.**
  `upstream/master crates/ironrdp-client/src/rdp.rs:774` is a bare
  `// TODO(#271): use the "auto-reconnect cookie"`. Both fork and upstream have the
  PDU infrastructure (`client_info.rs` `reconnect_cookie: Option<[u8;28]>` +
  `ExtendedClientOptionalInfo` builder; `session_info/logon_extended.rs`
  `ServerAutoReconnect { logon_id, random_bits:[u8;16] }`), but neither wires
  capture→use. **Correction to the prior gap-5(a) note:** populating
  `reconnect_cookie` is NOT a copy of `random_bits`. The client cookie's 16-byte
  SecurityVerifier = HMAC-MD5(ArcRandomBits, ClientRandom) per [MS-RDPBCGR] 5.5 —
  it is a crypto derivation, not a memcpy. So the "just store + populate" framing
  understated it. DEFERRED (from-scratch, and cannot observe a single successful
  reconnect round-trip here — no reproducible ARC-issuing unexpected drop; GRD's
  redirect is a different, already-resolved mechanism). If picked up: (1) confirm
  against [MS-RDPBCGR] 5.5 that under Enhanced (TLS/CredSSP) security ClientRandom
  is 32 zero bytes so the verifier is deterministic and unit-testable against a
  fixed vector; (2) gate behind an opt-in `--auto-reconnect` flag (mirror
  `--avc444`/`--network-autodetect`) so the banked resize-reconnect path stays
  byte-identical by default — safer than a bare `default None`. A capture-only
  half was rejected: adding `ProcessorOutput`/`ActiveStageOutput` variants across
  two crates for a value that is stored and never read is dead plumbing.

- EGFX destination_rectangle / dirty-rect (gap "2"): the relevant upstream commits
  (#1238/#1246 exclusive-bounds rects, #1197 progressive decode/integration) are
  egfx-crate *correctness*, not the fork's deficit. The fork's deficit — renderer
  ignores `destination_rectangle` and full-clones the surface — lives in the fork's
  own rewritten `app.rs`/`rdp.rs`, so it was from-scratch (design in the dirty-rect
  bullet above). **RESOLVED 2026-07-04 on `gap/egfx-dirtyrect`** — implemented
  from scratch per that design (parallel `ImageRegion` event + persistent app-side
  framebuffer + egfx bbox crop; `destination_rectangle` honored on both codecs).
  Unit-tested; live progressive validation on a no-H264 GRD build deferred to the
  dedicated live-validation phase.

- Connect-time / in-session network auto-detect (gap "4"): upstream #1178
  (4dcad099) handles the *share-data-framed* `ShareDataPdu::AutoDetectReq`. The
  fork does not have that arm, but it independently handles the framing GRD
  actually uses — the *message-channel / security-header-framed* auto-detect
  request (`connect_time_autodetect` + `x224::process_unrouted_channel`, commit
  61af5a77). Crucially, #1178 does NOT address gap 4's real blocker (suppressing
  re-advertisement of `SUPPORT_NET_CHAR_AUTODETECT` on the GRD handover reconnect,
  which tears the handover down) — upstream has no such suppression. So gap 4's
  importable part was already delivered by the fork; the remaining blocker has no
  upstream fix. `--network-autodetect` stays opt-in/off by default. #1178 SKIPPED
  (different framing than GRD, not the blocker, and would conflict with the fork's
  reworked `active_stage.rs`/`x224` while adding always-on autodetect the fork
  deliberately gated).

- Gap 5 (robustness) — IMPORTED: **#1236 (78effb3f)** `fix(connector): surface
  actual PDU type when an unexpected Share Control PDU arrives`. Replaces opaque
  "unexpected Share Control Pdu" errors in `legacy.rs` (`decode_share_data`,
  `decode_io_channel`) and `connection_activation.rs` CapabilitiesExchange with
  `reason_err!` messages naming the actual PDU via `as_short_name()` — improves
  the exact diagnostic surface the fork hit while root-causing the GRD handover
  BadCapabilities teardown. ADAPTED: kept the fork's `ServerRedirect` arm in
  `decode_io_channel`; `connection_activation.rs` auto-merged preserving the fork's
  interleaved Set-Error-Info diagnostic. Gate results: `cargo test --workspace`
  1348 passed / 0 failed (baseline held), `cargo build --release -p ironrdp-client`
  (default rustls+openh264) clean, clippy 0 findings on both touched files.
  dtm-work live insurance (honest framing): TCP+TLS+CredSSP/NLA auth SUCCEEDED
  with a runtime-read Credential-Manager credential (generic `TERMSRV/dtm-work.
  radius.dtmventures.com`, 48-byte blob read via CredRead at runtime, never
  persisted) — the connect/auth path #1236 sits on is intact. **First-frame
  present was NOT reproduced this session** (documented baseline was "connect +
  first frame 1920x1080, session held"), so this run is BELOW baseline: the
  session hit `[read frame]` `ConnectionReset` **WSA 10054 = "existing connection
  forcibly closed by the *remote* host"** right after auth, before any frame. This
  reproduced identically on a single clean run after a 60s half-open drain (not a
  rapid-reconnect artifact), so the cause is a dtm-work server-state condition
  (suspected active single-session / post-NLA reject) — suspected, not confirmed.
  It is provably NOT a #1236 regression: 10054 is remote-initiated (the server
  closed its socket), and #1236 changed only error-arm *strings* in match arms
  that do not execute on a successful connect (the `Ok` arms of `decode_io_channel`
  are byte-identical). asuspro13 GRD render was NOT attempted — #1236 is connector
  error-text and cannot reach the EGFX render path, so it is structurally
  irrelevant to that banked win.

- Already-present / no-op: **#1395 (368fe8e6)** `don't require CONTEXT block on
  every progressive frame` is ALREADY in the fork (`progressive.rs:835-850`,
  same fix + same GNOME-Remote-Desktop rationale comment) — NOT re-imported.

Intentionally SKIPPED (high-value but out-of-scope/entangled/unverifiable):
- #1178 (session auto-detect) — see gap-4 note above.
- #1132 (93833802, slow-path graphics + pointer): real feature but a 480-line
  rewrite of `fast_path.rs`, which the fork already reworked (item 27
  reactivation fix); heavy conflict, and both banked targets use the fast-path,
  not slow-path, so no observable benefit. SKIPPED.
- #1174 (059ca902, ClearCodec bitmap codec): from-scratch codec; no banked target
  negotiates ClearCodec (GRD=AVC420, dtm-work=bitmap/RFX). SKIPPED.
- #1305 (91ea46bd, RawCapabilitySet split): already dispositioned as deferred in
  item 46 (392-line breaking rework of the fork's most-diverged egfx file; client
  benefit already delivered by the #1298 adaptation). Still SKIPPED.

## Live smoke-test findings (2026-07-04, Windows -> asuspro13 GRD 46.3 + dtm-work)

Evidence-based QA pass against both live targets with a fresh `-p ironrdp-client`
release build. Binaries compared: `5b849c64` (last "render worked" commit) vs
current `HEAD` (0cf7a36a, post 16-commit upstream import).

1. GRD render regression: NOT caused by the import (root-caused).
Claim under test: "W->L render rendered a full Ubuntu desktop at 5b849c64, broke
after the import." Result: **both commits fail identically** against the same
live GRD, so the import did not regress render.
- 4 trials each (GRD restarted between): 5b849c64 = 4/4 DEACTIVATE_FAIL; HEAD =
  3/4 DEACTIVATE_FAIL + 1/4 CREDSSP_FAIL. Same error string, same flow.
- Confirmed root cause (HEAD, `ironrdp_connector=trace`): the session-handover
  reconnect passes CredSSP, confirms active, then GRD's handover instance runs a
  Deactivation-Reactivation and sends `ServerSetErrorInfo(RdpSpecificCode(
  BadCapabilities))`. The session-layer reactivation loop
  (`ConnectionActivationSequence`, `connection_activation.rs` CapabilitiesExchange)
  only expected ServerDeactivateAll / ServerDemandActive, so it aborted with the
  misleading `unexpected Share Control Pdu (expected ServerDemandActive)`.
- #1254/#1371 do NOT cause this: they fixed the *connector-initial* DeactivateAll
  handling; 5b849c64 lacks them and fails the same way. **Do not revert them** —
  reverting cannot restore render and loses the DeactivateAll robustness.
- Fix applied (this pass): the reactivation CapabilitiesExchange now treats an
  interleaved Set Error Info PDU as a diagnostic notification (logs the actual
  code, e.g. BadCapabilities, at warn) and keeps reading for ServerDemandActive
  instead of aborting — protocol-correct per [MS-RDPBCGR] 2.2.5.1. dtm-work
  baseline unaffected; 666 connector/testsuite tests pass.
- VERIFIED (after full system+user GRD restart): the fix fires and now logs the
  real reason — `error_info=[RDP specific code]: The capabilities received from
  the client in the Confirm Active PDU were not accepted by the server` — then,
  instead of sending a Server Demand Active, GRD's handover instance issues an
  MCS Disconnect Provider Ultimatum (UserRequested) and tears the connection
  down. So **render is NOT restorable client-side by this change**: GRD's
  handover/user-session instance genuinely rejects our Confirm Active
  capabilities (BadCapabilities). The fix's value is truthful diagnostics +
  protocol-correct continuation, not render restoration.
- Why the caps are now rejected (best current hypothesis, server-side): the
  headless GNOME rendering backend is degraded — the daemon logs `Cannot load
  libcuda.so.1` / `libnvidia-encode.so.1` and (per earlier sessions) ZINK/EGL
  "failed to choose pdev". A handover instance that cannot bring up its
  encode/render pipeline will reject the client's graphics capabilities. The
  client Confirm Active capability set is byte-identical between 5b849c64 and
  HEAD, so this is not a client regression.
- Concrete next step to close the render gap: on a freshly-booted asuspro13 with
  a healthy GRD graphics backend (verify no ZINK "failed to choose pdev" and a
  working render node), retry and capture a framebuffer dump. If it still
  BadCapabilities-rejects, enable FreeRDP verbose capability logging server-side
  (`WLOG_LEVEL=TRACE` on the handover instance) to identify exactly which
  capability set GRD refuses, then adjust the client Confirm Active caps to match
  what GRD's handover instance supports.
- Second, independent handover failure mode: CredSSP `InvalidToken`
  (nstatus 0xc00700ea) in the pub_key_auth step = handover NTLM SAM auth mismatch
  (LB_PASSWORD decode vs stored NTOWFv1, or SAM-not-ready race). Flaky, present at
  both commits. Server-side / environment; not an import regression.

2. Audio output disabled by GRD due to missing network-autodetect advertisement.
GRD logs, every connection: `[RDP] Client does not support autodetecting network
characteristics. Disabling audio output redirection`. FreeRDP-based servers gate
rdpsnd on the client's `RNS_UD_CS_SUPPORT_NET_CHAR_AUTODETECT` (0x0080) early-cap
flag, which the connector does not advertise (`connection.rs`
`create_gcc_blocks`). So the rdpsnd panic/underrun fixes (#1256/#1276, cpal
`play()`) cannot matter on GRD — audio is never enabled server-side.
- Naive fix (advertise the flag) BREAKS connections: with the flag set, GRD sends
  a connect-time Auto-Detect Request PDU that the connector's
  `ConnectTimeAutoDetection` state discards without consuming, desyncing
  LicensingExchange -> `decode during LicenseExchangeState::NewLicenseRequest ...
  invalid security header flags`. Reproduced live against GRD; reverted.
  (dtm-work happened to survive because Windows did not send connect-time
  auto-detect in that idle window — so whether the desync fires is
  server-dependent, making the flag unsafe to advertise without the handler.)
- Real fix — IMPLEMENTED (opt-in), 2026-07-04. A content-dispatching connect-time
  auto-detect handler now exists and is gated behind an opt-in config flag
  (`Config::network_autodetect`, client `--network-autodetect`):
  - new `connect_time_autodetect` module: classify a connect-time I/O-channel PDU
    by its `BasicSecurityHeader` (AUTODETECT_REQ vs anything else — this is the
    *security-header* framing used at connect time, distinct from the in-session
    share-data `ShareDataPdu::AutoDetectReq` framing) and build the security-header
    -framed Auto-Detect Response. Answers RTT Measure Request (RTT Response) and
    connect-time Bandwidth Measure Stop (Bandwidth Measure Results,
    byte_count = Stop payload length, nominal 1 ms delta).
  - `ConnectTimeAutoDetection` connector state: reads the next PDU ONLY when the
    flag is set; answers auto-detect requests and loops; feeds the first
    non-auto-detect PDU (the Licensing request) forward to `LicenseExchangeSequence`
    (mirroring the LicensingExchange terminal check so a single-PDU licensing
    exchange advances instead of being stepped twice).
  - Safe by construction: with the flag OFF (default) the GCC blocks are
    byte-identical and the state keeps its historical immediate passthrough
    (`next_pdu_hint = None`), so the standard connect path — dtm-work included — is
    unchanged. 5 connector unit tests cover classify/respond/passthrough/framing.
  - VALIDATED (live): dtm-work `--network-autodetect` connects + first frame
    presented 1920x1080 (feed-forward path, no auto-detect fired — the exact case
    that used to desync). GRD `--network-autodetect`: the connect-time auto-detect
    now COMPLETES — system-daemon path answered 10 RTT requests + received
    NetworkCharacteristics; handover path answered Bandwidth Start/Stop. No more
    LicensingExchange desync.
  - SERVER-SIDE AUDIO ENABLE — PROVEN (live A/B, 2026-07-04): with the flag OFF,
    GRD logs every connection `[RDP] Client does not support autodetecting network
    characteristics. Disabling audio output redirection`. With `--network-autodetect`
    that line is GONE from the GRD journal — the connect-time handshake flips GRD's
    server-side gate and it no longer disables audio output redirection. This is the
    exact behaviour gap 4 targeted; the negotiation half is done and verified.
  - CHANNEL-0 PDU DECODED + ROUTED (gap 3, RESOLVED 2026-07-04): the 10 head bytes
    `[0, 16, 0, 0, 6, 0, 11, 0, 1, 0]` are NOT a Share Control / MCS PDU. bytes[0..2]
    = `0x1000` LE = the `flags` field of a `BasicSecurityHeader` = `AUTODETECT_REQ`
    (RSP is `0x2000`); the remaining `[06 00 0B 00 01 00]` is the auto-detect request
    header (headerLength=6, seq=0x000B, RTT request). So GRD sends *continuous
    (in-session)* network auto-detect requests on the MCS **message channel** (which
    the IronRDP client never joins — GCC `message_channel: None`), so they surface in
    `x224/mod.rs` on an unrecognized channel decoded as `0`, framed with a
    `BasicSecurityHeader` (NOT a Share Data PDU, so the existing in-session
    `ShareDataPdu::AutoDetectReq` handler never sees them).
    Fix (committed): `x224::Processor::process` now routes unrecognized channels to
    `process_unrouted_channel`, which reuses the connector's connect-time responder
    (`connect_time_autodetect::in_session_autodetect_response`) to answer the
    security-header-framed auto-detect request, and TOLERATES (logs + drops) any other
    channel-`0` traffic instead of fatally aborting. Non-zero unknown channels keep
    the hard error. Exposed `pub mod connect_time_autodetect` +
    `pub struct ConnectTimeAutoDetectRsp` + `pub fn in_session_autodetect_response`.
    VALIDATED live: the `[X224] unexpected channel received: ID 0` fatal error is GONE
    (client log: "Answering in-session (message-channel) Auto-Detect Request
    channel_id=0"; unexpectedChannel count 2 -> 0). dtm-work regression PASS with the
    flag on (session-rendering, 76 frames) and off (67 frames).
  - DEEPER BLOCKER remains (why `network_autodetect` STILL stays OFF by default):
    even with channel 0 handled, `--network-autodetect` against GRD now fails on the
    HANDOVER reconnect — GRD's handover (FreeRDP) instance runs a
    deactivation-reactivation and sends an MCS Disconnect Provider Ultimatum
    (UserRequested) ~20 ms after the connection re-establishes, so no frame renders
    (client error: `[read deactivation-reactivation sequence step] ... received
    disconnect provider ultimatum: UserRequested`). Proven NOT to be the channel-0
    handling: a controlled A/B on a freshly-restarted GRD shows `--egfx` WITHOUT
    autodetect renders (26 frames) while `--egfx --network-autodetect` tears down, and
    a tolerate-only build (channel-0 request acknowledged but NOT answered) tears down
    identically. So the trigger is **advertising `SUPPORT_NET_CHAR_AUTODETECT` on the
    handover reconnect itself** (the handover FreeRDP instance's connect-time
    auto-detect / reactivation flow), not the message-channel response. This is a
    GRD-handover-specific incompatibility that needs its own investigation (likely: do
    NOT re-advertise autodetect on the redirected reconnect, or make the reactivation
    sequence tolerate the handover instance's post-autodetect PDU ordering). Note the
    channel-0 response is currently sent on the I/O channel (the client never joined a
    message channel to reply on); harmless for dtm-work and neutral for the GRD
    teardown, but revisit if a message-channel reply is ever required.
  - NET (gap 3): the literal ask — decode + route/tolerate MCS channel 0 — is DONE,
    tested, and safe (all changes gated behind the opt-in `--network-autodetect`;
    default path byte-identical, dtm-work + GRD render unaffected). Server-side audio
    ENABLE is proven (audio-disable journal line gone with the flag). `network_autodetect`
    stays OFF by default per the "else leave off + document" fallback, because full
    GRD audio needs the deeper handover-reactivation blocker above resolved first.
- Audio playback itself remains unconfirmable headlessly (no output device to
  hear).

3. dtm-work (standard Windows RDP) baseline: WORKS. NLA/HYBRID_EX via Credential
Manager, connect + first frame presented 1920x1080, session held. The `-d
<machine>` "trips IronRDP" claim did NOT reproduce on the fresh binary (`-u david
-d dtm-work` connected and held) — workaround appears no longer needed.

## Immediate next batch

This is the next concrete implementation queue, not a wish list.

1. Make the Hyper-V e2e suite a better Windows interaction lab before mirroring it to a second machine.
Refs: `scripts/windows/Invoke-HyperVE2ESuite.ps1`, `scripts/windows/Invoke-HyperVLiveConnectTest.ps1`, `docs/windows-native-install.md`, `crates/ironrdp-client/src/clipboard.rs`, `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-client/src/session_driver.rs`.
Why now:
- the suite is already producing useful transport/render data
- it now has explicit per-scenario health summaries plus staged clipboard/audio observations
- the default regression path is now stable via direct WinRM-backed file writes, so the remaining interaction gap is specifically “how do we launch a real interactive in-session workload when we want one?”
Done when:
- the default file-write workload remains green and a second optional workload reaches the active interactive guest session or an equivalent UI-driving path
- clipboard text transfer is asserted honestly end-to-end or explicitly documented as still local-path-only
- guest-side audio activity is exercised deliberately as part of the suite and correlated with app-driven interactive workload behavior
- unsupported device redirection stays explicit in the summary rather than implied

2. Tighten the Hyper-V suite’s diagnosis thresholds and render/transport attribution.
Refs: `scripts/windows/Invoke-HyperVE2ESuite.ps1`, Hyper-V suite logs under `%TEMP%\\ironrdp-hyperv-suite-*`.
Why now:
- the suite now classifies dominant bottlenecks using cadence comparison plus immediate-draw pressure, but the remaining thresholds still need one more pass against the refreshed client metrics
- the current branch still needs better attribution for “healthy but still expensive” present-path runs and RDPSND underrun-heavy scenarios
Done when:
- diagnosis distinguishes truly healthy idle workloads from present-cost-heavy steady-state workloads
- scenario summaries can call out when render, decode, or transport costs dominate even without hard failures
- suite rollups surface the worst diagnosis signals directly instead of only the primary class

3. Add focused runtime tests for the newer client/server session seams.
Refs: `crates/ironrdp-testsuite-extra`, `crates/ironrdp-server/src/session_driver.rs`, `crates/ironrdp-client/src/session_driver.rs`.
Why now:
- recent reliability changes need narrow tests, not just broad smoke coverage
- the decompressor regression test is in place as a unit-level guardrail, but the integration-level test in `ironrdp-testsuite-extra` that negotiates compression during reactivation is still missing
- server seam tests already cover resize reactivation, display-write failure, and disconnect parsing, but integration coverage is still thin
Done when:
- backlog disconnect, display failure, resize/reactivation, and single-session behavior are pinned down
- the decompressor regression has both a unit guardrail (done) and an integration-level test
Progress: graceful disconnect, display write failure, and double reactivation tests added (item 44). Remaining: single-session rejection test, decompressor regression integration test.

4. Mirror the now-validated Hyper-V deployment and live-connect flow onto `dtm-p1gen7`.
Refs: `build.ps1`, emitted artifact manifests, `scripts/windows/Install-IronRdpPackage.ps1`, `scripts/windows/Invoke-IronRdpSmokeTest.ps1`, `scripts/windows/Invoke-HyperVLiveConnectTest.ps1`, `scripts/windows/Deploy-IronRdpRemote.ps1`.
Why now:
- the portable bundle and bounded live client session are now proven locally, so the next deployment unknown is the real second machine
Done when:
- package output can be copied, launched, and verified remotely with one documented flow
- the Hyper-V-validated install/smoke/live-connect flow is mirrored on `dtm-p1gen7`

7. Clean up the native audio path after the first honest Hyper-V playback-observed runs.
Refs: `crates/ironrdp-rdpsnd-native/src/cpal.rs`, Hyper-V suite logs under `%TEMP%\\ironrdp-hyperv-suite-*`.
Why now:
- the suite now proves that the RDPSND path can reach `playback-observed`
- the latest live logs also surfaced Opus decode and closed-channel shutdown noise that should be treated as a real client-quality issue, not ignored test chatter
Done when:
- Opus decode failures are understood and either fixed or downgraded to clearly classified unsupported cases
- closed-channel teardown noise is removed from expected shutdown paths
- audio underrun metrics are still captured, but no longer hide shutdown/decoder correctness issues
Progress: Opus errors downgraded to warn with decode_error_count (item 42), shutdown noise eliminated, silence fill on underrun. Remaining: investigate root cause of Opus decode failures (codec mismatch? truncated packets?) and validate underrun reduction with real audio workload.

5. Use the Hyper-V live/e2e logs to drive the next standards-first render and transport optimizations.
Refs: `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-client/src/presentation.rs`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-server/src/gfx.rs`, local Hyper-V live-connect and suite logs.
Why now:
- the Hyper-V traces now show where the client and server are actually spending time
- the new client metrics now split surface acquire time from conversion/present time and expose redraw pressure directly
- the observed workload is still software bitmap heavy, so deeper work should stay grounded in measured data
Done when:
- there is a clear follow-up plan for the `16`-bpp bitmap path, surface acquisition vs conversion cost, and EGFX/H.264 readiness
- reconnect causes and graceful vs hard termination stay explicit in logs/tests
- the reason multitransport remains TCP-only in this environment is explicitly understood, not just observed

6. Split the Windows acceleration plan into two tracks and keep them separate in implementation.
Refs: `crates/ironrdp-client`, `crates/ironrdp-server/src/gfx.rs`, `crates/ironrdp-egfx`, future Windows-only streaming experiments.
Why now:
- the repo already supports standards-based acceleration ideas such as EGFX and multitransport negotiation
- Gemini-style IDD / GPU-P / custom UDP video ideas are better treated as a separate Windows streaming subsystem, not as accidental scope creep inside the core RDP path
Done when:
- the roadmap and docs keep standards-first RDP acceleration separate from any future custom streamer mode

## Priority 0: Lock the Windows build contract

1. Publish one crisp support matrix.
Refs: `README.md`, `xtask/README.md`, `ffi/README.md`, `build.ps1`.
Do next:
- define required vs optional Windows build tools
- document portable vs host-tuned artifact classes
- document when LLVM/lld, oneAPI, CUDA are only advisory
Effort: small.

2. Guarantee FFI demo outputs are self-contained for another Windows machine.
Refs: `build.ps1`, `xtask/src/ffi.rs`, `ffi/dotnet/Devolutions.IronRdp.targets`, `ffi/dotnet/Devolutions.IronRdp.ConnectExample/*.csproj`.
Do next:
- validate the native DLL inclusion contract
- validate publish folder layout
- add a manifest check for required runtime files
Effort: medium.

3. Finish the CargoTools/ProfileUtilities/MachineConfiguration contract for this fork.
Refs: `build.ps1`, local module environment, emitted manifests.
Do next:
- document which module provides which setting on each machine
- keep fallback behavior deterministic when optional modules are missing or broken
- confirm cache/artifact roots across this machine and `dtm-p1gen7`
Effort: medium.

4. Keep portable release settings distinct from workstation-only tuning.
Refs: `build.ps1`, `.cargo/config.toml`.
Do next:
- reserve `target-cpu=native` for machine-local builds
- keep portable `win-x64` outputs conservative
- mark the class clearly in artifact manifests and docs
Effort: small.

5. Pin the Windows developer toolchain story.
Refs: `rust-toolchain.toml`, `build.ps1`, `ffi/dotnet/*`.
Do next:
- decide on a repo `global.json` for .NET SDK pinning
- state the expected MSVC/NASM/Ninja/Clang availability by build mode
- keep oneAPI/CUDA documented as optional overlays
- make `build.ps1 -Mode doctor` explicitly surface `stable`-alias drift because CargoTools wrapper paths currently use `rustup run stable cargo`
Effort: small.

## Priority 1: Runtime correctness before acceleration

1. ~~Make single-session server behavior an explicit fork contract.~~ Done (item 40).

2. ~~Finish focused runtime tests around the session seams.~~ Done (P1.2 complete).
8 integration tests pass: client_server, deactivation_reactivation, graceful_disconnect,
display_write_failure, double_reactivation, echo_virtual_channel, single_session_rejection,
decompressor_regression.

3. ~~Make display and bitmap constraints fail early and clearly.~~ Done (item 41).

4. Extend Unicode/IME coverage from unit tests into end-to-end Windows validation.
Refs: `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-testsuite-extra`, deployment/demo notes.
Effort: medium.

5. ~~Finish reconnect/shutdown clarity.~~ Done (item 43).
Refs: `crates/ironrdp-client/src/main.rs`, `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-client/src/session_driver.rs`.
Effort: medium.

## Priority 2: Windows performance, acceleration, and transport groundwork

1. Forward dirty rectangles and reduce full-frame copy waste.
Refs: `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-client/src/presentation.rs`.
Problem:
- `ActiveStageOutput::GraphicsUpdate(InclusiveRectangle)` carries per-update dirty regions but they are discarded in `session_driver.rs` (the `_region` variable)
- `copy_rgba_frame` unconditionally copies the entire framebuffer: 7.9 MB/frame at 1920x1080 = ~475 MB/s wasted memory bandwidth
- `softbuffer` also converts every pixel even when only a small tile changed
Agent analysis: this is the #1 optimization opportunity ahead of any GPU backend work.
Effort: small to medium (in progress).

2. Keep the presentation backend seam stable and use it as the entry point for Windows acceleration experiments.
Refs: `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-client/src/presentation.rs`.
Do next:
- keep `softbuffer` as the default implementation
- add diagnostics that compare emit-to-present latency, backend acquisition, backend conversion, and present cost
- make later Windows GPU experiments additive rather than another app rewrite
Effort: medium.

3. Baseline CPU-first performance on Intel hardware before chasing GPU work.
Refs: `build.ps1`, `crates/ironrdp-client`, `crates/ironrdp-server/src/encoder/*`, `benches/`.
Do next:
- compare portable vs host-tuned builds
- validate allocator/linker/job-count choices against wall-clock data
- capture baseline scenarios for this machine and `dtm-p1gen7`
Effort: medium.

4. Add network-aware tuning guidance and measurements.
Refs: `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-server/src/session_driver.rs`, `crates/ironrdp-server/src/encoder/mod.rs`.
Do next:
- define LAN vs WAN profiles
- review flush cadence, request sizing, batching, and reconnect behavior
- expose only the knobs that are stable enough to support
Effort: medium.

5. Improve audio latency and underrun behavior on Windows.
Refs: `crates/ironrdp-rdpsnd-native/src/cpal.rs`.
Do next:
- make buffer sizing configurable or adaptive
- validate playback stability on different Windows endpoints
- add metrics around underruns
Effort: medium.

6. Make standards-based transport acceleration real before custom transport ideas.
Refs: `crates/ironrdp-client/src/config.rs`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-session/src/active_stage.rs`.
Do next:
- extend focused tests around the multitransport advertise/abort path into runtime coverage
- decide the first real UDP posture to support (`UDP_FECR` first, lossy later)
- keep unsupported cases explicit on the TCP control path
Effort: medium to large.

7. Evaluate Intel iGPU acceleration as a scoped experiment, not a default path.
Refs: future Windows-native render/encode experimentation.
Guardrails:
- no hard dependency in the default build
- no regression for CPU-only systems
- keep the experiment separate from the portable baseline
Effort: medium.

8. Keep NVIDIA/CUDA optional and isolated.
Refs: future packaging/docs/experiments.
Guardrails:
- do not require CUDA for normal builds
- only pursue if CPU and software-path wins are exhausted first
Effort: small.

9. Decide where LLVM/lld and oneAPI materially help, using measurements.
Refs: `build.ps1`, local machine configuration, future benchmark notes.
Effort: medium.

## Priority 3: Upstream fork integration

Tracked in `.claude/plans/upstream-fork-integration-plan.md`. Organized into
three tracks with prioritized phases based on upstream fork analysis.

### Track A: Authentication & credential management

1. ~~Port CredentialValidator trait for server-side auth.~~ Done.
`CredentialValidator` trait added to ironrdp-server, wired into accept_finalize.

2. ~~Port dynamic credential provider for CredSSP/NLA path.~~ Done.
`CredentialProvider` trait added to ironrdp-acceptor with provider -> static -> allow chain.

3. NTLM fallback when Kerberos is unavailable.
Source: formalco fork + upstream PR #1143 (ramnes).
Status: blocked — sspi/picky pins rand_core to an RC version.
Do next: wait for sspi stable release, then port NtlmConfig server mode.
Effort: small (once unblocked).

### Track B: Graphics acceleration pipeline

0. gnome-remote-desktop 46 session-handover redirection (PRIMARY BLOCKER to
pixels against GRD; supersedes the earlier "EGFX version=0" framing, which was
a benign warning — GRD/FreeRDP advertises General capset protocolVersion=0).
Status: partially done (commit "feat(session): handle RDP Server Redirection
PDU (GRD 46 handover)").
What was found: GRD 46's *system* daemon authenticates the initial connection
(NLA/CredSSP, damartel PAM password, from Windows Credential Manager
`TERMSRV/100.64.0.3`), reaches finalization, then sends a Standard RDP Server
Redirection PDU (`PDUTYPE_SERVER_REDIR_PKT`=0xA, [MS-RDPBCGR] 2.2.13.1.1) to
hand the client over to the spawned user session. Journal: `[RDP] Sending
server redirection` / `[DaemonSystem] ... handover`. GRD's RedirFlags =
`0x0001C016` = LB_LOAD_BALANCE_INFO | LB_USERNAME | LB_PASSWORD |
LB_PASSWORD_IS_PK_ENCRYPTED | LB_REDIRECTION_GUID | LB_TARGET_CERTIFICATE, with
NO target net address (reconnect to same host:port). The routing token
(LoadBalanceInfo) is 25 bytes.
Done: parse the redirection PDU, thread it up (ProcessorOutput/ActiveStageOutput
::Redirect), and reconnect carrying the LoadBalanceInfo verbatim as an X.224
routing token (NegoRequestData::Raw). The reconnect's negotiation IS accepted
by GRD (HYBRID confirmed, TLS upgrades).
Also done: the reconnect now switches to the redirection-provided credentials
(LB_USERNAME / LB_PASSWORD) instead of the original login, because GRD's
handover instance authenticates against a private winpr NTLM SAM, not PAM.
Server-side proof (user-session journal, `org.gnome.RemoteDesktop.Handover`):
`[com.winpr.sspi.NTLM] ntlm_fetch_ntlm_v2_hash: Could not find user in SAM
database` when the original `damartel` username was sent. The redirection
LB_USERNAME is a random per-handover cookie (e.g. "`@%PG..."), and LB_PASSWORD
has PASSWORD_IS_PK_ENCRYPTED set.

HANDOVER NOW COMPLETES (resolved). The earlier "PK-encrypted password"
theory was WRONG — confirmed by reading the GRD 46.3 source:
- grd-session-rdp.c `grd_session_rdp_send_server_redirection` sets
  LB_PASSWORD_IS_PK_ENCRYPTED but sends the password as *plaintext UTF-16LE*
  (`get_utf16_string(password)`); it never encrypts. The flag is cosmetic.
- grd-rdp-sam.c `create_sam_string` stores NTOWFv1(password) for `username`
  in the handover instance's winpr NTLM SAM.
So the client must reconnect with username=LB_USERNAME and password=LB_PASSWORD
decoded as UTF-16LE (regardless of the PK flag), plus the LoadBalanceInfo as an
X.224 routing token. The routing token is literally `Cookie: msts=<n>\r\n`
(CRLF-terminated; GRD peeks for 0x0D0A in grd-rdp-routing-token.c), sent
verbatim via NegoRequestData::Raw.
Verified end-to-end against GRD 46.3 on asuspro13: no more "Could not find
user in SAM"; the handover connection authenticates, and the EGFX pipeline
establishes — caps confirmed (AVC420), surface created 1920x1080 + mapped,
StartFrame/EndFrame frames flowing. Reproduced across multiple runs.
NOTE: GRD's handover subsystem is flaky server-side (ZINK/EGL "failed to
choose pdev" on the headless GNOME; the handover instance sometimes fails to
start / stops sending redirections until `systemctl [--user] restart
gnome-remote-desktop.service`). This is an asuspro13 environment issue, not a
client bug.

VISIBLE PIXELS — DONE (2026-07-04). GRD sends graphics via the RemoteFX
**Progressive** codec inside RDPGFX_WIRE_TO_SURFACE_PDU_2. `WireToSurface2` is
now decoded and composited, and a full 1920x1080 desktop renders from live
GRD 46.3 (100.64.0.3).

PORTED (not reimplemented) from upstream — the fork was 139 commits behind and
upstream already had this exact capability. Straight file adds + module
registration (the fork reworked egfx/rfx, so a whole-commit cherry-pick would
conflict on server.rs, which is server-side and not needed for client decode):
- `crates/ironrdp-pdu/src/codecs/rfx/progressive.rs` — progressive block-stream
  parser (SYNC/CONTEXT/FRAME/REGION/TILE_SIMPLE|FIRST|UPGRADE). From #1196
  (49099f0c), final form from #1197 (a142799d). Registered `pub mod progressive`.
- `crates/ironrdp-graphics/src/{dwt_extrapolate,srl}.rs` — reduce-extrapolate DWT
  + SRL primitives for progressive refinement. From #1196.
- `crates/ironrdp-graphics/src/progressive.rs` — `ProgressiveDecoder` with
  per-`codec_context_id` tile state; SIMPLE full-quality **and** FIRST/UPGRADE
  refinement. From #1197. Adapted `alloc::collections` -> `std::collections`
  (fork's ironrdp-graphics is std, not no_std+alloc like upstream).
- Applied the #1395 (368fe8e6) fix: GNOME Remote Desktop omits the CONTEXT
  block on every frame after the first; cache `use_reduce_extrapolate` per
  context so later frames don't fail with `MissingBlock("CONTEXT")` (this fix
  is load-bearing — without it only the coarse first frame renders).
- Wired into `GraphicsPipelineClient::handle_wire_to_surface2`: decode tiles ->
  blit into a persistent per-surface RGBA framebuffer -> deliver the full
  framebuffer via `on_bitmap_updated` (renderer Image path is full-frame).
  Progressive state reset on ResetGraphics; context freed on
  DeleteEncodingContext. Skipped the upstream server.rs encode changes.
Evidence: first frame composited **510 tiles** (full 1920x1080), dumped and
visually verified as the Ubuntu GDM login screen (clock, user field, Ubuntu
logo; all RGB channels correct). Subsequent 1-tile incremental frames decoded
with no CONTEXT block and no errors; session stayed up. Diagnostic dump gated
behind `IRONRDP_EGFX_DUMP=<path>` (writes raw RGBA + a `.dims` sidecar).
Remaining refinement:
- AVC444 dual-stream decode for WireToSurface1 (re-enable V10.7 vs Windows).
- FIRST/UPGRADE tiles are ported and available but not yet exercised against a
  server that sends them (GRD used SIMPLE only in these runs).

4. ~~Enable H.264 decode in the native client EGFX pipeline.~~ Done.
`EgfxRenderHandler` replaces `LoggingEgfxHandler`, `openh264` feature gates decoder.

5. Port ClearCodec bitmap compression codec and client decode.
Source: upstream PRs #1174 + #1175 (glamberson, ~5600 lines).
Refs: `crates/ironrdp-graphics/`, `crates/ironrdp-egfx/src/client.rs`.
Do next:
- Port ClearCodec decoder from #1174 into ironrdp-graphics
- Port EGFX client dispatch additions from #1175
- Both are additive (new files, new match arms)
Effort: medium.

6. Port ZGFX O(1) hash table compression optimization.
Source: glamberson fork (commits a0eacc50, 4a93ffae, 57608dad).
Refs: `crates/ironrdp-graphics/src/zgfx/`.
Do next:
- Read hash table implementation from glamberson fork
- Port optimization into our zgfx module layout (too diverged for cherry-pick)
- Also port duplicate-entry fix and size limits
Effort: medium.

7. Direct2D presentation backend.
Source: original fork work.
Refs: `crates/ironrdp-client/src/presentation.rs`, `crates/ironrdp-client/src/app.rs`.
Do next:
- Implement `PresentationBackend` trait using `ID2D1HwndRenderTarget`
- Eliminate softbuffer conversion step
- Keep softbuffer as fallback for non-Windows or headless
Effort: medium to large.

### Track C: Device redirection

8. Implement drive redirection backend.
Source: original fork work.
Refs: `crates/ironrdp-rdpdr/src/backend/`, `crates/ironrdp-rdpdr-native/`.
Do next:
- Implement `handle_drive_io_request()` for IRP_MJ_CREATE/READ/WRITE/CLOSE/QUERY
- Add `--redirect-drive <name>=<path>` CLI flag
- Wire device announcement during connection setup
- Hyper-V validation: file copy from host to guest
Effort: large.

9. Port clipboard file transfer support.
Source: upstream PR #1166 (gabrielbauman, 93 files — manual port of cliprdr additions only).
Refs: `crates/ironrdp-cliprdr/src/`, `crates/ironrdp-cliprdr-native/`.
Do next:
- Port `request_file_contents()`, `SendFileContentsResponse`, data locking from PR
- Skip web/FFI changes
- Implement native backend using Windows clipboard APIs
Effort: medium.

10. ~~Port USB redirection PDU definitions.~~ Done.
URBDRC PDU structures (header, caps, device, channel, TS_URB) added to ironrdp-rdpeusb.

11. ~~Auto-Detect RTT measurement.~~ Done.
Client-side auto-detect PDU handling added to x224 session layer.

## Priority 3: Windows-native feature parity and usability

1. Decide whether end-to-end EGFX/H.264 becomes a first-class Windows track.
Refs: `crates/ironrdp-server/src/gfx.rs`, `crates/ironrdp-egfx`, native client graphics path.
Guardrails:
- prove value on Windows workloads
- keep classic bitmap/RemoteFX compatibility paths intact
Agent analysis findings:
- EGFX client handler is currently a no-op stub: `handle_pdu` traces and returns, no GFX PDU affects the framebuffer
- H.264 decode is entirely absent client-side (no openh264, ffmpeg, or Media Foundation)
- server EGFX is substantially complete (AVC420/444, ZGFX, surface lifecycle, backpressure)
- recommended first step: advertise `AVC420_ENABLED` capability and observe whether the Hyper-V server switches from bitmap to EGFX traffic (2-hour experiment, not a multi-week feature)
- the `PresentationBackend` trait needs dirty-region and format-hint extensions before a GPU decode path is practical
Effort: medium to large.

2. Add real Windows device redirection beyond the current `NoopRdpdrBackend`.
Refs: `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-rdpdr`, `crates/ironrdp-rdpdr-native`.
Initial scope:
- drive redirection
- printer redirection if practical
- smartcard cleanup if already close
Effort: medium to large.

3. Define the Windows-only strategy for USB-class or vendor-specific devices.
Refs: `crates/ironrdp-dvc-com-plugin`, `crates/ironrdp-dvc-pipe-proxy`.
Likely direction:
- DVC/COM plugin bridge instead of trying to force everything through generic `RDPDR`
Effort: medium.

4. Defer Gemini-style IDD / VDD / GPU-P / DDA / hardware-encoder streaming ideas into a separate Windows streaming track.
Refs: future Windows-only capture/encode subsystem, Hyper-V or workstation experiments, `gateway.TODO.md` for control-plane style notes when relevant.
Why defer:
- these ideas are closer to a Parsec-like streamer than to standards-based RDP
- they likely require WDK driver work, encoder integration, and a custom transport
- they should reuse IronRDP for auth/session control only if they prove worth the complexity
Effort: large and separate from the core RDP branch.

## Priority 4: Deployment and operator experience

1. Keep the local Hyper-V Windows Server validation target as a repeatable regression harness.
Refs: local Hyper-V host tooling, `build.ps1`, `scripts/windows/Invoke-HyperVInstallerTest.ps1`, `scripts/windows/Invoke-HyperVLiveConnectTest.ps1`.
Do next:
- preserve the current Windows Server 2025 guest as the first clean-machine packaging regression target
- keep the VM powered on after installer validation unless a reboot or offline staging step is actually required
- keep the PowerShell Direct validation path working with the temporary local admin test account
- keep collecting richer live-connect data: guest logs, service state, port reachability, first-frame timing, and transport/codec behavior
- keep tracking which guest IP/path is actually reachable from the host so later `dtm-p1gen7` smoke runs use the same discipline
Effort: medium.

2. Turn `dtm-p1gen7` into a repeatable smoke-deploy target.
Refs: SSH deploy path, `build.ps1`, emitted manifests.
Effort: medium.

3. Keep the installer layer small and release-shaped.
Refs: `build.ps1`, `scripts/windows/New-IronRdpInstallers.ps1`, `.github/workflows/windows-release.yml`, `docs/windows-native-install.md`.
Do next:
- keep release outputs limited to portable zip, MSIX, MSI, App Installer, manifest, and trust material
- remove or suppress intermediate installer layout artifacts from release-facing outputs
- keep local `build.ps1 -Mode package|publish` behavior aligned with the GitHub tag workflow
Effort: medium.

4. Finish the .NET package and demo-distribution story.
Refs: `ffi/dotnet/Devolutions.IronRdp/*.csproj`, `ffi/README.md`.
Effort: medium.

## Priority 5: Boundary cleanup with clear payoff

1. Simplify the FFI connector API.
Refs: `ffi/src/connector/mod.rs`, `ffi/src/connector/config.rs`.
Effort: medium.

2. Move presentation-specific knobs out of `connector::Config`.
Refs: `crates/ironrdp-connector/src/lib.rs`, `crates/ironrdp-client/src/config.rs`, `ffi/src/connector/config.rs`.
Effort: medium.

3. Reduce `ironrdp-session` coupling to `ironrdp-connector`.
Refs: `crates/ironrdp-session/Cargo.toml`, `crates/ironrdp-session/src/lib.rs`.
Effort: medium.

4. Decide which unpublished crates stay intentionally private in this fork.
Refs: `crates/ironrdp-propertyset/Cargo.toml`, `crates/ironrdp-cfg/Cargo.toml`, `crates/ironrdp-mstsgu/Cargo.toml`, `crates/ironrdp-rdpfile/Cargo.toml`, `crates/ironrdp-egfx/Cargo.toml`.
Effort: small.

## Deferred strategic refactors

1. Turn `ironrdp-pdu` debt into scoped tracked work.
Refs: `ARCHITECTURE.md`, `crates/ironrdp-pdu/Cargo.toml`, `crates/ironrdp-pdu/README.md`.
Effort: medium to large.

2. Split `ironrdp-graphics` into smaller crates only after the Windows deployment path settles.
Refs: `ARCHITECTURE.md`, `crates/ironrdp-graphics/Cargo.toml`, `crates/ironrdp-graphics/src/lib.rs`.
Effort: large.

## Suggested execution order

1. Lock the supported build matrix and artifact-class contract.
2. Finish reconnect/shutdown clarity and add focused runtime seam tests.
3. Instrument emit-to-present latency and backend acquisition/conversion/present timing on the native client.
4. Make multitransport groundwork explicit and measurable before real UDP work.
5. Measure portable vs host-tuned Intel builds on both primary machines.
6. Mirror the no-repo install path onto `dtm-p1gen7`.
7. Extend Unicode/IME validation into end-to-end Windows smoke coverage.
8. Revisit optional Intel iGPU, EGFX, UDP/multitransport, LLVM/lld, oneAPI, and CUDA work only after the CPU/software baseline is measured and stable.
9. Take on the next connector/session/FFI boundary cleanup.
10. Keep gateway work in [gateway.TODO.md](C:/codedev/IronRDP/gateway.TODO.md) until the direct machine-to-machine path is stronger, and keep any Gemini-style custom streaming ideas out of the core RDP track until a separate subsystem is justified.
