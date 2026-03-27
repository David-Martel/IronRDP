[CmdletBinding(DefaultParameterSetName = 'Package')]
param(
    [Parameter(ParameterSetName = 'Package')]
    [string]$PackageRoot = (Split-Path -Parent $PSScriptRoot),

    [Parameter(ParameterSetName = 'Bundle', Mandatory = $true)]
    [string]$BundlePath,

    [Parameter(ParameterSetName = 'Installer', Mandatory = $true)]
    [string]$InstallerPath,

    [string]$InstallRoot,
    [switch]$AllUsers,
    [switch]$Force,
    [switch]$CreateDesktopShortcut,
    [string]$CertificatePath
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

function Get-ResolvedPackageRoot {
    switch ($PSCmdlet.ParameterSetName) {
        'Package' {
            return (Resolve-Path -LiteralPath $PackageRoot).Path
        }
        'Bundle' {
            $resolvedBundlePath = (Resolve-Path -LiteralPath $BundlePath).Path
            $extractRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("IronRDP-bundle-" + [Guid]::NewGuid().ToString('N'))
            New-Item -ItemType Directory -Force -Path $extractRoot | Out-Null
            Expand-Archive -LiteralPath $resolvedBundlePath -DestinationPath $extractRoot -Force
            return $extractRoot
        }
        default {
            return $null
        }
    }
}

function Install-PackageRoot {
    param([Parameter(Mandatory)][string]$ResolvedPackageRoot)

    $manifestPath = Join-Path $ResolvedPackageRoot 'build-manifest.json'
    $clientDir = Join-Path $ResolvedPackageRoot 'client'
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
    Get-ChildItem -LiteralPath $ResolvedPackageRoot -Recurse -File | Unblock-File -ErrorAction SilentlyContinue

    Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $InstallRoot 'build-manifest.json') -Force
    Copy-DirectoryContentsIfPresent -SourcePath $clientDir -DestinationPath (Join-Path $InstallRoot 'client')
    Copy-DirectoryContentsIfPresent -SourcePath (Join-Path $ResolvedPackageRoot 'ffi') -DestinationPath (Join-Path $InstallRoot 'ffi')
    Copy-DirectoryContentsIfPresent -SourcePath (Join-Path $ResolvedPackageRoot 'docs') -DestinationPath (Join-Path $InstallRoot 'docs')
    Copy-DirectoryContentsIfPresent -SourcePath (Join-Path $ResolvedPackageRoot 'tools') -DestinationPath (Join-Path $InstallRoot 'tools')

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
    Write-Host "Installed IronRDP $($manifest.version.SemVer) ($($manifest.build.class)) to $InstallRoot" -ForegroundColor Green
    Write-Host "Client executable: $installedClientExe" -ForegroundColor Cyan
    Write-Host "Launcher script: $launcherPs1" -ForegroundColor Cyan
    Write-Host "Launcher cmd: $launcherCmd" -ForegroundColor Cyan
    return
}

function Install-AppxCertificateIfRequested {
    param([string]$ResolvedCertificatePath)

    if ([string]::IsNullOrWhiteSpace($ResolvedCertificatePath)) {
        return
    }

    Import-Certificate -FilePath $ResolvedCertificatePath -CertStoreLocation 'Cert:\CurrentUser\TrustedPeople' | Out-Null
    Import-Certificate -FilePath $ResolvedCertificatePath -CertStoreLocation 'Cert:\CurrentUser\Root' | Out-Null

    foreach ($store in @('Cert:\LocalMachine\TrustedPeople', 'Cert:\LocalMachine\Root')) {
        try {
            Import-Certificate -FilePath $ResolvedCertificatePath -CertStoreLocation $store | Out-Null
        } catch {
            Write-Verbose "Skipping machine certificate import for ${store}: $($_.Exception.Message)"
        }
    }
}

function Install-MsixPackage {
    param([Parameter(Mandatory)][string]$ResolvedInstallerPath)

    Install-AppxCertificateIfRequested -ResolvedCertificatePath $(if ($CertificatePath) { (Resolve-Path -LiteralPath $CertificatePath).Path } else { $null })

    if ($ResolvedInstallerPath.EndsWith('.appinstaller', [System.StringComparison]::OrdinalIgnoreCase)) {
        Add-AppxPackage -AppInstallerFile $ResolvedInstallerPath -ForceApplicationShutdown
    } else {
        Add-AppxPackage -Path $ResolvedInstallerPath -ForceApplicationShutdown
    }

    $packageName = 'DavidMartel.IronRDP'
    $package = Get-AppxPackage -Name $packageName | Sort-Object Version -Descending | Select-Object -First 1
    if (-not $package) {
        throw "installed MSIX package not found: $packageName"
    }

    Write-Host "Installed IronRDP MSIX package $($package.Name) $($package.Version)" -ForegroundColor Green
    Write-Host "Install location: $($package.InstallLocation)" -ForegroundColor Cyan
}

function Install-MsiPackage {
    param([Parameter(Mandatory)][string]$ResolvedInstallerPath)

    $arguments = @('/i', "`"$ResolvedInstallerPath`"", '/qn', '/norestart')
    if ($Force) {
        $arguments += 'REINSTALL=ALL'
        $arguments += 'REINSTALLMODE=amus'
    }

    $process = Start-Process -FilePath 'msiexec.exe' -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "msiexec install failed with exit code $($process.ExitCode)"
    }

    Write-Host 'Installed IronRDP MSI package' -ForegroundColor Green
    Write-Host "Default install location: $(Join-Path ${env:ProgramFiles} 'IronRDP')" -ForegroundColor Cyan
}

switch ($PSCmdlet.ParameterSetName) {
    'Package' { Install-PackageRoot -ResolvedPackageRoot (Get-ResolvedPackageRoot) }
    'Bundle' { Install-PackageRoot -ResolvedPackageRoot (Get-ResolvedPackageRoot) }
    'Installer' {
        $resolvedInstallerPath = (Resolve-Path -LiteralPath $InstallerPath).Path
        if ($resolvedInstallerPath.EndsWith('.msix', [System.StringComparison]::OrdinalIgnoreCase) -or
            $resolvedInstallerPath.EndsWith('.appinstaller', [System.StringComparison]::OrdinalIgnoreCase)) {
            Install-MsixPackage -ResolvedInstallerPath $resolvedInstallerPath
        } elseif ($resolvedInstallerPath.EndsWith('.msi', [System.StringComparison]::OrdinalIgnoreCase)) {
            Install-MsiPackage -ResolvedInstallerPath $resolvedInstallerPath
        } else {
            throw "unsupported installer type: $resolvedInstallerPath"
        }
    }
}
