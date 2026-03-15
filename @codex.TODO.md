# Codex TODO

This file tracks David-Martel-owned follow-up work for the Windows/server-focused fork.
The upstream IronRDP codebase is now an input, not the deployment target: changes here should improve the quality, maintainability, and deployability of the Windows-only fork across multiple machines.

Current baseline:
- Branch: `windows-server-only`
- Fork owner: `David-Martel`
- Latest pushed commit at the time of this update: `a5006bee`
- Current release posture: signed commits, public fork, Windows-native build path centered on `build.ps1`, Rust native client, and the .NET/FFI surface

## Recently completed

1. Platform-specific CI/build drift was cleaned up.
Refs: `xtask/src/main.rs`, `xtask/src/cov.rs`, `xtask/src/check.rs`, `ARCHITECTURE.md`, `Cargo.toml`.
Status: done.
Notes: `cargo xtask ci` no longer assumes removed web-era paths or old platform behavior, stale coverage filters were removed, and the repo metadata better reflects the trimmed fork.

2. Excluded crates were reclassified away from generic “fix compilation” debt.
Refs: `Cargo.toml`, `AGENTS.md`, `ARCHITECTURE.md`.
Status: done.
Notes: the fork now treats these as intentionally parked/legacy surfaces instead of pretending they are active near-term work.

3. The Windows build/deployment path was upgraded.
Refs: `build.ps1`, `.cargo/config.toml`, `Cargo.toml`, `xtask/src/ffi.rs`.
Status: done.
Notes: `build.ps1` now uses the CargoTools environment for optimized Windows builds, `sccache`/linker acceleration, and FFI helper setup.

4. FFI boundary hardening landed.
Refs: `ffi/src/log.rs`, `ffi/src/connector/mod.rs`, `ffi/dotnet/Devolutions.IronRdp/src/Connection.cs`.
Status: mostly done.
Notes: panic-prone paths were removed from the exposed FFI surface, and the managed connection helper no longer blindly accepts any TLS certificate by default.

5. The foundational `std`/`no_std` story was clarified instead of left misleading.
Refs: `crates/ironrdp-propertyset/Cargo.toml`, `crates/ironrdp-svc/Cargo.toml`, `crates/ironrdp-svc/src/lib.rs`, `crates/ironrdp-dvc/Cargo.toml`, `crates/ironrdp-rdpsnd/Cargo.toml`.
Status: done.
Notes: manifest/runtime feature wiring now better matches actual code paths.

6. The unsound native client window/display lifetime workaround was removed.
Refs: `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-client/src/main.rs`.
Status: done.
Notes: window, `softbuffer` context, and surface ownership now live in a dedicated state boundary without the fake `'static` lifetime hack.

