[CmdletBinding(DefaultParameterSetName = 'Package')]
param(
    [Parameter(ParameterSetName = 'Package')]
    [string]$PackageRoot = (Split-Path -Parent $PSScriptRoot),

    [Parameter(ParameterSetName = 'Install', Mandatory = $true)]
    [string]$InstallRoot,

    [string]$LaunchHost,
    [string]$Username
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($PSCmdlet.ParameterSetName -eq 'Install') {
    $root = (Resolve-Path -LiteralPath $InstallRoot).Path
} else {
    $root = (Resolve-Path -LiteralPath $PackageRoot).Path
}

$manifestPath = Join-Path $root 'build-manifest.json'
$clientExe = Join-Path $root 'client\ironrdp-client.exe'
$ffiDll = Join-Path $root 'ffi\DevolutionsIronRdp.dll'
$installGuide = Join-Path $root 'docs\windows-native-install.md'
$launcherPs1 = Join-Path $root 'Start-IronRdpClient.ps1'
$launcherCmd = Join-Path $root 'ironrdp-client.cmd'

foreach ($required in @($manifestPath, $clientExe)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "required file not found: $required"
    }
}

$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$versionText = (& $clientExe --version 2>&1 | Out-String).Trim()
if ($LASTEXITCODE -ne 0) {
    throw "ironrdp-client --version failed with exit code $LASTEXITCODE"
}

$helpText = (& $clientExe --help 2>&1 | Out-String)
if ($LASTEXITCODE -ne 0) {
    throw "ironrdp-client --help failed with exit code $LASTEXITCODE"
}

$result = [pscustomobject]@{
    root = $root
    clientPath = $clientExe
    version = $versionText
    deploymentName = $manifest.deploymentName
    buildClass = $manifest.build.class
    ffiPresent = Test-Path -LiteralPath $ffiDll -PathType Leaf
    docsPresent = Test-Path -LiteralPath $installGuide -PathType Leaf
    launcherPs1Present = if ($PSCmdlet.ParameterSetName -eq 'Install') { Test-Path -LiteralPath $launcherPs1 -PathType Leaf } else { $null }
    launcherCmdPresent = if ($PSCmdlet.ParameterSetName -eq 'Install') { Test-Path -LiteralPath $launcherCmd -PathType Leaf } else { $null }
}

$result | Format-List | Out-String | Write-Host

if ($LaunchHost) {
    $arguments = @($LaunchHost)
    if ($Username) {
        $arguments += @('--username', $Username)
    }

    Write-Host "Launching IronRDP client against $LaunchHost" -ForegroundColor Yellow
    Start-Process -FilePath $clientExe -ArgumentList $arguments -WorkingDirectory (Split-Path -Parent $clientExe)
}
