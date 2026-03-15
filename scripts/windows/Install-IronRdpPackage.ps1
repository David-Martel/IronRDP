[CmdletBinding()]
param(
    [string]$PackageRoot = (Split-Path -Parent $PSScriptRoot),
    [string]$InstallRoot,
    [switch]$AllUsers,
    [switch]$Force,
    [switch]$CreateDesktopShortcut
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Copy-DirectoryContentsIfPresent {
    param(
        [Parameter(Mandatory)][string]$SourcePath,
        [Parameter(Mandatory)][string]$DestinationPath
    )

    if (-not (Test-Path -LiteralPath $SourcePath -PathType Container)) {
        return
    }

    New-Item -ItemType Directory -Force -Path $DestinationPath | Out-Null
    foreach ($item in (Get-ChildItem -LiteralPath $SourcePath -Force)) {
        Copy-Item -LiteralPath $item.FullName -Destination $DestinationPath -Recurse -Force
    }
}

function New-DesktopShortcut {
    param(
        [Parameter(Mandatory)][string]$TargetPath,
        [Parameter(Mandatory)][string]$ShortcutPath
    )

    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($ShortcutPath)
    $shortcut.TargetPath = $TargetPath
    $shortcut.WorkingDirectory = (Split-Path -Parent $TargetPath)
    $shortcut.IconLocation = $TargetPath
    $shortcut.Save()
}

$PackageRoot = (Resolve-Path -LiteralPath $PackageRoot).Path
$manifestPath = Join-Path $PackageRoot 'build-manifest.json'
$clientDir = Join-Path $PackageRoot 'client'
$clientExe = Join-Path $clientDir 'ironrdp-client.exe'

if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "package manifest not found: $manifestPath"
}

if (-not (Test-Path -LiteralPath $clientExe -PathType Leaf)) {
    throw "client executable not found: $clientExe"
}

if (-not $InstallRoot) {
    $InstallRoot = if ($AllUsers) {
        Join-Path ${env:ProgramFiles} 'IronRDP'
    } else {
        Join-Path $env:LOCALAPPDATA 'Programs\IronRDP'
    }
}

if ((Test-Path -LiteralPath $InstallRoot) -and -not $Force) {
    throw "install root already exists: $InstallRoot (use -Force to replace it)"
}

if ((Test-Path -LiteralPath $InstallRoot) -and $Force) {
    Remove-Item -LiteralPath $InstallRoot -Recurse -Force
}

New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
Get-ChildItem -LiteralPath $PackageRoot -Recurse -File | Unblock-File -ErrorAction SilentlyContinue

Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $InstallRoot 'build-manifest.json') -Force
Copy-DirectoryContentsIfPresent -SourcePath $clientDir -DestinationPath (Join-Path $InstallRoot 'client')
Copy-DirectoryContentsIfPresent -SourcePath (Join-Path $PackageRoot 'ffi') -DestinationPath (Join-Path $InstallRoot 'ffi')
Copy-DirectoryContentsIfPresent -SourcePath (Join-Path $PackageRoot 'docs') -DestinationPath (Join-Path $InstallRoot 'docs')
Copy-DirectoryContentsIfPresent -SourcePath (Join-Path $PackageRoot 'tools') -DestinationPath (Join-Path $InstallRoot 'tools')

$launcherPs1 = Join-Path $InstallRoot 'Start-IronRdpClient.ps1'
$launcherCmd = Join-Path $InstallRoot 'ironrdp-client.cmd'
$installedClientExe = Join-Path $InstallRoot 'client\ironrdp-client.exe'

$launcherScript = @'
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments
)

$clientExe = Join-Path $PSScriptRoot 'client\ironrdp-client.exe'
& $clientExe @Arguments
exit $LASTEXITCODE
'@
Set-Content -LiteralPath $launcherPs1 -Value $launcherScript -Encoding UTF8

$cmdScript = "@echo off`r`nset SCRIPT_DIR=%~dp0`r`npwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File `"%SCRIPT_DIR%Start-IronRdpClient.ps1`" %*`r`n"
Set-Content -LiteralPath $launcherCmd -Value $cmdScript -Encoding ASCII

if ($CreateDesktopShortcut) {
    $desktopShortcut = Join-Path ([Environment]::GetFolderPath('Desktop')) 'IronRDP Client.lnk'
    New-DesktopShortcut -TargetPath $installedClientExe -ShortcutPath $desktopShortcut
}

$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$buildClass = $manifest.build.class
$version = $manifest.version.SemVer

Write-Host "Installed IronRDP $version ($buildClass) to $InstallRoot" -ForegroundColor Green
Write-Host "Client executable: $installedClientExe" -ForegroundColor Cyan
Write-Host "Launcher script: $launcherPs1" -ForegroundColor Cyan
Write-Host "Launcher cmd: $launcherCmd" -ForegroundColor Cyan
Write-Host ''
Write-Host 'Usage examples:' -ForegroundColor Yellow
Write-Host "  & '$launcherPs1' --help"
Write-Host "  & '$launcherPs1' --version"
Write-Host "  & '$launcherPs1' 10.0.0.20 --username alice --password secret"
