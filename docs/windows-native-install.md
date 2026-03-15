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

The current portable bundle has been validated on a clean Hyper-V
Windows Server 2025 guest using the same `Install-IronRdpPackage.ps1` and
`Invoke-IronRdpSmokeTest.ps1` flow documented here.

To launch the client as part of the smoke step:

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
