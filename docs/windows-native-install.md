# Windows-Native IronRDP Install And Use

This fork ships Windows deployment artifacts from `build.ps1 -Mode package` and
`build.ps1 -Mode publish`. A target machine does not need the source repo to run
the native client package.

The current Windows release set includes:

- a portable bundle zip
- a signed `MSIX` package
- a signed `MSI` package
- an `App Installer` descriptor when `build.ps1` is given a release repo/tag
- a signing certificate `.cer` when a self-signed test certificate is used

## Package contents

A packaged artifact root contains:

- `build-manifest.json`
- `client/ironrdp-client.exe`
- `ffi/DevolutionsIronRdp.dll` when the native FFI build is present
- `docs/windows-native-install.md`
- `tools/Install-IronRdpPackage.ps1`
- `tools/Invoke-IronRdpSmokeTest.ps1`
- `tools/Invoke-HyperVInstallerTest.ps1`
- `tools/Invoke-HyperVLiveConnectTest.ps1`
- `tools/Invoke-HyperVE2ESuite.ps1`

`build.ps1` also emits:

- a deployment zip under the machine-scoped `bundles/` directory
- installer artifacts under the sibling `installers/` directory

## Prerequisites

- Windows x64
- Open a PowerShell 7+ session when using the helper scripts

Package and publish builds now embed a static MSVC CRT for the native Windows
artifacts, so the portable bundle no longer assumes the Visual C++
Redistributable is preinstalled on the target machine.

## Install from a packaged artifact root or bundle

If you extracted or copied the packaged artifact root to a machine:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Install-IronRdpPackage.ps1
```

Default install location:

- per-user: `%LOCALAPPDATA%\Programs\IronRDP`

Useful options:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Install-IronRdpPackage.ps1 -Force
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Install-IronRdpPackage.ps1 -AllUsers -Force
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Install-IronRdpPackage.ps1 -CreateDesktopShortcut
```

The installer writes:

- `Start-IronRdpClient.ps1`
- `ironrdp-client.cmd`

into the install root for easier launching.

If you have the portable bundle zip instead of the unpacked package root:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\Install-IronRdpPackage.ps1 -BundlePath .\IronRDP-DTM-WORK-0.0.0-dev-portable.zip -Force
```

## Install from MSI

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\Install-IronRdpPackage.ps1 -InstallerPath .\IronRDP.msi
```

Default install location:

- all users: `%ProgramFiles%\IronRDP`

## Install from MSIX

If the package is signed with a self-signed test certificate, import the
matching `.cer` through the same helper:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\Install-IronRdpPackage.ps1 -InstallerPath .\IronRDP.msix -CertificatePath .\IronRDP-test-signing.cer
```

If `build.ps1` also emitted an `.appinstaller` file, install from that instead
to enable App Installer-managed upgrades:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\Install-IronRdpPackage.ps1 -InstallerPath .\IronRDP.appinstaller -CertificatePath .\IronRDP-test-signing.cer
```

## Smoke test after install

From the installed location:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Invoke-IronRdpSmokeTest.ps1 -InstallRoot $env:LOCALAPPDATA\Programs\IronRDP
```

This validates:

- `build-manifest.json` is present and parseable
- `ironrdp-client.exe --version` succeeds
- `ironrdp-client.exe --help` succeeds
- optional docs/FFI files are present when packaged
- installed MSIX layout when validating by package name

If you also provide connection credentials, the smoke helper can run a bounded
live session probe and return timing/log data instead of just launching the
client:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Invoke-IronRdpSmokeTest.ps1 `
  -InstallRoot $env:LOCALAPPDATA\Programs\IronRDP `
  -LaunchHost 10.0.0.20 `
  -Username alice `
  -Password secret `
  -ConnectSeconds 20
```

The live-connect result currently records:

- whether the client reached connection setup, image emission, and frame present
- client log path and tail
- frame counts
- copy/present timing summaries from the software render path
- reconnect and multitransport-abort counts

The current portable bundle has been validated on a clean Hyper-V
Windows Server 2025 guest using the same `Install-IronRdpPackage.ps1` and
`Invoke-IronRdpSmokeTest.ps1` flow documented here.

To launch the client without bounded validation:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Invoke-IronRdpSmokeTest.ps1 -InstallRoot $env:LOCALAPPDATA\Programs\IronRDP -LaunchHost 10.0.0.20 -Username alice
```

