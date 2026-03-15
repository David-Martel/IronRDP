#requires -Version 7.0

[CmdletBinding()]
param(
    [ValidateSet('check', 'client', 'ffi', 'test', 'coverage', 'fmt', 'lints', 'package', 'publish', 'doctor', 'all')]
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
    [int]$Jobs,
    [string]$InstallerPublisher = 'CN=David-Martel IronRDP Test',
    [string]$InstallerCertificatePath,
    [string]$InstallerCertificatePassword,
    [string]$ReleaseRepo,
    [string]$ReleaseTag,
    [switch]$SkipMsix,
    [switch]$SkipMsi
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
$script:ImportedModules = [ordered]@{}
$script:ToolchainInfo = [ordered]@{}
$script:HardwareProfile = [ordered]@{}

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

function Register-ImportedModule {
    param([Parameter(Mandatory)][string]$Name)

    $module = Get-Module -Name $Name | Select-Object -First 1
    if (-not $module) {
        return
    }

    $script:ImportedModules[$Name] = [ordered]@{
        name = $module.Name
        version = "$($module.Version)"
        path = $module.Path
    }
}

function Resolve-ToolPath {
    param(
        [string]$CommandName,
        [string]$LiteralPath
    )

    if (-not [string]::IsNullOrWhiteSpace($LiteralPath) -and (Test-Path -LiteralPath $LiteralPath)) {
        return (Resolve-Path -LiteralPath $LiteralPath).Path
    }

    if (-not [string]::IsNullOrWhiteSpace($CommandName)) {
        $command = Get-Command $CommandName -ErrorAction SilentlyContinue
        if ($command) {
            return $command.Source
        }
    }

    return $null
}

function Add-PathEntry {
    param([string]$PathEntry)

    if ([string]::IsNullOrWhiteSpace($PathEntry) -or -not (Test-Path -LiteralPath $PathEntry)) {
        return
    }

    $resolvedEntry = (Resolve-Path -LiteralPath $PathEntry).Path
    $pathEntries = @($env:PATH -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($pathEntries -contains $resolvedEntry) {
        return
    }

    $env:PATH = "$resolvedEntry;$env:PATH"
}

function Set-DefaultEnvPathList {
    param(
        [Parameter(Mandatory)][string]$Name,
        [string[]]$Values
    )

    if (-not [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($Name))) {
        return
    }

    $resolvedValues = @(
        foreach ($value in $Values) {
            if (-not [string]::IsNullOrWhiteSpace($value) -and (Test-Path -LiteralPath $value)) {
                (Resolve-Path -LiteralPath $value).Path
            }
        }
    )

    if ($resolvedValues.Count -eq 0) {
        return
    }

    [Environment]::SetEnvironmentVariable($Name, ($resolvedValues -join ';'))
}

function ConvertTo-VersionValue {
    param([string]$VersionText)

    if ([string]::IsNullOrWhiteSpace($VersionText)) {
        return [version]'0.0'
    }

    try {
        return [version]$VersionText
    } catch {
        return [version]'0.0'
    }
}

function Get-WindowsSdkInfoLocal {
    $sdkBase = 'C:\Program Files (x86)\Windows Kits\10'
    $libBase = Join-Path $sdkBase 'Lib'
    $includeBase = Join-Path $sdkBase 'Include'
    if (-not (Test-Path -LiteralPath $libBase) -or -not (Test-Path -LiteralPath $includeBase)) {
        return $null
    }

    $sdkVersion = Get-ChildItem -Path $libBase -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
        Sort-Object { ConvertTo-VersionValue $_.Name } -Descending |
        Select-Object -First 1 -ExpandProperty Name

    if (-not $sdkVersion) {
        return $null
    }

    [pscustomobject]@{
        sdkBase = $sdkBase
        version = $sdkVersion
        binDir = Join-Path $sdkBase "bin\$sdkVersion"
        binDirX64 = Join-Path $sdkBase "bin\$sdkVersion\x64"
        binDirX86 = Join-Path $sdkBase "bin\$sdkVersion\x86"
        umLibDir = Join-Path $libBase "$sdkVersion\um\x64"
        ucrtLibDir = Join-Path $libBase "$sdkVersion\ucrt\x64"
        sharedIncludeDir = Join-Path $includeBase "$sdkVersion\shared"
        ucrtIncludeDir = Join-Path $includeBase "$sdkVersion\ucrt"
        umIncludeDir = Join-Path $includeBase "$sdkVersion\um"
        rcExe = Resolve-ToolPath -LiteralPath (Join-Path $sdkBase "bin\$sdkVersion\x64\rc.exe")
        mtExe = Resolve-ToolPath -LiteralPath (Join-Path $sdkBase "bin\$sdkVersion\x64\mt.exe")
    }
}

function Get-MsvcToolchainFromVisualStudio {
    param(
        [Parameter(Mandatory)][string]$InstallationPath,
        [string]$DisplayName,
        [string]$ChannelId,
        [string]$InstallationVersion,
        [bool]$IsPrerelease = $false
    )

    if (-not (Test-Path -LiteralPath $InstallationPath)) {
        return $null
    }

    $msvcBase = Join-Path $InstallationPath 'VC\Tools\MSVC'
    if (-not (Test-Path -LiteralPath $msvcBase)) {
        return $null
    }

    $msvcVersion = Get-ChildItem -Path $msvcBase -Directory -ErrorAction SilentlyContinue |
        Sort-Object { ConvertTo-VersionValue $_.Name } -Descending |
        Select-Object -First 1 -ExpandProperty Name

    if (-not $msvcVersion) {
        return $null
    }

    $toolsetRoot = Join-Path $msvcBase $msvcVersion
    $binDir = Join-Path $toolsetRoot 'bin\Hostx64\x64'
    $libDir = Join-Path $toolsetRoot 'lib\x64'
    $includeDir = Join-Path $toolsetRoot 'include'
    $pathText = $InstallationPath.ToLowerInvariant()
    $displayText = "$DisplayName $ChannelId".ToLowerInvariant()
    $preference = 0

    if ($pathText -match '\\18\\insiders' -or $displayText -match 'insiders') {
        $preference += 500
    } elseif ($pathText -match '\\18\\preview' -or $displayText -match 'preview') {
        $preference += 450
    } elseif ($pathText -match '\\2026\\') {
        $preference += 425
    } elseif ($pathText -match '\\2022\\') {
        $preference += 300
    }

    if ($displayText -match 'enterprise') {
        $preference += 40
    } elseif ($displayText -match 'professional') {
        $preference += 30
    } elseif ($displayText -match 'community') {
        $preference += 20
    } elseif ($displayText -match 'buildtools') {
        $preference += 10
    }

    [pscustomobject]@{
        installationPath = (Resolve-Path -LiteralPath $InstallationPath).Path
        displayName = if ($DisplayName) { $DisplayName } else { Split-Path -Leaf $InstallationPath }
        channelId = $ChannelId
        installationVersion = $InstallationVersion
        isPrerelease = $IsPrerelease
        msvcVersion = $msvcVersion
        toolsetRoot = if (Test-Path -LiteralPath $toolsetRoot) { (Resolve-Path -LiteralPath $toolsetRoot).Path } else { $null }
        binDir = if (Test-Path -LiteralPath $binDir) { (Resolve-Path -LiteralPath $binDir).Path } else { $null }
        libDir = if (Test-Path -LiteralPath $libDir) { (Resolve-Path -LiteralPath $libDir).Path } else { $null }
        includeDir = if (Test-Path -LiteralPath $includeDir) { (Resolve-Path -LiteralPath $includeDir).Path } else { $null }
        clExe = Resolve-ToolPath -LiteralPath (Join-Path $binDir 'cl.exe')
        linkExe = Resolve-ToolPath -LiteralPath (Join-Path $binDir 'link.exe')
        preference = $preference
    }
}

function Get-PreferredVisualStudioInfo {
    $candidates = New-Object System.Collections.Generic.List[object]
    $seen = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::OrdinalIgnoreCase)

    $vswherePath = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path -LiteralPath $vswherePath) {
        try {
            $vswhereJson = & $vswherePath -products * -prerelease -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format json 2>$null
            if ($vswhereJson) {
                foreach ($instance in ($vswhereJson | ConvertFrom-Json)) {
                    if (-not $instance.installationPath -or -not $seen.Add($instance.installationPath)) {
                        continue
                    }

                    $candidate = Get-MsvcToolchainFromVisualStudio `
                        -InstallationPath $instance.installationPath `
                        -DisplayName $instance.displayName `
                        -ChannelId $instance.channelId `
                        -InstallationVersion $instance.installationVersion `
                        -IsPrerelease ([bool]$instance.isPrerelease)
                    if ($candidate) {
                        $candidates.Add($candidate)
                    }
                }
            }
        } catch {
            Write-Verbose "vswhere probing failed: $_"
        }
    }

    $fallbackPaths = @(
        'C:\Program Files\Microsoft Visual Studio\18\Insiders',
        'C:\Program Files\Microsoft Visual Studio\18\Preview',
        'C:\Program Files\Microsoft Visual Studio\2026\Enterprise',
        'C:\Program Files\Microsoft Visual Studio\2026\Professional',
        'C:\Program Files\Microsoft Visual Studio\2026\Community',
        'C:\Program Files\Microsoft Visual Studio\2026\BuildTools',
        'C:\Program Files\Microsoft Visual Studio\2022\Enterprise',
        'C:\Program Files\Microsoft Visual Studio\2022\Professional',
        'C:\Program Files\Microsoft Visual Studio\2022\Community',
        'C:\Program Files\Microsoft Visual Studio\2022\BuildTools'
    )

    foreach ($path in $fallbackPaths) {
        if (-not (Test-Path -LiteralPath $path) -or -not $seen.Add($path)) {
            continue
        }

        $candidate = Get-MsvcToolchainFromVisualStudio -InstallationPath $path
        if ($candidate) {
            $candidates.Add($candidate)
        }
    }

    if ($candidates.Count -eq 0) {
        return $null
    }

    $selected = $candidates |
        Sort-Object `
            @{ Expression = { $_.preference }; Descending = $true }, `
            @{ Expression = { ConvertTo-VersionValue $_.installationVersion }; Descending = $true }, `
            @{ Expression = { ConvertTo-VersionValue $_.msvcVersion }; Descending = $true } |
        Select-Object -First 1

    return $selected
}

function Get-LlvmToolchainInfoLocal {
    $llvmRootCandidates = @($env:LLVM_PATH, 'C:\Program Files\LLVM') | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    foreach ($root in $llvmRootCandidates) {
        if (-not (Test-Path -LiteralPath $root)) {
            continue
        }

        $binDir = Join-Path $root 'bin'
        $lldLink = Resolve-ToolPath -LiteralPath (Join-Path $binDir 'lld-link.exe')
        $clangCl = Resolve-ToolPath -LiteralPath (Join-Path $binDir 'clang-cl.exe')
        $clangExe = Resolve-ToolPath -LiteralPath (Join-Path $binDir 'clang.exe')
        $llvmAr = Resolve-ToolPath -LiteralPath (Join-Path $binDir 'llvm-ar.exe')
        $version = $null
        if ($clangExe) {
            try {
                $versionOutput = & $clangExe --version 2>$null
                if ($versionOutput -and $versionOutput[0] -match '(\d+\.\d+\.\d+)') {
                    $version = $Matches[1]
                }
            } catch {
                Write-Verbose "clang version probe failed: $_"
            }
        }

        return [pscustomobject]@{
            root = (Resolve-Path -LiteralPath $root).Path
            binDir = if (Test-Path -LiteralPath $binDir) { (Resolve-Path -LiteralPath $binDir).Path } else { $null }
            version = $version
            lldLink = $lldLink
            clangCl = $clangCl
            llvmAr = $llvmAr
        }
    }

    return $null
}

function Get-OneApiInfoLocal {
    $root = 'C:\Program Files (x86)\Intel\oneAPI'
    if (-not (Test-Path -LiteralPath $root)) {
        return $null
    }

    $version = Get-ChildItem -Path $root -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^\d{4}\.\d+$' } |
        Sort-Object { ConvertTo-VersionValue $_.Name } -Descending |
        Select-Object -First 1 -ExpandProperty Name

    [pscustomobject]@{
        root = (Resolve-Path -LiteralPath $root).Path
        version = $version
        setvars = Resolve-ToolPath -LiteralPath (Join-Path $root 'setvars.bat')
        setvarsVcvars = Resolve-ToolPath -LiteralPath (Join-Path $root 'setvars-vcvarsall.bat')
    }
}

function Get-CudaInfoLocal {
    $root = 'C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA'
    if (-not (Test-Path -LiteralPath $root)) {
        return $null
    }

    $selected = Get-ChildItem -Path $root -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^v\d+(\.\d+)*$' } |
        Sort-Object { ConvertTo-VersionValue ($_.Name.TrimStart('v')) } -Descending |
        Select-Object -First 1

    if (-not $selected) {
        return $null
    }

    $toolkitRoot = (Resolve-Path -LiteralPath $selected.FullName).Path
    [pscustomobject]@{
        root = $toolkitRoot
        version = $selected.Name
        binDir = Resolve-ToolPath -LiteralPath (Join-Path $toolkitRoot 'bin')
        nvcc = Resolve-ToolPath -LiteralPath (Join-Path $toolkitRoot 'bin\nvcc.exe')
    }
}

function ConvertTo-LinkSpeedGbps {
    param([string]$LinkSpeed)

    if ([string]::IsNullOrWhiteSpace($LinkSpeed)) {
        return $null
    }

    if ($LinkSpeed -match '^\s*([\d.]+)\s*Gbps\s*$') {
        return [double]$Matches[1]
    }

    if ($LinkSpeed -match '^\s*([\d.]+)\s*Mbps\s*$') {
        return ([double]$Matches[1]) / 1000.0
    }

    return $null
}

function Get-NetworkInterfaceProfile {
    $adapters = @()

    try {
        $adapters = @(Get-NetAdapter -ErrorAction Stop | Sort-Object -Property LinkSpeed -Descending)
    } catch {
        Write-Verbose "Network adapter probing failed: $_"
        return @()
    }

    return @(
        foreach ($adapter in $adapters) {
            [pscustomobject]@{
                name = $adapter.Name
                interfaceDescription = $adapter.InterfaceDescription
                status = $adapter.Status
                linkSpeed = $adapter.LinkSpeed
                linkSpeedGbps = ConvertTo-LinkSpeedGbps $adapter.LinkSpeed
                macAddress = $adapter.MacAddress
            }
        }
    )
}

function Get-HardwareProfile {
    $cpu = $null
    $gpus = @()
    $networkAdapters = Get-NetworkInterfaceProfile

    try {
        $cpuInstance = Get-CimInstance Win32_Processor -ErrorAction Stop | Select-Object -First 1
        if ($cpuInstance) {
            $cpu = [pscustomobject]@{
                name = $cpuInstance.Name
                manufacturer = $cpuInstance.Manufacturer
                cores = $cpuInstance.NumberOfCores
                logicalProcessors = $cpuInstance.NumberOfLogicalProcessors
                maxClockSpeedMHz = $cpuInstance.MaxClockSpeed
            }
        }
    } catch {
        Write-Verbose "CPU probing failed: $_"
    }

    try {
        $gpus = @(
            foreach ($gpu in (Get-CimInstance Win32_VideoController -ErrorAction Stop)) {
                [pscustomobject]@{
                    name = $gpu.Name
                    vendor = $gpu.AdapterCompatibility
                    driverVersion = $gpu.DriverVersion
                    pnpDeviceId = $gpu.PNPDeviceID
                }
            }
        )
    } catch {
        Write-Verbose "GPU probing failed: $_"
    }

    $hasIntelGpu = $gpus | Where-Object { $_.vendor -match 'intel' -or $_.name -match 'intel' } | Select-Object -First 1
    $hasNvidiaGpu = $gpus | Where-Object { $_.vendor -match 'nvidia' -or $_.name -match 'nvidia' } | Select-Object -First 1
    $activeAdapters = @($networkAdapters | Where-Object { $_.status -eq 'Up' })
    $fastestNetworkGbps = $activeAdapters | Measure-Object -Property linkSpeedGbps -Maximum
    $primaryAdapter = $activeAdapters | Sort-Object -Property linkSpeedGbps -Descending | Select-Object -First 1

    [pscustomobject]@{
        cpu = $cpu
        gpus = $gpus
        networkAdapters = $networkAdapters
        hasIntelCpu = [bool]($cpu -and $cpu.manufacturer -match 'intel')
        hasIntelGpu = [bool]$hasIntelGpu
        hasNvidiaGpu = [bool]$hasNvidiaGpu
        activeAdapterCount = $activeAdapters.Count
        primaryNetworkAdapter = $primaryAdapter
        fastestNetworkGbps = $fastestNetworkGbps.Maximum
        buildClass = if ($hasNvidiaGpu) {
            'intel-cpu-nvidia-gpu'
        } elseif ($hasIntelGpu) {
            'intel-cpu-intel-gpu'
        } else {
            'intel-cpu-software'
        }
    }
}

function Initialize-PreferredToolchainContext {
    $windowsSdk = Get-WindowsSdkInfoLocal
    $preferredVs = Get-PreferredVisualStudioInfo
    $llvm = Get-LlvmToolchainInfoLocal
    $oneApi = Get-OneApiInfoLocal
    $cuda = Get-CudaInfoLocal
    $hardware = Get-HardwareProfile

    $script:ToolchainInfo = [ordered]@{
        commands = [ordered]@{
            cargo = Resolve-ToolPath -CommandName 'cargo'
            rustc = Resolve-ToolPath -CommandName 'rustc'
            rustup = Resolve-ToolPath -CommandName 'rustup'
            dotnet = Resolve-ToolPath -CommandName 'dotnet'
            sccache = Resolve-ToolPath -CommandName 'sccache'
            ninja = Resolve-ToolPath -CommandName 'ninja'
            cmake = Resolve-ToolPath -CommandName 'cmake'
            nasm = Resolve-ToolPath -CommandName 'nasm'
        }
        visualStudio = $preferredVs
        windowsSdk = $windowsSdk
        llvm = $llvm
        oneApi = $oneApi
        cuda = $cuda
    }
    $script:HardwareProfile = $hardware

    if ($preferredVs) {
        Add-PathEntry -PathEntry $preferredVs.binDir
        Set-DefaultEnvVar -Name 'VSINSTALLDIR' -Value $preferredVs.installationPath
        Set-DefaultEnvVar -Name 'VCINSTALLDIR' -Value (Join-Path $preferredVs.installationPath 'VC')
        Set-DefaultEnvVar -Name 'VCToolsInstallDir' -Value $preferredVs.toolsetRoot
        Set-DefaultEnvPathList -Name 'INCLUDE' -Values @(
            $preferredVs.includeDir,
            $(if ($windowsSdk) { $windowsSdk.ucrtIncludeDir } else { $null }),
            $(if ($windowsSdk) { $windowsSdk.umIncludeDir } else { $null }),
            $(if ($windowsSdk) { $windowsSdk.sharedIncludeDir } else { $null })
        )
        Set-DefaultEnvPathList -Name 'LIB' -Values @(
            $preferredVs.libDir,
            $(if ($windowsSdk) { $windowsSdk.ucrtLibDir } else { $null }),
            $(if ($windowsSdk) { $windowsSdk.umLibDir } else { $null })
        )
    }

    if ($windowsSdk) {
        Add-PathEntry -PathEntry $windowsSdk.binDirX64
        Add-PathEntry -PathEntry $windowsSdk.binDirX86
        Set-DefaultEnvVar -Name 'WindowsSdkDir' -Value $windowsSdk.sdkBase
        Set-DefaultEnvVar -Name 'WindowsSDKVersion' -Value "$($windowsSdk.version)\"
        Set-DefaultEnvVar -Name 'RC' -Value $windowsSdk.rcExe
        Set-DefaultEnvVar -Name 'MT' -Value $windowsSdk.mtExe
        Set-DefaultEnvVar -Name 'CMAKE_RC_COMPILER' -Value $windowsSdk.rcExe
        Set-DefaultEnvVar -Name 'CMAKE_MT' -Value $windowsSdk.mtExe
    }

    if ($llvm) {
        Add-PathEntry -PathEntry $llvm.binDir
        Set-DefaultEnvVar -Name 'LLVM_PATH' -Value $llvm.root
        Set-DefaultEnvVar -Name 'CARGO_LLD_PATH' -Value $llvm.lldLink
        if ($llvm.lldLink) {
            Set-DefaultEnvVar -Name 'CARGO_USE_LLD' -Value '1'
        }
    }

    if ($cuda) {
        Set-DefaultEnvVar -Name 'CUDA_PATH' -Value $cuda.root
    }

    if ($oneApi) {
        Set-DefaultEnvVar -Name 'ONEAPI_ROOT' -Value $oneApi.root
    }

    if (Test-Tool 'ninja') {
        Set-DefaultEnvVar -Name 'CMAKE_GENERATOR' -Value 'Ninja'
    }
}

function Set-DeploymentMetadataEnvironment {
    Set-DefaultEnvVar -Name 'IRONRDP_DEPLOYMENT_NAME' -Value $script:DeploymentName
    Set-DefaultEnvVar -Name 'IRONRDP_MACHINE_IDENTITY' -Value $script:MachineIdentity
    Set-DefaultEnvVar -Name 'IRONRDP_TARGET_MACHINE' -Value $TargetMachine
    Set-DefaultEnvVar -Name 'IRONRDP_ARTIFACT_ROOT' -Value $script:ArtifactRoot
    Set-DefaultEnvVar -Name 'IRONRDP_BUILD_VERSION' -Value (Get-NestedValue $script:VersionInfo @('SemVer'))
    Set-DefaultEnvVar -Name 'IRONRDP_BUILD_CLASS' -Value (Get-NestedValue $script:HardwareProfile @('buildClass'))
    Set-DefaultEnvVar -Name 'IRONRDP_PRIMARY_NETWORK_GBPS' -Value "$([string](Get-NestedValue $script:HardwareProfile @('fastestNetworkGbps')))"
}

function Invoke-RepoCargo {
    param(
        [Parameter(Mandatory)][string[]]$ArgumentList
    )

    if ($script:UseCargoWrapper) {
        $command = $ArgumentList[0]
        $additionalArgs = if ($ArgumentList.Length -gt 1) { $ArgumentList[1..($ArgumentList.Length - 1)] } else { @() }
        $wrapperResult = Invoke-CargoWrapper -Command $command -AdditionalArgs $additionalArgs -WorkingDirectory $RepoRoot
        $exitCode = 0

        if ($wrapperResult -is [int]) {
            $exitCode = $wrapperResult
        } elseif ($wrapperResult -is [array] -and $wrapperResult.Length -eq 1 -and $wrapperResult[0] -is [int]) {
            $exitCode = $wrapperResult[0]
        } elseif ($LASTEXITCODE -is [int]) {
            $exitCode = $LASTEXITCODE
        }

        if ($exitCode -ne 0) {
            throw "cargo $($ArgumentList -join ' ') failed with exit code $exitCode"
        }

        return
    }

    $shouldRetryWithoutSccache = -not $env:SCCACHE_DISABLE -and -not [string]::IsNullOrWhiteSpace($env:RUSTC_WRAPPER)

    try {
        & cargo @ArgumentList
        if ($LASTEXITCODE -ne 0) {
            throw "cargo $($ArgumentList -join ' ') failed with exit code $LASTEXITCODE"
        }
    } catch {
        if (-not $shouldRetryWithoutSccache) {
            throw
        }

        Write-Warning "Cargo command failed with sccache enabled outside the CargoTools wrapper; retrying once without sccache."
        Remove-Item Env:RUSTC_WRAPPER -ErrorAction SilentlyContinue
        Remove-Item Env:CARGO_BUILD_RUSTC_WRAPPER -ErrorAction SilentlyContinue
        Remove-Item Env:RUSTC_WORKSPACE_WRAPPER -ErrorAction SilentlyContinue
        $env:SCCACHE_DISABLE = '1'

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
    Set-DeploymentMetadataEnvironment
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

function Publish-DeploymentSupportFiles {
    $publishedArtifacts = New-Object System.Collections.Generic.List[object]
    $supportFiles = @(
        @{
            SourcePath = Join-Path $RepoRoot 'scripts\windows\Install-IronRdpPackage.ps1'
            RelativeDirectory = 'tools'
            ArtifactKind = 'deployment-tool'
        },
        @{
            SourcePath = Join-Path $RepoRoot 'scripts\windows\Invoke-IronRdpSmokeTest.ps1'
            RelativeDirectory = 'tools'
            ArtifactKind = 'deployment-tool'
        },
        @{
            SourcePath = Join-Path $RepoRoot 'scripts\windows\New-IronRdpInstallers.ps1'
            RelativeDirectory = 'tools'
            ArtifactKind = 'deployment-tool'
        },
        @{
            SourcePath = Join-Path $RepoRoot 'scripts\windows\Invoke-HyperVInstallerTest.ps1'
            RelativeDirectory = 'tools'
            ArtifactKind = 'deployment-tool'
        },
        @{
            SourcePath = Join-Path $RepoRoot 'docs\windows-native-install.md'
            RelativeDirectory = 'docs'
            ArtifactKind = 'deployment-doc'
        }
    )

    foreach ($supportFile in $supportFiles) {
        if (-not (Test-Path -LiteralPath $supportFile.SourcePath -PathType Leaf)) {
            continue
        }

        $publishedArtifacts.Add((
            Publish-RepoArtifact `
                -SourcePath $supportFile.SourcePath `
                -RelativeDirectory $supportFile.RelativeDirectory `
                -ArtifactKind $supportFile.ArtifactKind
        ))
    }

    return $publishedArtifacts
}

function New-DeploymentBundleArtifact {
    $bundleDirectory = Join-Path (Split-Path -Parent $script:ArtifactRoot) 'bundles'
    New-Item -ItemType Directory -Force -Path $bundleDirectory | Out-Null

    $buildClass = if ($NativeCpu) { 'host-optimized' } else { 'portable' }
    $version = (Get-NestedValue $script:VersionInfo @('SemVer'))
    $bundleName = "IronRDP-$($script:MachineIdentity)-$version-$buildClass.zip"

    [pscustomobject]@{
        kind = 'deployment-bundle'
        source = $script:ArtifactRoot
        destination = Join-Path $bundleDirectory $bundleName
    }
}

function Publish-DeploymentBundle {
    param(
        [Parameter(Mandatory)][string]$DestinationPath
    )

    if (Test-Path -LiteralPath $DestinationPath -PathType Leaf) {
        Remove-Item -LiteralPath $DestinationPath -Force
    }

    Compress-Archive -Path (Join-Path $script:ArtifactRoot '*') -DestinationPath $DestinationPath -Force
}

function New-InstallerOutputRoot {
    $installerRoot = Join-Path (Split-Path -Parent $script:ArtifactRoot) 'installers'
    New-Item -ItemType Directory -Force -Path $installerRoot | Out-Null

    $buildClass = if ($NativeCpu) { 'host-optimized' } else { 'portable' }
    $version = Get-NestedValue $script:VersionInfo @('SemVer')
    $outputName = "IronRDP-$($script:MachineIdentity)-$version-$buildClass"

    return Join-Path $installerRoot $outputName
}

function Publish-InstallerArtifacts {
    param(
        [Parameter(Mandatory)][string]$PackageRoot
    )

    $installerScript = Join-Path $RepoRoot 'scripts\windows\New-IronRdpInstallers.ps1'
    if (-not (Test-Path -LiteralPath $installerScript -PathType Leaf)) {
        throw "installer script not found: $installerScript"
    }

    $installerOutputRoot = New-InstallerOutputRoot
    if (Test-Path -LiteralPath $installerOutputRoot) {
        Remove-Item -LiteralPath $installerOutputRoot -Recurse -Force
    }

    $resolvedReleaseRepo = if ([string]::IsNullOrWhiteSpace($ReleaseRepo)) {
        if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_REPOSITORY)) { $env:GITHUB_REPOSITORY } else { $null }
    } else {
        $ReleaseRepo
    }
    $resolvedReleaseTag = if ([string]::IsNullOrWhiteSpace($ReleaseTag)) {
        if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_REF_NAME)) { $env:GITHUB_REF_NAME } else { $null }
    } else {
        $ReleaseTag
    }

    $arguments = @(
        '-NoLogo',
        '-NoProfile',
        '-ExecutionPolicy', 'Bypass',
        '-File', $installerScript,
        '-PackageRoot', $PackageRoot,
        '-OutputRoot', $installerOutputRoot,
        '-Publisher', $InstallerPublisher,
        '-OutputJsonPath', (Join-Path $installerOutputRoot 'installer-output.json')
    )

    if (-not [string]::IsNullOrWhiteSpace($InstallerCertificatePath)) {
        $arguments += @('-CertificatePath', $InstallerCertificatePath)
    }

    if (-not [string]::IsNullOrWhiteSpace($InstallerCertificatePassword)) {
        $arguments += @('-CertificatePassword', $InstallerCertificatePassword)
    }

    if (-not [string]::IsNullOrWhiteSpace($resolvedReleaseRepo)) {
        $arguments += @('-ReleaseRepo', $resolvedReleaseRepo)
    }

    if (-not [string]::IsNullOrWhiteSpace($resolvedReleaseTag)) {
        $arguments += @('-ReleaseTag', $resolvedReleaseTag)
    }

    if ($SkipMsix) {
        $arguments += '-SkipMsix'
    }

    if ($SkipMsi) {
        $arguments += '-SkipMsi'
    }

    & pwsh @arguments
    if ($LASTEXITCODE -ne 0) {
        throw 'installer generation failed'
    }

    $installerResultPath = Join-Path $installerOutputRoot 'installer-output.json'
    if (-not (Test-Path -LiteralPath $installerResultPath -PathType Leaf)) {
        throw "installer generation did not produce metadata: $installerResultPath"
    }

    $installerResult = Get-Content -LiteralPath $installerResultPath -Raw | ConvertFrom-Json
    return @(
        foreach ($artifact in $installerResult.artifacts) {
            [pscustomobject]@{
                kind = $artifact.kind
                source = $PackageRoot
                destination = $artifact.path
            }
        }
    )
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
        build = [ordered]@{
            mode = $Mode
            release = [bool]$Release
            nativeCpu = [bool]$NativeCpu
            class = if ($NativeCpu) { 'host-optimized' } else { 'portable' }
            windowsRuntime = if ([string]::IsNullOrWhiteSpace($env:IRONRDP_WINDOWS_RUNTIME)) { 'default' } else { $env:IRONRDP_WINDOWS_RUNTIME }
        }
        environment = [ordered]@{
            cargoTargetDir = $env:CARGO_TARGET_DIR
            sccacheDir = $env:SCCACHE_DIR
            sccacheCacheSize = $env:SCCACHE_CACHE_SIZE
            sccacheIdleTimeout = $env:SCCACHE_IDLE_TIMEOUT
            nugetPackages = $env:NUGET_PACKAGES
            cargoBuildJobs = $env:CARGO_BUILD_JOBS
            cargoUseLld = $env:CARGO_USE_LLD
            cargoUseNextest = $env:CARGO_USE_NEXTEST
            cargoLldPath = $env:CARGO_LLD_PATH
            cmakeGenerator = $env:CMAKE_GENERATOR
            include = $env:INCLUDE
            lib = $env:LIB
            llvmPath = $env:LLVM_PATH
            cudaPath = $env:CUDA_PATH
            oneApiRoot = $env:ONEAPI_ROOT
            ironrdpDeploymentName = $env:IRONRDP_DEPLOYMENT_NAME
            ironrdpMachineIdentity = $env:IRONRDP_MACHINE_IDENTITY
            ironrdpTargetMachine = $env:IRONRDP_TARGET_MACHINE
            ironrdpArtifactRoot = $env:IRONRDP_ARTIFACT_ROOT
            ironrdpBuildClass = $env:IRONRDP_BUILD_CLASS
            ironrdpPrimaryNetworkGbps = $env:IRONRDP_PRIMARY_NETWORK_GBPS
            ironrdpWindowsRuntime = $env:IRONRDP_WINDOWS_RUNTIME
            ironrdpReleaseRepo = $ReleaseRepo
            ironrdpReleaseTag = $ReleaseTag
        }
        modules = $script:ImportedModules
        cargoMachineConfig = $script:CargoMachineConfig
        profileMachineConfig = $script:ProfileMachineConfig
        profileConfig = [ordered]@{
            module = Get-NestedValue $script:ProfileConfig @('module')
            defaults = Get-NestedValue $script:ProfileConfig @('defaults')
            paths = Get-NestedValue $script:ProfileConfig @('paths')
        }
        toolchains = $script:ToolchainInfo
        hardware = $script:HardwareProfile
        artifacts = $Artifacts
    }

    $manifestPath = Join-Path $script:ArtifactRoot 'build-manifest.json'
    $manifest | ConvertTo-Json -Depth 10 | Set-Content -Path $manifestPath -Encoding utf8
    Write-Host "Build manifest: $manifestPath" -ForegroundColor Cyan
}

Push-Location $RepoRoot
try {
    Import-Module CargoTools -ErrorAction Stop
    Register-ImportedModule -Name 'CargoTools'
    $null = Import-OptionalModule -Name 'ProfileUtilities'
    Register-ImportedModule -Name 'ProfileUtilities'
    $null = Import-OptionalModule -Name 'MachineConfiguration'
    Register-ImportedModule -Name 'MachineConfiguration'

    if ($BootstrapTools) {
        Install-RustTool -ExecutableName 'cargo-nextest' -PackageName 'cargo-nextest'
        Install-RustTool -ExecutableName 'cargo-llvm-cov' -PackageName 'cargo-llvm-cov'
        Install-ChocoTool -ExecutableName 'ninja' -PackageName 'ninja'
        Install-ChocoTool -ExecutableName 'nasm' -PackageName 'nasm'
        Ensure-DiplomatTool
    }

    Initialize-PreferredToolchainContext
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
        Remove-Item Env:CARGO_BUILD_RUSTC_WRAPPER -ErrorAction SilentlyContinue
        Remove-Item Env:RUSTC_WORKSPACE_WRAPPER -ErrorAction SilentlyContinue
        $env:SCCACHE_DISABLE = '1'
    }

    if ($NativeCpu -and -not $script:UseCargoWrapper) {
        Add-RustFlag -Flags @('-C', 'target-cpu=native')
    }

    $portableWindowsRuntimeModes = @('package', 'publish')
    if ($portableWindowsRuntimeModes -contains $Mode) {
        Add-RustFlag -Flags @('-C', 'target-feature=+crt-static')
        $env:IRONRDP_WINDOWS_RUNTIME = 'static-msvc-crt'
    }

    Write-Host "Mode: $Mode" -ForegroundColor Cyan
    Write-Host "Machine identity: $script:MachineIdentity" -ForegroundColor Cyan
    Write-Host "Artifact root: $script:ArtifactRoot" -ForegroundColor Cyan
    Write-Host "Jobs: $env:CARGO_BUILD_JOBS" -ForegroundColor Cyan
    Write-Host "CargoTools wrapper: $script:UseCargoWrapper" -ForegroundColor Cyan
    Write-Host "RUSTC_WRAPPER: $($env:RUSTC_WRAPPER)" -ForegroundColor Cyan
    Write-Host "CARGO_BUILD_RUSTC_WRAPPER: $($env:CARGO_BUILD_RUSTC_WRAPPER)" -ForegroundColor Cyan
    Write-Host "CARGO_USE_LLD: $($env:CARGO_USE_LLD)" -ForegroundColor Cyan
    Write-Host "CMAKE_GENERATOR: $($env:CMAKE_GENERATOR)" -ForegroundColor Cyan
    Write-Host "CARGO_TARGET_DIR: $($env:CARGO_TARGET_DIR)" -ForegroundColor Cyan
    Write-Host "SCCACHE_DIR: $($env:SCCACHE_DIR)" -ForegroundColor Cyan
    Write-Host "NUGET_PACKAGES: $($env:NUGET_PACKAGES)" -ForegroundColor Cyan
    Write-Host "LLVM: $(Get-NestedValue $script:ToolchainInfo @('llvm', 'root'))" -ForegroundColor Cyan
    Write-Host "LLD: $(Get-NestedValue $script:ToolchainInfo @('llvm', 'lldLink'))" -ForegroundColor Cyan
    Write-Host "MSVC: $(Get-NestedValue $script:ToolchainInfo @('visualStudio', 'installationPath')) [$((Get-NestedValue $script:ToolchainInfo @('visualStudio', 'msvcVersion')))]" -ForegroundColor Cyan
    Write-Host "Windows SDK: $(Get-NestedValue $script:ToolchainInfo @('windowsSdk', 'version'))" -ForegroundColor Cyan
    Write-Host "oneAPI: $(Get-NestedValue $script:ToolchainInfo @('oneApi', 'root'))" -ForegroundColor Cyan
    Write-Host "CUDA: $(Get-NestedValue $script:ToolchainInfo @('cuda', 'root'))" -ForegroundColor Cyan
    Write-Host "Build class: $(Get-NestedValue $script:HardwareProfile @('buildClass'))" -ForegroundColor Cyan
    Write-Host "Windows runtime: $(if ($env:IRONRDP_WINDOWS_RUNTIME) { $env:IRONRDP_WINDOWS_RUNTIME } else { 'default' })" -ForegroundColor Cyan
    Write-Host "Primary NIC: $(Get-NestedValue $script:HardwareProfile @('primaryNetworkAdapter', 'name')) @ $((Get-NestedValue $script:HardwareProfile @('primaryNetworkAdapter', 'linkSpeed')))" -ForegroundColor Cyan
    Write-Host "GPU vendors: $((@((Get-NestedValue $script:HardwareProfile @('gpus')) | ForEach-Object { $_.vendor }) | Where-Object { $_ } | Select-Object -Unique) -join ', ')" -ForegroundColor Cyan
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
        'doctor' {
            Write-BuildManifest
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
            $artifacts = New-Object System.Collections.Generic.List[object]
            foreach ($artifact in (Publish-BuildOutputs)) { $artifacts.Add($artifact) }
            foreach ($artifact in (Publish-DeploymentSupportFiles)) { $artifacts.Add($artifact) }
            $bundleArtifact = New-DeploymentBundleArtifact
            $artifacts.Add($bundleArtifact)
            Write-BuildManifest -Artifacts $artifacts
            foreach ($artifact in (Publish-InstallerArtifacts -PackageRoot $script:ArtifactRoot)) { $artifacts.Add($artifact) }
            Write-BuildManifest -Artifacts $artifacts
            Publish-DeploymentBundle -DestinationPath $bundleArtifact.destination
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
            $artifacts = New-Object System.Collections.Generic.List[object]
            foreach ($artifact in (Publish-BuildOutputs)) { $artifacts.Add($artifact) }
            foreach ($artifact in (Publish-DeploymentSupportFiles)) { $artifacts.Add($artifact) }
            $bundleArtifact = New-DeploymentBundleArtifact
            $artifacts.Add($bundleArtifact)
            Write-BuildManifest -Artifacts $artifacts
            foreach ($artifact in (Publish-InstallerArtifacts -PackageRoot $script:ArtifactRoot)) { $artifacts.Add($artifact) }
            Write-BuildManifest -Artifacts $artifacts
            Publish-DeploymentBundle -DestinationPath $bundleArtifact.destination
        }
    }
} finally {
    Pop-Location
}
