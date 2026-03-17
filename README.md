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

## Examples

### [`ironrdp-client`](https://github.com/Devolutions/IronRDP/tree/master/crates/ironrdp-client)

A full-fledged RDP client based on IronRDP crates suite, and implemented using non-blocking, asynchronous I/O.

```shell
cargo run --bin ironrdp-client -- <HOSTNAME> --username <USERNAME> --password <PASSWORD>
```

## Windows Build And Deployment

This fork carries a Windows-focused build entrypoint at [`build.ps1`](./build.ps1).
It is intended to run with the local `CargoTools` PowerShell module, and it
opportunistically consumes `ProfileUtilities` and `MachineConfiguration` when
they are available on the workstation.

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
- software present timing and overwritten-frame counts
- compression mix and bitmap characteristics
- bounded resize, mouse-input, and clipboard-mutation scenarios against the running guest
- explicit capability reporting for clipboard, audio wiring, and currently unsupported device redirection
- per-scenario health summaries with failures, warnings, staged clipboard/audio observations, and workload-stage diagnosis

Current measured Hyper-V e2e findings on this branch:

- connection establishment is about `~130 ms`
- first image and first frame are about `~700 ms`
- the guest still prefers `Rdp61`/bitmap traffic over EGFX/H.264 on this path
- the resize/reactivation path now completes cleanly without the earlier FastPath decompressor failure
- the native client no longer overwrites queued frames in the current resize workload after the pacing/coalescing pass
- resize scenarios now show client-handled clipboard activity, but guest-side text verification is still not proven end to end
- suite summaries now call out workload-stage quality explicitly; the current guest workload still falls back to session `0`
- the audio path can now reach `playback-observed` in live suite runs, but a deliberate guest-side sound workload is still needed for deterministic assertions
- backend-local `softbuffer` conversion and present time are still the main client-side render bottlenecks

### [`screenshot`](https://github.com/Devolutions/IronRDP/blob/master/crates/ironrdp/examples/screenshot.rs)

Example of utilizing IronRDP in a blocking, synchronous fashion.

This example showcases the use of IronRDP in a blocking manner. It
demonstrates how to create a basic RDP client with just a few hundred lines
of code by leveraging the IronRDP crates suite.

In this basic client implementation, the client establishes a connection
with the destination server, decodes incoming graphics updates, and saves the
resulting output as a BMP image file on the disk.

```shell
cargo run --example=screenshot -- --host <HOSTNAME> --username <USERNAME> --password <PASSWORD> --output out.bmp
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