For an installed MSIX package:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\Invoke-IronRdpSmokeTest.ps1 -MsixPackageName DavidMartel.IronRDP
```

## Typical client usage

Direct TCP/TLS:

```pwsh
& "$env:LOCALAPPDATA\Programs\IronRDP\Start-IronRdpClient.ps1" 10.0.0.20 --username alice --password secret
```

Help and version:

```pwsh
& "$env:LOCALAPPDATA\Programs\IronRDP\Start-IronRdpClient.ps1" --help
& "$env:LOCALAPPDATA\Programs\IronRDP\Start-IronRdpClient.ps1" --version
```

You can also run `ironrdp-client.exe` directly from the `client/` directory.

## Updating or replacing an install

Portable installs can be replaced by running the helper again against a newer
package with `-Force`:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Install-IronRdpPackage.ps1 -Force
```

Installer-based updates:

- `MSIX`: use the emitted `.appinstaller` when available for automatic
  upgrade checks, or reinstall the newer `.msix`
- `MSI`: install the newer `.msi`; the package is authored for major-upgrade
  replacement

## Hyper-V guest validation

This repo also ships a guest-side MSI smoke harness for the local Windows
Server Hyper-V test VM:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Invoke-HyperVInstallerTest.ps1 -MsiPath .\IronRDP.msi
```

The current baseline validated by this fork is:

- MSI install inside `WS2025-ReFS-Repair`
- `ironrdp-client --version`
- `ironrdp-client --help`
- `TermService`, `WinRM`, and `sshd` running
- guest IP discovery
- RDP port `3389` reachable from the host

There is now also a host-to-guest live session harness that keeps the guest
running and uses the packaged client to connect back into the VM:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Invoke-HyperVLiveConnectTest.ps1 -PackageRoot . -ConnectSeconds 25
```

The current observed Hyper-V baseline is:

- the running guest is reachable from the host over the Hyper-V Default Switch
  address (`172.23.x.x` in the current lab)
- the `dtm-net-switch` guest address (`10.10.20.250` in the current lab) is
  not yet host-reachable for RDP on this machine
- the native client reaches `session-rendering` status reliably against the VM
- the guest is currently sending software-compressed bitmap updates, including
  many `16`-bpp RLE bitmap streams, not EGFX/H.264
- advertising multitransport with `prefer-reliable` did not trigger a server
  UDP sideband request on this path
- the remaining dominant client-side render cost is the `softbuffer` backend
  conversion step, which currently measures around `~0.95-1.1 ms` per presented
  frame on this workstation

For richer diagnostics, the packaged toolset now includes a repeatable
Hyper-V e2e suite:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Invoke-HyperVE2ESuite.ps1 -PackageRoot . -ScenarioSet quick
```

Or through the repo build entrypoint:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\build.ps1 -Mode hyperv-suite -HyperVScenarioSet quick
```

The suite currently drives:

- a bounded baseline session
- resize and input automation against the live client window
- host clipboard mutation and CLIPRDR log capture
- explicit capability reporting for clipboard, audio wiring, and unsupported device redirection
- explicit per-scenario health summaries with failures, warnings, staged clipboard/audio observations, and workload-stage diagnosis
- optional temporary host-side outage simulation by blocking outbound `3389`
- per-scenario screenshots, client logs, CPU samples, and JSON summaries

The current measured Hyper-V e2e baseline is:

- connection establishment is about `~130 ms`
- first image and first frame are about `~700 ms`
- the active guest path is still dominated by `Rdp61` plus `16`-bpp RLE bitmap updates
- the resize/reactivation path now completes cleanly without the earlier FastPath decompressor failure
- the native client no longer overwrites queued unpresented frames in the current resize workload after the pacing/coalescing pass
- resize scenarios now show client-handled clipboard activity, but guest-side text verification is still not proven end to end
- suite summaries now report that the guest workload currently falls back to a non-interactive process in session `0` because scheduled interactive task registration is rejected on this VM account model
- the audio path can now reach `playback-observed` in live suite runs, but the suite still needs a deliberate guest-side sound workload before playback assertions are deterministic
- device redirection remains explicitly unsupported on this branch because the client still uses `NoopRdpdrBackend`
- backend-local `softbuffer` conversion and present time are still the main client-side render bottlenecks

## Uninstall

Portable uninstall:

```pwsh
Remove-Item "$env:LOCALAPPDATA\Programs\IronRDP" -Recurse -Force
```

MSI uninstall:

```pwsh
msiexec /x IronRDP.msi /qn
```

MSIX uninstall:

```pwsh
Remove-AppxPackage -Package (Get-AppxPackage -Name DavidMartel.IronRDP).PackageFullName
```
