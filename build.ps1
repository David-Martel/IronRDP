#requires -Version 7.0

[CmdletBinding()]
param(
    [ValidateSet('check', 'client', 'ffi', 'test', 'coverage', 'fmt', 'lints', 'all')]
    [string]$Mode = 'all',

    [switch]$Release,
    [switch]$BootstrapTools,
    [switch]$UseNextest,
    [switch]$Timings,
    [switch]$NativeCpu,
    [switch]$NoSccache,
    [switch]$SkipDotNet,
    [int]$Jobs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$DotNetProject = Join-Path $RepoRoot 'ffi\dotnet\Devolutions.IronRdp\Devolutions.IronRdp.csproj'
$FfiOutputDir = Join-Path $RepoRoot 'dependencies\runtimes\win-x64\native'
$FfiOutputDll = Join-Path $FfiOutputDir 'DevolutionsIronRdp.dll'
$FfiBuildProfile = 'production-ffi'
$ClientBuildProfile = 'production'
$script:UseCargoWrapper = $true

function Add-RustFlag {
    param([Parameter(Mandatory)][string[]]$Flags)

    $existingFlags = @()
    if ($env:RUSTFLAGS) {
        $existingFlags = @($env:RUSTFLAGS -split '\s+')
    }

    foreach ($flag in $Flags) {
        if ($existingFlags -notcontains $flag) {
            $existingFlags += $flag
        }
    }

    $env:RUSTFLAGS = ($existingFlags | Where-Object { $_ }) -join ' '
}

function Test-Tool {
    param([Parameter(Mandatory)][string]$Name)

    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Install-RustTool {
    param(
        [Parameter(Mandatory)][string]$ExecutableName,
        [Parameter(Mandatory)][string]$PackageName
    )

    if (Test-Tool $ExecutableName) {
        return
    }

    Write-Host "Installing Rust tool $PackageName" -ForegroundColor Cyan

    if (Test-Tool 'cargo-binstall') {
        & cargo binstall -y $PackageName
    } else {
        & cargo install --locked $PackageName
    }

    if ($LASTEXITCODE -ne 0) {
        throw "failed to install $PackageName"
    }
}

function Install-ChocoTool {
    param(
        [Parameter(Mandatory)][string]$ExecutableName,
        [Parameter(Mandatory)][string]$PackageName
    )

    if (Test-Tool $ExecutableName) {
        return
    }

    if (-not (Test-Tool 'choco')) {
        throw "Chocolatey is required to install $PackageName automatically"
    }

    Write-Host "Installing native tool $PackageName" -ForegroundColor Cyan
    & choco install $PackageName -y --no-progress

    if ($LASTEXITCODE -ne 0) {
        throw "failed to install $PackageName"
    }
}

function Invoke-RepoCargo {
    param(
        [Parameter(Mandatory)][string[]]$ArgumentList
    )

    if ($script:UseCargoWrapper) {
        $exitCode = Invoke-CargoWrapper -ArgumentList $ArgumentList -WorkingDirectory $RepoRoot
        if ($exitCode -ne 0) {
            throw "cargo $($ArgumentList -join ' ') failed with exit code $exitCode"
        }

        return
    }

    & cargo @ArgumentList
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($ArgumentList -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Copy-FfiNativeBinary {
    param([Parameter(Mandatory)][string]$ProfileName)

    $profileDir = Join-Path $RepoRoot "target\$ProfileName"
    $sourceDll = Join-Path $profileDir 'ironrdp.dll'

    if (-not (Test-Path $sourceDll)) {
        throw "ffi binary not found at $sourceDll"
    }

    New-Item -ItemType Directory -Force -Path $FfiOutputDir | Out-Null
    Copy-Item $sourceDll $FfiOutputDll -Force
}

function Invoke-DotNetBuild {
    if ($SkipDotNet) {
        Write-Host 'Skipping .NET build' -ForegroundColor Yellow
        return
    }

    $dotnetArgs = @('build', $DotNetProject)
    if ($Release) {
        $dotnetArgs += @('-c', 'Release')
    }

    & dotnet @dotnetArgs
    if ($LASTEXITCODE -ne 0) {
        throw 'dotnet build failed'
    }
}

function Ensure-DiplomatTool {
    if (Test-Tool 'diplomat-tool') {
        return
    }

    Write-Host 'Installing diplomat-tool via xtask' -ForegroundColor Cyan
    Invoke-RepoCargo -ArgumentList @('xtask', 'ffi', 'install', '-v')
}

function Get-BuildArgs {
    param(
        [string[]]$BaseArgs = @(),
        [switch]$SupportsTimings
    )

    $args = New-Object System.Collections.Generic.List[string]
    foreach ($arg in $BaseArgs) {
        $args.Add($arg)
    }

    if ($Timings -and $SupportsTimings) {
        $args.Add('--timings')
    }

    if ($NativeCpu -and $script:UseCargoWrapper) {
        $args.Add('--use-native')
    }

    return $args.ToArray()
}

function Get-ProfileArgs {
    param([string]$Profile)

    if ([string]::IsNullOrWhiteSpace($Profile)) {
        return @()
    }

    return @('--profile', $Profile)
}

Push-Location $RepoRoot
try {
    Import-Module CargoTools -ErrorAction Stop

    if ($BootstrapTools) {
        Install-RustTool -ExecutableName 'cargo-nextest' -PackageName 'cargo-nextest'
        Install-RustTool -ExecutableName 'cargo-llvm-cov' -PackageName 'cargo-llvm-cov'
        Install-ChocoTool -ExecutableName 'ninja' -PackageName 'ninja'
        Install-ChocoTool -ExecutableName 'nasm' -PackageName 'nasm'
        Ensure-DiplomatTool
    }

    Initialize-CargoEnv
    $buildEnvironment = Test-BuildEnvironment -Detailed

    $machineDeps = $buildEnvironment.Results.MachineDeps
    if ($machineDeps -is [string] -and $machineDeps.StartsWith('fail', [System.StringComparison]::OrdinalIgnoreCase)) {
        $script:UseCargoWrapper = $false
        Write-Warning "CargoTools wrapper preflight is stricter than this workstation requires ($machineDeps); falling back to direct cargo with CargoTools-managed environment."
    }

    if ($Jobs -le 0) {
        $Jobs = [int](Get-OptimalBuildJobs)
    }
    $env:CARGO_BUILD_JOBS = "$Jobs"

    # Use the CargoTools wrapper for acceleration, but keep this script's phases explicit.
    $env:CARGOTOOLS_ENFORCE_QUALITY = '0'
    $env:CARGOTOOLS_RUN_TESTS_AFTER_BUILD = '0'
    $env:CARGOTOOLS_RUN_DOCTESTS_AFTER_BUILD = '0'

    if ($UseNextest) {
        $env:CARGO_USE_NEXTEST = '1'
    }

    if ($NoSccache) {
        Remove-Item Env:RUSTC_WRAPPER -ErrorAction SilentlyContinue
        $env:SCCACHE_DISABLE = '1'
    } else {
        $null = Start-SccacheServer
    }

    if ($NativeCpu -and -not $script:UseCargoWrapper) {
        Add-RustFlag -Flags @('-C', 'target-cpu=native')
    }

    Write-Host "Mode: $Mode" -ForegroundColor Cyan
    Write-Host "Jobs: $env:CARGO_BUILD_JOBS" -ForegroundColor Cyan
    Write-Host "CargoTools wrapper: $script:UseCargoWrapper" -ForegroundColor Cyan
    Write-Host "RUSTC_WRAPPER: $($env:RUSTC_WRAPPER)" -ForegroundColor Cyan
    Write-Host "CARGO_USE_LLD: $($env:CARGO_USE_LLD)" -ForegroundColor Cyan
    Write-Host "CMAKE_GENERATOR: $($env:CMAKE_GENERATOR)" -ForegroundColor Cyan

    switch ($Mode) {
        'check' {
            Invoke-RepoCargo -ArgumentList (Get-BuildArgs @('check', '--workspace'))
        }
        'client' {
            $clientProfile = if ($Release) { $ClientBuildProfile } else { '' }
            $clientArgs = @('build', '--package', 'ironrdp-client') + (Get-ProfileArgs $clientProfile)
            Invoke-RepoCargo -ArgumentList (Get-BuildArgs -BaseArgs $clientArgs -SupportsTimings)
        }
        'ffi' {
            $ffiProfile = if ($Release) { $FfiBuildProfile } else { '' }
            $ffiArgs = @('build', '--package', 'ffi') + (Get-ProfileArgs $ffiProfile)
            Invoke-RepoCargo -ArgumentList (Get-BuildArgs -BaseArgs $ffiArgs -SupportsTimings)
            Copy-FfiNativeBinary -ProfileName $(if ($Release) { $FfiBuildProfile } else { 'debug' })
            Ensure-DiplomatTool
            Invoke-RepoCargo -ArgumentList @('xtask', 'ffi', 'bindings', '-v')
            Invoke-DotNetBuild
        }
        'test' {
            if ($UseNextest -or (Test-Tool 'cargo-nextest')) {
                Invoke-RepoCargo -ArgumentList @('nextest', 'run', '--workspace', '--no-fail-fast')
            } else {
                Invoke-RepoCargo -ArgumentList @('test', '--workspace')
            }
        }
        'coverage' {
            Install-RustTool -ExecutableName 'cargo-llvm-cov' -PackageName 'cargo-llvm-cov'
            Invoke-RepoCargo -ArgumentList @('llvm-cov', '--workspace', 'nextest', '--no-fail-fast')
        }
        'fmt' {
            Invoke-RepoCargo -ArgumentList @('xtask', 'check', 'fmt', '-v')
        }
        'lints' {
            Invoke-RepoCargo -ArgumentList @('xtask', 'check', 'lints', '-v')
        }
        'all' {
            $clientProfile = if ($Release) { $ClientBuildProfile } else { '' }
            $ffiProfile = if ($Release) { $FfiBuildProfile } else { '' }

            Invoke-RepoCargo -ArgumentList (Get-BuildArgs @('check', '--workspace'))
            Invoke-RepoCargo -ArgumentList (Get-BuildArgs -BaseArgs (@('build', '--package', 'ironrdp-client') + (Get-ProfileArgs $clientProfile)) -SupportsTimings)
            Invoke-RepoCargo -ArgumentList (Get-BuildArgs -BaseArgs (@('build', '--package', 'ffi') + (Get-ProfileArgs $ffiProfile)) -SupportsTimings)
            Copy-FfiNativeBinary -ProfileName $(if ($Release) { $FfiBuildProfile } else { 'debug' })
            Ensure-DiplomatTool
            Invoke-RepoCargo -ArgumentList @('xtask', 'ffi', 'bindings', '-v')
            Invoke-DotNetBuild

            if ($UseNextest -or (Test-Tool 'cargo-nextest')) {
                Invoke-RepoCargo -ArgumentList @('nextest', 'run', '--workspace', '--no-fail-fast')
            } else {
                Invoke-RepoCargo -ArgumentList @('test', '--workspace')
            }
        }
    }
} finally {
    Pop-Location
}
