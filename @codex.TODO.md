# Codex TODO

This file prioritizes the easiest remaining fixes after the Windows/server-focused trim of the repository.
The recommendations were assembled from parallel read-only analysis passes over the workspace, runtime code, and Rust/.NET packaging surface.
The Windows/.NET packaging pass used the `rust-dll-csharp-cli` skill because the trimmed repo now leans heavily on the FFI path.

## Priority 1: Small, high-payoff fixes

1. Align `cargo xtask ci` with the actual CI contract.
Refs: `xtask/src/main.rs`, `xtask/src/ffi.rs`, `.github/workflows/ci.yml`.
Problem: `cargo xtask ci` now unconditionally runs Windows-only FFI steps even though the repo still runs generic checks on both Windows and Linux.
Recommendation: gate the FFI branch on Windows with an explicit skip elsewhere, or split `ci` into platform-neutral and Windows-specific variants.
Effort: small.

2. Remove stale web-era coverage and documentation drift.
Refs: `xtask/src/cov.rs`, `ARCHITECTURE.md`, `crates/ironrdp-rdcleanpath/Cargo.toml`, `AGENTS.md`.
Problem: coverage filters still reference deleted web paths, and `ironrdp-rdcleanpath` still describes itself in terms of the removed web client.
Recommendation: clean the coverage regex/globs, rewrite the `ironrdp-rdcleanpath` description around current consumers only, and move missing architectural facts out of `AGENTS.md` into `ARCHITECTURE.md`.
Effort: small.

3. Reclassify the excluded crates explicitly.
Refs: `Cargo.toml`, `AGENTS.md`.
Problem: the workspace still labels excluded crates as temporary compilation breakage even though they now look more like parked legacy surfaces.
Recommendation: replace the blanket `# FIXME: fix compilation` note with an intentional classification such as `legacy/unmaintained`, or link each excluded crate to a tracked issue.
Effort: small.

4. Stop panicking across the FFI boundary.
Refs: `ffi/src/log.rs`, `ffi/src/connector/mod.rs`.
Problem: `Log::init_with_env()` still documents that it panics, and the FFI connector path still contains an `expect(...)` on DRDYNVC initialization.
Recommendation: make logging initialization return a typed FFI error, store the result in a fallible one-time init path, and replace the `expect` with a checked error return.
Effort: small.

5. Stop unconditional TLS acceptance in the .NET direct-connection helper.
Refs: `ffi/dotnet/Devolutions.IronRdp/src/Connection.cs`.
Problem: the `SslStream` validation callback always returns `true`, so the managed helper bypasses certificate validation entirely.
Recommendation: add an explicit certificate-validation policy hook, default it to normal platform validation, and clean up the nearby nullable and generic-exception paths.
Effort: small.

6. Clean the obvious .NET build/package warnings while the surface is still small.
Refs: `ffi/dotnet/Devolutions.IronRdp/Generated/ConnectionActivationState.cs`, `ffi/dotnet/Devolutions.IronRdp/Generated/CredsspSequence.cs`, `ffi/dotnet/Devolutions.IronRdp/Generated/WinCliprdr.cs`, `ffi/dotnet/Devolutions.IronRdp/Devolutions.IronRdp.csproj`.
Problem: the managed package still emits `GetType()` name-collision and nullability warnings, and the project file still carries a stale `ImplicitUsings` FIXME and obsolete trim fallout.
Recommendation: fix the generation/post-generation path so optional Rust returns become nullable managed types, rename generated `GetType` methods consistently, disable `ImplicitUsings` or remove the FIXME, and remove stale platform-oriented conditions.
Effort: small to medium.

7. Remove the unsound native-client lifetime workaround.
Refs: `crates/ironrdp-client/src/app.rs`.
Problem: the native client currently uses `transmute` to coerce `DisplayHandle<'_>` to `DisplayHandle<'static>`, and the file itself states that the API is unsound as written.
Recommendation: refactor `App` ownership so the `softbuffer` context and surface are created and stored without needing a fake `'static` lifetime.
Effort: medium.

8. Finish the Windows-native client bootstrap basics.
Refs: `crates/ironrdp-client/src/main.rs`, `crates/ironrdp-client/src/app.rs`, `crates/ironrdp-client/src/rdp.rs`.
Problem: the initial window size is still hardcoded, Unicode input is still unimplemented, resize reconnects still skip the auto-reconnect cookie path, and failure paths still lack explicit process exit codes.
Recommendation: move initial size/scale into app bootstrap, implement Unicode input through modern `winit` events, wire proper exit codes, and complete the auto-reconnect-cookie flow for resize-driven reconnects.
Effort: small to medium.

9. Make display and bitmap limitations fail explicitly instead of degrading implicitly.
Refs: `crates/ironrdp-displaycontrol/src/client.rs`, `crates/ironrdp-server/src/encoder/bitmap.rs`, `crates/ironrdp-server/src/server.rs`, `crates/ironrdp-server/src/encoder/mod.rs`.
Problem: display-control still does not enforce negotiated monitor-area limits, and the server still has known gaps around non-multiple-of-4 bitmap widths, client-smaller-than-server behavior, and pessimistic bitmap buffer growth.
Recommendation: add capability/size validation before live-session entry, clamp or reject unsupported monitor layouts early, and add a cheap encoded-size heuristic to reduce repeated over-allocation.
Effort: small to medium.

