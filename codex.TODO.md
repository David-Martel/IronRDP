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

1. Make single-session server behavior an explicit fork contract.
Refs: `crates/ironrdp-server/src/server.rs`, `crates/ironrdp-server/README.md`.
Effort: small to medium.

2. Finish focused runtime tests around the session seams.
Refs: `crates/ironrdp-testsuite-extra/tests/*`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-server/src/session_driver.rs`.
Effort: medium.

3. Make display and bitmap constraints fail early and clearly.
Refs: `crates/ironrdp-server/src/encoder/bitmap.rs`, `crates/ironrdp-server/src/encoder/mod.rs`, `crates/ironrdp-server/src/session_driver.rs`.
Effort: medium.

4. Extend Unicode/IME coverage from unit tests into end-to-end Windows validation.
Refs: `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-testsuite-extra`, deployment/demo notes.
Effort: medium.

5. Finish reconnect/shutdown clarity.
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