7. The client runtime was split into clearer responsibilities.
Refs: `crates/ironrdp-client/src/rdp.rs`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-client/README.md`.
Status: done.
Notes: connection/bootstrap and live session processing are now separated.

8. The server runtime was split and the event dispatcher was isolated.
Refs: `crates/ironrdp-server/src/server.rs`, `crates/ironrdp-server/src/session_driver.rs`, `crates/ironrdp-server/README.md`.
Status: done.
Notes: listener/bootstrap logic is separated from accepted-session runtime, and the event-routing path now has narrower internal helpers.

9. The fork is now in a clean signed state suitable for continued development.
Refs: Git history on `windows-server-only`.
Status: done.
Notes: the local worktree was cleaned, signed commits were pushed, and the Codex handoff context was refreshed.

10. The Windows FFI/package build path is now quieter and more reliable.
Refs: `build.ps1`, `xtask/src/ffi.rs`, `ffi/dotnet/Devolutions.IronRdp/Devolutions.IronRdp.csproj`, `ffi/dotnet/Devolutions.IronRdp/README.md`, `ffi/README.md`.
Status: done.
Notes: `build.ps1` now owns the managed build sequencing, the `--skip-dotnet-build` path is honored, generated binding refresh is atomic instead of destructive, package readme metadata is present, generated-warning noise is suppressed in the package project, and FFI artifact discovery now respects `CARGO_TARGET_DIR`.

11. `dtm-p1gen7` access is now understood well enough to support deployment work.
Refs: `C:/Users/david/.codex/config.toml`, `C:/Users/david/OneDrive/Documents/PowerShell/.codex/config.toml`, `C:/Users/david/.codex/tmp/dtm-p1gen7-config.toml`.
Status: in progress.
Notes: local `rust-mcp-filesystem` roots were expanded to all mounted drives, a Codex restart is still required for that MCP config to reload, WinRM transport is reachable but still unauthorized, SSH access works, and remote dot-directory sync is now an SSH-first operation rather than a WinRM dependency.

12. The managed framing hot path was trimmed.
Refs: `ffi/dotnet/Devolutions.IronRdp/src/Framed.cs`.
Status: done.
Notes: the .NET stream wrapper now uses an async-safe write lock, avoids rebuilding the live buffer list on every exact read, and reduces per-read buffer churn in the hot path.

13. The .NET demo builds no longer depend on machine-local NuGet source state.
Refs: `ffi/dotnet/NuGet.Config`, `ffi/README.md`.
Status: done.
Notes: the repo now carries a local NuGet source policy using `nuget.org` plus the standard Visual Studio offline cache, which avoids restore failures caused by broken host-specific package sources.

## Priority 1: Next practical wins

1. Finish the remaining .NET package metadata and hand-written C# hygiene.
Refs: `ffi/dotnet/Devolutions.IronRdp/Devolutions.IronRdp.csproj`, `ffi/dotnet/Devolutions.IronRdp/src/*.cs`.
Problem: the package build is now clean, but the csproj still lacks some publisher metadata for long-term distribution, and `ImplicitUsings` is still enabled with a FIXME because the hand-written sources have not been made explicit yet.
Recommendation: add the remaining package metadata deliberately for the David-Martel fork, then add explicit usings in hand-written files and disable implicit usings in a separate pass.
Effort: small.

2. Finish the Windows-native client bootstrap basics.
Refs: `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-client/src/main.rs`, `crates/ironrdp-client/src/session_driver.rs`, `crates/ironrdp-client/src/rdp.rs`.
Problem: the client still has bootstrap gaps around initial sizing, Unicode input, reconnect/resize polish, and explicit exit/error behavior.
Recommendation: finish the user-visible startup and reconnect path before deeper internal refactors.
Effort: medium.

3. Add focused tests around the server runtime seam.
Refs: `crates/ironrdp-testsuite-extra/tests/mod.rs`, `crates/ironrdp-server/src/session_driver.rs`.
Problem: integration coverage exists, but it does not directly assert input delivery, backlog handling, clipboard/audio server events, or display-update failure modes.
Recommendation: add focused tests for input handler delivery, event dispatch families, and non-resize display update paths.
Effort: medium.

4. Make display and bitmap limitations fail explicitly instead of degrading implicitly.
Refs: `crates/ironrdp-displaycontrol/src/client.rs`, `crates/ironrdp-server/src/encoder/bitmap.rs`, `crates/ironrdp-server/src/server.rs`, `crates/ironrdp-server/src/encoder/mod.rs`.
Problem: unsupported display layouts and bitmap edge cases are still handled too late or too implicitly.
Recommendation: reject unsupported combinations earlier, clamp where safe, and document the enforced constraints in the server README/API docs.
Effort: medium.

5. Make the server connection model a first-class contract.
Refs: `crates/ironrdp-server/src/server.rs`, `crates/ironrdp-server/README.md`.
Problem: `RdpServer::run()` is still effectively single-client, but that behavior is not yet the explicit product contract for this fork.
Recommendation: either document and embrace single-session behavior for David-Martel’s use case, or move to task-per-connection deliberately.
Effort: medium.

6. Unify the Windows deployment pipeline around one authoritative path.
Refs: `build.ps1`, `xtask/src/ffi.rs`, `.github/workflows/nuget-publish.yml`, `ffi/dotnet/*`.
Problem: the local path is better defined now, but release automation still does not fully consume the same contract.
Recommendation: make `build.ps1` and `cargo xtask ffi build` the canonical local/CI entrypoints, then align publish workflows around them and document the single supported path.
Effort: medium.

7. Turn `dtm-p1gen7` into a repeatable demo/deployment target.
Refs: `C:/Users/david/.codex/tmp/dtm-p1gen7-config.toml`, local `~/.ssh`, future deployment scripts.
Problem: remote access works over SSH, but the current sync path is ad hoc and WinRM still is not authorized.
Recommendation: finish the dot-directory sync, extract the Rust best-practice guidance into the local agent/tooling environment, and script the demo deployment path so the updated client/server builds can be exercised between this machine and `dtm-p1gen7`.
Effort: medium.

## Priority 2: Boundary cleanup with deployment payoff

8. Simplify the FFI connector API.
Refs: `ffi/src/connector/mod.rs`, `ffi/src/connector/config.rs`.
Problem: the connector surface still exposes too much consumed-state machinery and internal ownership detail.
Recommendation: introduce clearer attach/build semantics and reduce exported stateful take/replace patterns.
Effort: medium.

9. Move presentation/rendering knobs out of `connector::Config`.
Refs: `crates/ironrdp-connector/src/lib.rs`, `crates/ironrdp-client/src/config.rs`, `ffi/src/connector/config.rs`.
Problem: client-specific rendering and pointer settings still leak into lower-level connector configuration.
Recommendation: move them into a higher-level client/session config boundary.
Effort: medium.

10. Reduce `ironrdp-session` coupling to `ironrdp-connector`.
Refs: `crates/ironrdp-session/Cargo.toml`, `crates/ironrdp-session/src/lib.rs`.
Problem: activation/session responsibilities are still not as cleanly separated as they should be.
Recommendation: extract the minimum shared activation/session state and narrow the dependency boundary.
Effort: medium.

11. Decide which unpublished crates are intentionally private in this fork.
Refs: `crates/ironrdp-propertyset/Cargo.toml`, `crates/ironrdp-cfg/Cargo.toml`, `crates/ironrdp-mstsgu/Cargo.toml`, `crates/ironrdp-rdpfile/Cargo.toml`, `crates/ironrdp-egfx/Cargo.toml`.
Problem: several crates still read as unresolved “publish later” debt.
Recommendation: replace the vague TODOs with an explicit private/internal policy where appropriate.
Effort: small.

## Priority 3: Strategic refactors

12. Turn broad `ironrdp-pdu` architecture debt into scoped tracked work.
Refs: `ARCHITECTURE.md`, `crates/ironrdp-pdu/Cargo.toml`, `crates/ironrdp-pdu/README.md`.
Problem: the crate is still too broad and still carries dependency debt described only in broad terms.
Recommendation: split the work into discrete dependency-removal and module-boundary tasks.
Effort: medium to large.

13. Split `ironrdp-graphics` into smaller crates and remove legacy helpers.
Refs: `ARCHITECTURE.md`, `crates/ironrdp-graphics/Cargo.toml`, `crates/ironrdp-graphics/src/lib.rs`.
Problem: graphics remains oversized and still mixes several concerns.
Recommendation: isolate codec- or format-specific code, then tighten local invariants and lint posture.
Effort: large.

## Deployment roadmap

1. Stabilize the current local build path.
Goal: one reproducible command for native Rust + FFI packaging on Windows.
Primary refs: `build.ps1`, `xtask/src/ffi.rs`.

2. Finish the .NET package metadata and make distribution semantics explicit.
Goal: reliable reusable library output for David-Martel-owned tools across multiple machines with a clear Windows-only identity.
Primary refs: `ffi/dotnet/Devolutions.IronRdp/*`.

3. Turn `dtm-p1gen7` into the first repeatable demo/deployment target.
Goal: validate real client/server builds and deployment steps across multiple David-Martel machines.
Primary refs: local SSH/deployment scripts, `build.ps1`, remote host config.

4. Add installer-oriented packaging once the runtime surface settles.
Goal: support self-contained EXE plus installer packaging from a stable build graph.
Primary refs: `build.ps1`, future packaging project/scripts.

5. Add CI validation for the David-Martel deployment path only.
Goal: validate the Windows/server fork directly instead of carrying upstream surface area that is no longer relevant.
Primary refs: `.github/workflows/*`, `build.ps1`.

## Suggested execution order

1. Finish the remaining .NET package metadata and hand-written C# hygiene.
2. Finish Windows-native client bootstrap polish.
3. Add focused runtime tests for server input, display update, and server-event routing.
4. Make display/bitmap limits and server connection behavior explicit.
5. Script and validate the `dtm-p1gen7` demo/deployment path.
6. Simplify the FFI connector path.
7. Narrow `connector`/`session` responsibilities.
8. Treat `ironrdp-pdu` and `ironrdp-graphics` as the next major refactor track after the Windows deployment path is stable.
