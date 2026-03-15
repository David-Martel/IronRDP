#requires -Version 7.0

[CmdletBinding()]
param(
    [ValidateSet('check', 'client', 'ffi', 'test', 'coverage', 'fmt', 'lints', 'package', 'publish', 'all')]
    [string]$Mode = 'all',

    [switch]$Release,
    [switch]$BootstrapTools,
    [switch]$UseNextest,
    [switch]$Timings,
    [switch]$NativeCpu,
    [switch]$NoSccache,
    [switch]$SkipDotNet,
    [string]$ArtifactRoot,
    [string]$DeploymentName,
    [string]$TargetMachine,
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
$script:CargoMachineConfig = $null
$script:ProfileMachineConfig = $null
$script:ProfileConfig = $null
$script:MachineIdentity = $null
$script:VersionInfo = $null
$script:ArtifactRoot = $null
$script:DeploymentName = $null

function Set-DefaultEnvVar {
    param(
        [Parameter(Mandatory)][string]$Name,
        [string]$Value
    )

    if ([string]::IsNullOrWhiteSpace($Value) -or -not [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($Name))) {
        return
    }

    [Environment]::SetEnvironmentVariable($Name, $Value)
}

function Import-OptionalModule {
    param([Parameter(Mandatory)][string]$Name)

    try {
        Import-Module $Name -ErrorAction Stop
        return $true
    } catch {
        Write-Warning "Optional module '$Name' could not be loaded: $($_.Exception.Message)"
        return $false
    }
}

function Get-NestedValue {
    param(
        [Parameter(Mandatory)]$InputObject,
        [Parameter(Mandatory)][string[]]$Path
    )

    $current = $InputObject
    foreach ($segment in $Path) {
        if ($null -eq $current) {
            return $null
        }

        if ($current -is [System.Collections.IDictionary]) {
            if (-not $current.Contains($segment)) {
                return $null
            }
            $current = $current[$segment]
            continue
        }

        $property = $current.PSObject.Properties[$segment]
        if (-not $property) {
            return $null
        }

        $current = $property.Value
    }

    return $current
}

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

    $shouldRetryWithoutSccache = -not $env:SCCACHE_DISABLE -and -not [string]::IsNullOrWhiteSpace($env:RUSTC_WRAPPER)

    try {
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
    } catch {
        if (-not $shouldRetryWithoutSccache) {
            throw
        }

        Write-Warning "Cargo command failed with sccache enabled; retrying once without sccache."
        Remove-Item Env:RUSTC_WRAPPER -ErrorAction SilentlyContinue
        $env:SCCACHE_DISABLE = '1'

        if ($script:UseCargoWrapper) {
            $exitCode = Invoke-CargoWrapper -ArgumentList $ArgumentList -WorkingDirectory $RepoRoot
            if ($exitCode -ne 0) {
                throw "cargo $($ArgumentList -join ' ') failed with exit code $exitCode after disabling sccache"
            }

            return
        }

        & cargo @ArgumentList
        if ($LASTEXITCODE -ne 0) {
            throw "cargo $($ArgumentList -join ' ') failed with exit code $LASTEXITCODE after disabling sccache"
        }
    }
}

