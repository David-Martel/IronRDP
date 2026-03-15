# Windows-Native IronRDP Install And Use

This fork ships Windows deployment artifacts from `build.ps1 -Mode package` and
`build.ps1 -Mode publish`. A target machine does not need the source repo to run
the native client package.

The current shipping format is a portable bundle. It is not an MSI or MSIX
installer yet. The bundle contains the client executable, optional FFI native
DLL, operator docs, and helper scripts for install and smoke validation.

## Package contents

A packaged artifact root contains:

- `build-manifest.json`
- `client/ironrdp-client.exe`
- `ffi/DevolutionsIronRdp.dll` when the native FFI build is present
- `docs/windows-native-install.md`
- `tools/Install-IronRdpPackage.ps1`
- `tools/Invoke-IronRdpSmokeTest.ps1`

`build.ps1` also emits a deployment zip alongside the artifact root under the
machine-scoped `bundles/` directory.

## Prerequisites

- Windows x64
- Open a PowerShell 7+ session when using the helper scripts
- If the client fails to start with a missing Visual C++ runtime error, install
  the current Microsoft Visual C++ Redistributable for Visual Studio

## Install from a packaged artifact root

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

To launch the client as part of the smoke step:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Invoke-IronRdpSmokeTest.ps1 -InstallRoot $env:LOCALAPPDATA\Programs\IronRDP -LaunchHost 10.0.0.20 -Username alice
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

Run the installer again against a newer package with `-Force`:

```pwsh
pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Install-IronRdpPackage.ps1 -Force
```

## Uninstall

This package currently uses a portable copy-style install. Uninstall by deleting
the install root, for example:

```pwsh
Remove-Item "$env:LOCALAPPDATA\Programs\IronRDP" -Recurse -Force
```
