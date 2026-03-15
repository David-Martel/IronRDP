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

On the Windows-native branch, the current client path remains CPU/software-first:
`winit` drives input and window lifecycle, `softbuffer` presents the decoded
desktop, IME commit events are translated into Unicode fast-path input, and the
packed presentation buffer is now reused across frames to reduce render-path
heap churn before deeper GPU work is considered.

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