10. Make the server connection model explicit.
Refs: `crates/ironrdp-server/src/server.rs`.
Problem: `RdpServer::run()` accepts a connection and then awaits it inline, so the listener is effectively single-connection even though that is not surfaced as a first-class contract.
Recommendation: either document single-client behavior explicitly in type docs and README, or move to task-per-connection handling and replace the handler mutex with a channel-owned worker model.
Effort: medium.

## Priority 2: Medium-size boundary cleanup

11. Unify FFI packaging logic between `xtask` and release workflows.
Refs: `xtask/src/ffi.rs`, `.github/workflows/nuget-publish.yml`, `ffi/Cargo.toml`.
Problem: `xtask` and the NuGet workflow currently implement overlapping DLL-copying and profile-selection logic, and the workflow still rewrites `ffi/Cargo.toml` at build time.
Recommendation: teach `cargo xtask ffi build` about the production packaging profile, stop mutating `Cargo.toml` in CI, and have CI/release call the same entrypoint used locally.
Effort: medium.

12. Replace the allocation-heavy managed framing path with a pooled or ring-buffer design.
Refs: `ffi/dotnet/Devolutions.IronRdp/src/Framed.cs`.
Problem: reads repeatedly call `ToArray()`, `Take()`, `Skip()`, and rebuild `List<byte>` buffers, which is avoidable churn on a now-central Windows code path.
Recommendation: move to `ArrayBufferWriter<byte>` or a dedicated ring buffer, avoid full-buffer copies in `ReadPdu`/`ReadByHint`, and normalize EOF/error mapping at the same time.
Effort: medium.

13. Simplify the FFI connector API instead of exposing consumed-state plumbing.
Refs: `ffi/src/connector/mod.rs`.
Problem: `ClientConnector` wraps `Option<...>` to model ownership transfer, still carries naming and missing-opaque-type FIXMEs, and exposes an awkward take-and-put-back shape to managed callers.
Recommendation: rename the mutating helpers to `attach_*`, add opaque channel wrappers, and reduce the number of stateful consumption patterns exported through Diplomat.
Effort: medium.

14. Move client-only rendering knobs out of `connector::Config`.
Refs: `crates/ironrdp-connector/src/lib.rs`, `crates/ironrdp-client/src/config.rs`, `ffi/src/connector/config.rs`.
Problem: fields like `enable_server_pointer` and `pointer_software_rendering` live in connector config even though the connector crate itself calls them client-only concerns.
Recommendation: move presentation/rendering knobs into a higher-level client or session config so `ironrdp-connector` stays focused on handshake and negotiated protocol state.
Effort: medium.

15. Reduce `ironrdp-session`’s coupling to `ironrdp-connector`.
Refs: `crates/ironrdp-session/Cargo.toml`, `crates/ironrdp-session/src/lib.rs`.
Problem: `ironrdp-session` still publicly depends on `ironrdp-connector`, and the crate root still questions whether some functionality belongs there at all.
Recommendation: extract the minimum shared activation/session state needed by both crates and remove the public connector dependency from session.
Effort: medium.

16. Decide which unpublished crates are intentionally private.
Refs: `crates/ironrdp-propertyset/Cargo.toml`, `crates/ironrdp-cfg/Cargo.toml`, `crates/ironrdp-mstsgu/Cargo.toml`, `crates/ironrdp-rdpfile/Cargo.toml`, `crates/ironrdp-egfx/Cargo.toml`.
Problem: several crates still read as `publish = false # TODO: publish`, which looks like unresolved packaging debt rather than an explicit policy.
Recommendation: either publish them or replace the TODOs with a clear statement that they are intentionally internal/private.
Effort: small.

## Priority 3: Strategic refactors

17. Turn broad architecture TODOs into tracked work for `ironrdp-pdu`.
Refs: `ARCHITECTURE.md`, `crates/ironrdp-pdu/Cargo.toml`, `crates/ironrdp-pdu/README.md`.
Problem: the architecture document and crate manifest still describe dependency debt in broad terms, and the README still contains an unfinished section.
Recommendation: convert the vague TODOs into scoped work items or issue links, then continue removing `byteorder`, `num-derive`, and `num-traits` from the crate.
Effort: medium to large.

18. Split `ironrdp-graphics` into smaller crates and remove legacy helpers.
Refs: `ARCHITECTURE.md`, `crates/ironrdp-graphics/Cargo.toml`, `crates/ironrdp-graphics/src/lib.rs`, `crates/ironrdp-graphics/src/rle.rs`, `crates/ironrdp-graphics/src/utils.rs`.
Problem: graphics is still explicitly called out as oversized and still carries the same legacy dependency pattern as `ironrdp-pdu`.
Recommendation: isolate codec- or format-specific code into smaller crates, push reusable cursor/buffer helpers toward `ironrdp-core` where appropriate, and remove remaining blanket lint allows once invariants are stated locally.
Effort: large.

## Suggested execution order

1. Fix `xtask ci` platform behavior and remove stale coverage/docs drift.
2. Fix FFI panic/TLS/package warning issues while the Windows/.NET surface is still narrow.
3. Fix the native-client `softbuffer` lifetime issue and finish the bootstrap/input gaps.
4. Make server/display limitations explicit and document the server connection model.
5. Unify FFI packaging and simplify the FFI connector surface.
6. Clean up `connector::Config` and reduce `ironrdp-session` coupling.
7. Treat `ironrdp-pdu` and `ironrdp-graphics` as the next major refactor track, not as quick cleanup.
