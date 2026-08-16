# IronRDP

[![](https://docs.rs/ironrdp/badge.svg)](https://docs.rs/ironrdp/) [![](https://img.shields.io/crates/v/ironrdp)](https://crates.io/crates/ironrdp)

A collection of Rust crates providing an implementation of the Microsoft Remote Desktop Protocol, with a focus on security.

## Demonstration

<https://user-images.githubusercontent.com/3809077/202049929-76f42471-aeb0-41da-9118-0dc6ea491bd2.mp4>

## Video Codec Support

Supported codecs:

- Uncompressed raw bitmap
- Interleaved Run-Length Encoding (RLE) Bitmap Codec
- RDP 6.0 Bitmap Compression
- Microsoft RemoteFX (RFX)
- H.264 / AVC420 over the Graphics Pipeline (EGFX), and opt-in AVC444/AVC444v2
  4:4:4 dual-stream decode, via bundled OpenH264

> On this fork the `ironrdp-client` binary is built with the `openh264` feature
> **on by default**, so a plain `cargo build --release -p ironrdp-client` can
> render EGFX/H.264 servers (e.g. gnome-remote-desktop) out of the box. This
> pulls in a from-source build of Cisco's OpenH264 — see
> [H.264 / OpenH264 build prerequisites](#h264--openh264-build-prerequisites)
> below for the toolchain requirement (nasm + a C/C++ compiler) and the H.264
> patent/licensing consideration for redistribution.

## Examples

### [`ironrdp-client`](https://github.com/Devolutions/IronRDP/tree/master/crates/ironrdp-client)

A full-fledged RDP client based on IronRDP crates suite, and implemented using non-blocking, asynchronous I/O.

```shell
cargo run --bin ironrdp-client -- <HOSTNAME> --username <USERNAME>
```

The client prompts for the password without echoing it.
Unattended callers should use `--password-stdin` with a private pipe; do not put passwords in shell history or process arguments.

## Windows Build And Deployment

This fork carries a Windows-focused build entrypoint at [`build.ps1`](./build.ps1).
It is intended to run with the local `CargoTools` PowerShell module, and it
opportunistically consumes `ProfileUtilities` and `MachineConfiguration` when
they are available on the workstation.

### H.264 / OpenH264 build prerequisites

Because the `ironrdp-client` binary is built with the `openh264` feature on by
default (see [Video Codec Support](#video-codec-support)), the default build
compiles **Cisco's OpenH264 from source** (`openh264` → `ironrdp-egfx/openh264-bundled`
→ `openh264/source`). That adds two build-time toolchain requirements beyond the
Rust toolchain:

- **A C/C++ compiler** — hard requirement. On Windows this is the MSVC toolset
  (`cl.exe`) that ships with Visual Studio / the C++ Build Tools; `build.ps1`
  already treats MSVC as a required dependency (`-Mode doctor` reports it).
- **`nasm`** — required for OpenH264's SIMD assembly. This is a *soft* requirement:
  `openh264-sys2` silently falls back to a slower pure-C decoder when `nasm` is
  missing, so a release build can succeed while quietly shipping the unoptimized
  H.264 path. `build.ps1` therefore checks for `nasm` before building the client
  and **errors** for release/artifact modes (`package`, `publish`, `deploy`,
  `deploy-suite`) if it is absent, and warns for `client`/`all`. Install it with
  `-BootstrapTools` (which runs `choco install nasm` and refreshes the process
  PATH so the just-installed assembler is visible to the child `cargo`), or
  `choco install nasm` manually, or run `-Mode doctor` to see its status.

**Building without H.264 (patent-clean / no native toolchain).** Cisco's
royalty-free H.264 patent grant only covers Cisco's own *binary* OpenH264
distribution; a from-source build falls outside that umbrella, so shipping the
default build assumes the distributor either holds an H.264/AVC patent license or
accepts that risk. This is a **conscious distribution decision** for this fork's
release artifacts. To produce a patent-clean, nasm-free portable client (classic
bitmap / RemoteFX only, no H.264), pass `-NoH264`:

```pwsh
pwsh -NoLogo -NoProfile -File .\build.ps1 -Mode package -Release -NoH264
```

`-NoH264` maps to `cargo build --no-default-features --features rustls` for the
client; the equivalent raw cargo invocation is
`cargo build --release -p ironrdp-client --no-default-features --features rustls`.
As a middle ground, set `OPENH264_NO_ASM=1` to deliberately build OpenH264
without SIMD assembly (keeps H.264, drops the nasm requirement, slower decode).

Typical local flows:

```pwsh
pwsh -NoLogo -NoProfile -File .\build.ps1 -Mode doctor
pwsh -NoLogo -NoProfile -File .\build.ps1 -Mode test -UseNextest
pwsh -NoLogo -NoProfile -File .\build.ps1 -Mode package -Release
pwsh -NoLogo -NoProfile -File .\build.ps1 -Mode publish -Release -TargetMachine dtm-p1gen7
```

For a machine that does not have the repo checked out, package mode now emits:

- a portable artifact root
- a portable deployment zip
- a signed MSIX package
- a signed MSI package
- an App Installer descriptor when `build.ps1` is given release repo/tag metadata

The operator-facing install and smoke-test flow is documented in
[docs/windows-native-install.md](./docs/windows-native-install.md) and shipped
inside the package under `docs/` and `tools/`.
Package and publish modes also embed a static MSVC CRT for the native Windows
artifacts so the portable bundle and installer payloads do not depend on a
separately installed Visual C++ Redistributable on a clean target machine.

The script uses CargoTools machine settings for build job count, `sccache`,
`CARGO_TARGET_DIR`, linker acceleration, and artifact publishing. Package and
publish modes also emit a machine-scoped `build-manifest.json` alongside the
generated artifacts so deployment decisions can be reproduced across machines.
When newer Visual Studio toolchains are installed, `build.ps1` prefers the
latest compatible MSVC toolset automatically, including Preview or Insiders
channels when they are present. LLVM/lld, Intel oneAPI, and CUDA are treated as
optional overlays and are recorded in the emitted manifest rather than becoming
hard dependencies for normal builds. Portable release artifacts remain distinct
from host-tuned `-NativeCpu` builds.

CargoTools currently routes wrapped builds through `rustup run stable cargo`, so
each Windows machine should keep the `stable` toolchain updated to the repo's
pinned Rust version before relying on `build.ps1`:

```pwsh
rustup update stable
```

On the Windows-native branch, the current client path remains CPU/software-first:
`winit` drives input and window lifecycle, `softbuffer` presents the decoded
desktop, IME commit events are translated into Unicode fast-path input, and the
packed presentation buffer is now reused across frames to reduce render-path
heap churn before deeper GPU work is considered.

The current deployment/test loop for Windows operators is:

```pwsh
pwsh -NoLogo -NoProfile -File .\build.ps1 -Mode package -Release -SkipDotNet
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Install-IronRdpPackage.ps1 -BundlePath .\artifacts\IronRDP-DTM-WORK-0.0.0-dev-portable.zip -Force
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Invoke-IronRdpSmokeTest.ps1 -InstallRoot $env:LOCALAPPDATA\Programs\IronRDP
```

Installer-backed validation now also exists:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Install-IronRdpPackage.ps1 -InstallerPath .\IronRDP.msix -CertificatePath .\IronRDP-test-signing.cer
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Invoke-IronRdpSmokeTest.ps1 -MsixPackageName DavidMartel.IronRDP
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Invoke-HyperVInstallerTest.ps1 -MsiPath .\IronRDP.msi
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Invoke-HyperVLiveConnectTest.ps1 -PackageRoot T:\RustCache\artifacts\IronRDP\windows-server-only\DTM-WORK -ConnectSeconds 25
```

The current validation baseline is:

- portable bundle install and smoke on the host
- MSIX install and smoke on the host
- MSI install inside the Hyper-V Windows Server 2025 guest
- guest-side `ironrdp-client --version` and `--help`
- guest `TermService` availability and host-visible port `3389`
- a bounded live IronRDP client session from the host into the running Hyper-V guest

The current observed Hyper-V live-connect profile is:

- the host reaches the guest reliably over the Hyper-V Default Switch IPv4 path
- the alternate `dtm-net-switch` guest address is not currently host-reachable for RDP
- the session renders successfully through the packaged client with bounded shutdown
- Windows Server 2025 is currently negotiating software bitmap updates on this path, including frequent `16`-bpp RLE bitmaps
- advertising experimental multitransport did not trigger a server-side UDP request in this environment
- the dominant client-side present cost is still the `softbuffer` conversion step rather than the session-driver frame copy

The packaged toolset also now includes a richer Hyper-V e2e suite:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\build.ps1 -Mode hyperv-suite -HyperVScenarioSet quick
```

That suite captures:

- connection-established, first-image, and first-frame timing
- frame cadence and image cadence summaries
- software present timing, surface-acquire timing, and redraw-pressure counters
- compression mix and bitmap characteristics
- bounded resize, mouse-input, and clipboard-mutation scenarios against the running guest
- guest-side control over WinRM instead of only Hyper-V-local remoting
- explicit capability reporting for clipboard, audio wiring, and currently unsupported device redirection
- per-scenario health summaries with failures, warnings, staged clipboard/audio observations, workload-stage diagnosis, and workload launch mode reporting

Current measured Hyper-V e2e findings on this branch:

- connection establishment is about `~130 ms`
- first image and first frame are about `~700 ms`
- the guest still prefers `Rdp61`/bitmap traffic over EGFX/H.264 on this path
- the resize/reactivation path now completes cleanly without the earlier FastPath decompressor failure
- the native client no longer overwrites queued frames in the current resize workload after the pacing/coalescing pass
- resize scenarios now show client-handled clipboard activity, but guest-side text verification is still not proven end to end
- the guest-control path now enables and verifies WinRM, then reuses stored Credential Manager entries for `WSMAN/` and `TERMSRV/` access
- the default guest workload is now a direct WinRM-backed file write, so baseline and resize scenarios no longer depend on Notepad or a fragile session-`0` fallback just to prove guest-side activity
- suite summaries now call out workload-stage quality and launch mode explicitly; the current validated baseline is `remote-file-write`, while a fully interactive in-session workload is still a follow-up item
- the suite now drives a deliberate guest-side audio pulse and can reach `playback-observed` in live runs, but a stronger interactive workload is still needed for deterministic app-driven audio assertions
- backend-local `softbuffer` conversion and present time are still the main client-side render bottlenecks, and the suite now separates surface acquisition cost from conversion/present cost when diagnosing that path

### [`screenshot`](https://github.com/Devolutions/IronRDP/blob/master/crates/ironrdp/examples/screenshot.rs)

Example of utilizing IronRDP in a blocking, synchronous fashion.

This example showcases the use of IronRDP in a blocking manner. It
demonstrates how to create a basic RDP client with just a few hundred lines
of code by leveraging the IronRDP crates suite.

In this basic client implementation, the client establishes a connection
with the destination server, decodes incoming graphics updates, and saves the
resulting output as a PNG image file on the disk.

```shell
printf '%s\n' "$RDP_PASSWORD" | cargo run --example=screenshot -- --host <HOSTNAME> --username <USERNAME> --password-stdin --output out.png
```

### How to enable RemoteFX on server

Run the following PowerShell commands, and reboot.

```pwsh
Set-ItemProperty -Path 'HKLM:\Software\Policies\Microsoft\Windows NT\Terminal Services' -Name 'ColorDepth' -Type DWORD -Value 5
Set-ItemProperty -Path 'HKLM:\Software\Policies\Microsoft\Windows NT\Terminal Services' -Name 'fEnableVirtualizedGraphics' -Type DWORD -Value 1
```

Alternatively, you may change a few group policies using `gpedit.msc`:

1. Run `gpedit.msc`.

2. Enable `Computer Configuration/Administrative Templates/Windows Components/Remote Desktop Services/Remote Desktop Session Host/Remote Session Environment/RemoteFX for Windows Server 2008 R2/Configure RemoteFX`

3. Enable `Computer Configuration/Administrative Templates/Windows Components/Remote Desktop Services/Remote Desktop Session Host/Remote Session Environment/Enable RemoteFX encoding for RemoteFX clients designed for Windows Server 2008 R2 SP1`

4. Enable `Computer Configuration/Administrative Templates/Windows Components/Remote Desktop Services/Remote Desktop Session Host/Remote Session Environment/Limit maximum color depth`

5. Reboot.

## Rust version (MSRV)

IronRDP libraries follow a conservative Minimum Supported Rust Version (MSRV) policy.
The MSRV is the oldest stable Rust release that is at least 6 months old, bounded by the Rust version available in [Debian stable-backports](https://packages.debian.org/search?suite=all&arch=any&searchon=names&keywords=rust) and [Fedora stable](https://packages.fedoraproject.org/pkgs/rust/rust/).
The pinned toolchain in `rust-toolchain.toml` is both the project toolchain and the MSRV validated by CI.
See [ARCHITECTURE.md](./ARCHITECTURE.md#msrv-policy) for the full policy.

## Architecture

See the [ARCHITECTURE.md](https://github.com/Devolutions/IronRDP/blob/master/ARCHITECTURE.md) document.

## Getting help

- Report bugs in the [issue tracker](https://github.com/Devolutions/IronRDP/issues)
- Discuss the project on the [matrix room](https://matrix.to/#/#IronRDP:matrix.org)
