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

## Immediate next batch

This is the next concrete implementation queue, not a wish list.

1. Add focused runtime tests for the newer client/server session seams.
Refs: `crates/ironrdp-testsuite-extra`, `crates/ironrdp-server/src/session_driver.rs`, `crates/ironrdp-client/src/session_driver.rs`.
Why now:
- recent reliability changes need narrow tests, not just broad smoke coverage
- the frame-buffer reuse and IME work now have unit coverage
- server seam tests now cover resize reactivation, display-write failure, and disconnect parsing, but integration coverage is still thin
Done when:
- backlog disconnect, display failure, and single-session behavior are pinned down

2. Add a repeatable deploy-and-smoke-test path for `dtm-p1gen7`.
Refs: `build.ps1`, emitted artifact manifests, `scripts/windows/Install-IronRdpPackage.ps1`, `scripts/windows/Invoke-IronRdpSmokeTest.ps1`.
Why now:
- the portable bundle is now proven on a clean Windows Server guest, so the next deployment unknown is the real second machine
Done when:
- package output can be copied, launched, and verified remotely with one documented flow
- the Hyper-V-validated portable install/smoke flow is mirrored on `dtm-p1gen7`

3. Keep reconnect/shutdown behavior explicit before deeper transport work.
Refs: `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-client/README.md`.
Why now:
- UDP/multitransport and GPU work should build on a predictable runtime contract
- resize-triggered reconnects should fail clearly when the negotiated size never changes
Done when:
- reconnect causes are explicit in logs/tests
- graceful close and hard session failure remain distinct at the top-level client boundary

4. Split the Windows acceleration plan into two tracks and keep them separate in implementation.
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

1. Instrument and measure the remaining backend-local surface conversion cost end to end.
Refs: `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-client/src/presentation.rs`.
Problem:
- the extra packed staging buffer is gone, and the current presenter loop has been simplified
- `softbuffer` still requires a full RGBA-to-surface-word conversion during present
- queue latency, surface acquisition, and backend conversion cost now need to be separated before deeper GPU work
Effort: medium.

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
Refs: local Hyper-V host tooling, `build.ps1`, `scripts/windows/Invoke-HyperVInstallerTest.ps1`.
Do next:
- preserve the current Windows Server 2025 guest as the first clean-machine packaging regression target
- keep the VM powered on after installer validation unless a reboot or offline staging step is actually required
- keep the PowerShell Direct validation path working with the temporary local admin test account
- add richer runtime validation on the live guest: guest logs, service state, port reachability, and eventually real client session traces
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