function Copy-FfiNativeBinary {
    param([Parameter(Mandatory)][string]$ProfileName)

    $configuration = if ($ProfileName -eq 'debug') { 'Debug' } else { 'Release' }
    $profileDir = Resolve-CargoTargetDirectory -ProjectDir $RepoRoot -Configuration $configuration
    if ($ProfileName -ne 'debug' -and -not $profileDir.EndsWith($ProfileName, [System.StringComparison]::OrdinalIgnoreCase)) {
        $profileDir = Join-Path (Split-Path -Parent $profileDir) $ProfileName
    }
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

function Initialize-MachineBuildContext {
    $script:MachineIdentity = Get-MachineIdentity
    $script:CargoMachineConfig = Get-MachineConfig

    if (-not [string]::IsNullOrWhiteSpace($TargetMachine)) {
        $script:MachineIdentity = $TargetMachine
    }

    if (Get-Command Get-MachineConfiguration -ErrorAction SilentlyContinue) {
        $script:ProfileMachineConfig = Get-MachineConfiguration
    }

    if (Get-Command Get-ProfileConfiguration -ErrorAction SilentlyContinue) {
        $script:ProfileConfig = Get-ProfileConfiguration
    }

    Set-DefaultEnvVar -Name 'CARGO_TARGET_DIR' -Value (Get-NestedValue $script:CargoMachineConfig @('CargoTargetDir'))
    Set-DefaultEnvVar -Name 'SCCACHE_DIR' -Value (Get-NestedValue $script:CargoMachineConfig @('SccacheDir'))
    Set-DefaultEnvVar -Name 'SCCACHE_CACHE_SIZE' -Value (Get-NestedValue $script:CargoMachineConfig @('SccacheCacheSize'))
    Set-DefaultEnvVar -Name 'SCCACHE_IDLE_TIMEOUT' -Value ("$(Get-NestedValue $script:CargoMachineConfig @('SccacheIdleTimeout'))")
    Set-DefaultEnvVar -Name 'NUGET_PACKAGES' -Value $(if ($script:CargoMachineConfig) { Join-Path $script:CargoMachineConfig.CacheRoot 'nuget-packages' } else { $null })

    if ([string]::IsNullOrWhiteSpace($DeploymentName)) {
        $DeploymentName = 'windows-server-only'
    }
    $script:DeploymentName = $DeploymentName

    if ([string]::IsNullOrWhiteSpace($ArtifactRoot)) {
        $artifactRoots = @(
            (Get-NestedValue $script:ProfileMachineConfig @('paths', 'artifactRoot')),
            (Get-NestedValue $script:ProfileMachineConfig @('paths', 'deployRoot')),
            (Get-NestedValue $script:ProfileConfig @('paths', 'artifactRoot')),
            (Get-NestedValue $script:ProfileConfig @('paths', 'deployRoot')),
            $(if ($script:CargoMachineConfig) { Join-Path $script:CargoMachineConfig.CacheRoot 'artifacts' } else { $null }),
            (Join-Path $RepoRoot 'artifacts')
        ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }

        $ArtifactRoot = $artifactRoots[0]
    }

    $script:ArtifactRoot = Join-Path $ArtifactRoot (Join-Path 'IronRDP' (Join-Path $DeploymentName $script:MachineIdentity))
    $script:VersionInfo = Get-BuildVersionInfo -RepoRoot $RepoRoot -DefaultVersion '0.0.0-dev' -TagPrefix 'v'
    Set-BuildVersionEnvironment -VersionInfo $script:VersionInfo -Prefixes @('IRONRDP')
}

function Publish-RepoArtifact {
    param(
        [Parameter(Mandatory)][string]$SourcePath,
        [Parameter(Mandatory)][string]$RelativeDirectory,
        [string]$DestinationFileName,
        [string]$ArtifactKind = 'native'
    )

    $destinationDirectory = Join-Path $script:ArtifactRoot $RelativeDirectory
    Publish-BuildArtifact -SourcePath $SourcePath `
        -DestinationDirectory $destinationDirectory `
        -DestinationFileName $DestinationFileName `
        -VersionInfo $script:VersionInfo `
        -ArtifactKind $ArtifactKind | Out-Null

    [pscustomobject]@{
        kind = $ArtifactKind
        source = $SourcePath
        destination = Join-Path $destinationDirectory $(if ($DestinationFileName) { $DestinationFileName } else { Split-Path -Leaf $SourcePath })
    }
}

function Publish-BuildOutputs {
    $publishedArtifacts = New-Object System.Collections.Generic.List[object]
    $clientProfile = if ($Release) { $ClientBuildProfile } else { 'debug' }
    $ffiProfile = if ($Release) { $FfiBuildProfile } else { 'debug' }
    $configuration = if ($Release) { 'Release' } else { 'Debug' }

    $clientDir = Resolve-CargoTargetDirectory -ProjectDir $RepoRoot -Configuration $configuration
    if ($clientProfile -ne 'debug' -and -not $clientDir.EndsWith($clientProfile, [System.StringComparison]::OrdinalIgnoreCase)) {
        $clientDir = Join-Path (Split-Path -Parent $clientDir) $clientProfile
    }
    $clientExe = Join-Path $clientDir 'ironrdp-client.exe'
    if (Test-Path $clientExe) {
        $publishedArtifacts.Add((Publish-RepoArtifact -SourcePath $clientExe -RelativeDirectory 'client' -ArtifactKind 'native-client'))
    }

    $ffiDir = Resolve-CargoTargetDirectory -ProjectDir $RepoRoot -Configuration $configuration
    if ($ffiProfile -ne 'debug' -and -not $ffiDir.EndsWith($ffiProfile, [System.StringComparison]::OrdinalIgnoreCase)) {
        $ffiDir = Join-Path (Split-Path -Parent $ffiDir) $ffiProfile
    }
    $ffiDll = Join-Path $ffiDir 'ironrdp.dll'
    if (Test-Path $ffiDll) {
        $publishedArtifacts.Add((Publish-RepoArtifact -SourcePath $ffiDll -RelativeDirectory 'ffi' -DestinationFileName 'DevolutionsIronRdp.dll' -ArtifactKind 'ffi-native'))
    }

    if (-not $SkipDotNet) {
        $dotnetConfiguration = if ($Release) { 'Release' } else { 'Debug' }
        $dotnetOutDir = Join-Path $RepoRoot "ffi\dotnet\Devolutions.IronRdp\bin\$dotnetConfiguration"
        if (Test-Path $dotnetOutDir) {
            $packages = Get-ChildItem $dotnetOutDir -Filter 'Devolutions.IronRdp*.nupkg' -Recurse -File -ErrorAction SilentlyContinue
            foreach ($package in $packages) {
                $publishedArtifacts.Add((Publish-RepoArtifact -SourcePath $package.FullName -RelativeDirectory 'nuget' -ArtifactKind 'nuget-package'))
            }
        }
    }

    return $publishedArtifacts
}

function Write-BuildManifest {
    param([object[]]$Artifacts = @())

    New-Item -ItemType Directory -Force -Path $script:ArtifactRoot | Out-Null

    $manifest = [ordered]@{
        generatedAt = (Get-Date).ToString('o')
        repoRoot = $RepoRoot
        deploymentName = $script:DeploymentName
        machineIdentity = $script:MachineIdentity
        targetMachine = $TargetMachine
        version = $script:VersionInfo
        environment = [ordered]@{
            cargoTargetDir = $env:CARGO_TARGET_DIR
            sccacheDir = $env:SCCACHE_DIR
            sccacheCacheSize = $env:SCCACHE_CACHE_SIZE
            sccacheIdleTimeout = $env:SCCACHE_IDLE_TIMEOUT
            nugetPackages = $env:NUGET_PACKAGES
            cargoBuildJobs = $env:CARGO_BUILD_JOBS
            cargoUseLld = $env:CARGO_USE_LLD
            cargoUseNextest = $env:CARGO_USE_NEXTEST
        }
        cargoMachineConfig = $script:CargoMachineConfig
        profileMachineConfig = $script:ProfileMachineConfig
        profileConfig = [ordered]@{
            module = Get-NestedValue $script:ProfileConfig @('module')
            defaults = Get-NestedValue $script:ProfileConfig @('defaults')
            paths = Get-NestedValue $script:ProfileConfig @('paths')
        }
        artifacts = $Artifacts
    }

    $manifestPath = Join-Path $script:ArtifactRoot 'build-manifest.json'
    $manifest | ConvertTo-Json -Depth 10 | Set-Content -Path $manifestPath -Encoding utf8
    Write-Host "Build manifest: $manifestPath" -ForegroundColor Cyan
}

Push-Location $RepoRoot
try {
    Import-Module CargoTools -ErrorAction Stop
    $null = Import-OptionalModule -Name 'ProfileUtilities'
    $null = Import-OptionalModule -Name 'MachineConfiguration'

    if ($BootstrapTools) {
        Install-RustTool -ExecutableName 'cargo-nextest' -PackageName 'cargo-nextest'
        Install-RustTool -ExecutableName 'cargo-llvm-cov' -PackageName 'cargo-llvm-cov'
        Install-ChocoTool -ExecutableName 'ninja' -PackageName 'ninja'
        Install-ChocoTool -ExecutableName 'nasm' -PackageName 'nasm'
        Ensure-DiplomatTool
    }

    Initialize-CargoEnv
    Initialize-MachineBuildContext
    $buildEnvironment = Test-BuildEnvironment -Detailed

    $machineDeps = $buildEnvironment.Results.MachineDeps
    if ($machineDeps -is [string] -and $machineDeps.StartsWith('fail', [System.StringComparison]::OrdinalIgnoreCase)) {
        $script:UseCargoWrapper = $false
        Write-Warning "CargoTools wrapper preflight is stricter than this workstation requires ($machineDeps); falling back to direct cargo with CargoTools-managed environment."
    }

    if ($Jobs -le 0) {
        $configuredBuildJobs = Get-NestedValue $script:CargoMachineConfig @('BuildJobs')
        if ($configuredBuildJobs) {
            $Jobs = [int]$configuredBuildJobs
        } else {
            $Jobs = [int](Get-OptimalBuildJobs)
        }
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
        $script:UseCargoWrapper = $false
        Remove-Item Env:RUSTC_WRAPPER -ErrorAction SilentlyContinue
        $env:SCCACHE_DISABLE = '1'
    } else {
        $null = Start-SccacheServer
    }

    if ($NativeCpu -and -not $script:UseCargoWrapper) {
        Add-RustFlag -Flags @('-C', 'target-cpu=native')
    }

    Write-Host "Mode: $Mode" -ForegroundColor Cyan
    Write-Host "Machine identity: $script:MachineIdentity" -ForegroundColor Cyan
    Write-Host "Artifact root: $script:ArtifactRoot" -ForegroundColor Cyan
    Write-Host "Jobs: $env:CARGO_BUILD_JOBS" -ForegroundColor Cyan
    Write-Host "CargoTools wrapper: $script:UseCargoWrapper" -ForegroundColor Cyan
    Write-Host "RUSTC_WRAPPER: $($env:RUSTC_WRAPPER)" -ForegroundColor Cyan
    Write-Host "CARGO_USE_LLD: $($env:CARGO_USE_LLD)" -ForegroundColor Cyan
    Write-Host "CMAKE_GENERATOR: $($env:CMAKE_GENERATOR)" -ForegroundColor Cyan
    Write-Host "CARGO_TARGET_DIR: $($env:CARGO_TARGET_DIR)" -ForegroundColor Cyan
    Write-Host "SCCACHE_DIR: $($env:SCCACHE_DIR)" -ForegroundColor Cyan
    Write-Host "NUGET_PACKAGES: $($env:NUGET_PACKAGES)" -ForegroundColor Cyan
    Write-Host "Machine config provider: $(if (Get-Command Get-MachineConfiguration -ErrorAction SilentlyContinue) { (Get-Command Get-MachineConfiguration).Source } else { 'unavailable' })" -ForegroundColor Cyan
    Write-Host "Profile config provider: $(if (Get-Command Get-ProfileConfiguration -ErrorAction SilentlyContinue) { (Get-Command Get-ProfileConfiguration).Source } else { 'unavailable' })" -ForegroundColor Cyan

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
            Invoke-RepoCargo -ArgumentList @('xtask', 'ffi', 'bindings', '-v', '--skip-dotnet-build')
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
            Invoke-RepoCargo -ArgumentList @('xtask', 'ffi', 'bindings', '-v', '--skip-dotnet-build')
            Invoke-DotNetBuild

            if ($UseNextest -or (Test-Tool 'cargo-nextest')) {
                Invoke-RepoCargo -ArgumentList @('nextest', 'run', '--workspace', '--no-fail-fast')
            } else {
                Invoke-RepoCargo -ArgumentList @('test', '--workspace')
            }
        }
        'package' {
            $Release = $true
            $ffiProfile = $FfiBuildProfile
            $clientProfile = $ClientBuildProfile

            Invoke-RepoCargo -ArgumentList (Get-BuildArgs -BaseArgs (@('build', '--package', 'ironrdp-client') + (Get-ProfileArgs $clientProfile)) -SupportsTimings)
            Invoke-RepoCargo -ArgumentList (Get-BuildArgs -BaseArgs (@('build', '--package', 'ffi') + (Get-ProfileArgs $ffiProfile)) -SupportsTimings)
            Copy-FfiNativeBinary -ProfileName $ffiProfile
            Ensure-DiplomatTool
            Invoke-RepoCargo -ArgumentList @('xtask', 'ffi', 'bindings', '-v', '--skip-dotnet-build')
            Invoke-DotNetBuild
            $artifacts = Publish-BuildOutputs
            Write-BuildManifest -Artifacts $artifacts
        }
        'publish' {
            $Release = $true
            $env:CARGO_USE_NEXTEST = '1'

            Invoke-RepoCargo -ArgumentList @('nextest', 'run', '--workspace', '--no-fail-fast')
            $ffiProfile = $FfiBuildProfile
            $clientProfile = $ClientBuildProfile
            Invoke-RepoCargo -ArgumentList (Get-BuildArgs -BaseArgs (@('build', '--package', 'ironrdp-client') + (Get-ProfileArgs $clientProfile)) -SupportsTimings)
            Invoke-RepoCargo -ArgumentList (Get-BuildArgs -BaseArgs (@('build', '--package', 'ffi') + (Get-ProfileArgs $ffiProfile)) -SupportsTimings)
            Copy-FfiNativeBinary -ProfileName $ffiProfile
            Ensure-DiplomatTool
            Invoke-RepoCargo -ArgumentList @('xtask', 'ffi', 'bindings', '-v', '--skip-dotnet-build')
            Invoke-DotNetBuild
            $artifacts = Publish-BuildOutputs
            Write-BuildManifest -Artifacts $artifacts
        }
    }
} finally {
    Pop-Location
}
