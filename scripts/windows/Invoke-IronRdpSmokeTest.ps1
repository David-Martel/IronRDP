[CmdletBinding(DefaultParameterSetName = 'Package')]
param(
    [Parameter(ParameterSetName = 'Package')]
    [string]$PackageRoot = (Split-Path -Parent $PSScriptRoot),

    [Parameter(ParameterSetName = 'Install', Mandatory = $true)]
    [string]$InstallRoot,

    [Parameter(ParameterSetName = 'Msix', Mandatory = $true)]
    [string]$MsixPackageName,

    [string]$LaunchHost,
    [string]$Username,
    [string]$Password,
    [int]$ConnectSeconds = 20,
    [string]$ConnectionLogPath,
    [ValidateSet('off', 'prefer-reliable', 'reliable', 'prefer-lossy', 'lossy')]
    [string]$Multitransport = 'off',
    [int]$Width = 1280,
    [int]$Height = 720
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Resolve-PackageRoot {
    param([string]$ParameterSetName)

    switch ($ParameterSetName) {
        'Install' {
            return (Resolve-Path -LiteralPath $InstallRoot).Path
        }
        'Package' {
            return (Resolve-Path -LiteralPath $PackageRoot).Path
        }
        'Msix' {
            $package = Get-AppxPackage -Name $MsixPackageName | Sort-Object Version -Descending | Select-Object -First 1
            if (-not $package) {
                throw "installed MSIX package not found: $MsixPackageName"
            }

            $root = Join-Path $package.InstallLocation 'VFS\ProgramFilesX64\IronRDP'
            if ([string]::IsNullOrWhiteSpace($root)) {
                throw "installed MSIX package does not expose an install location: $MsixPackageName"
            }

            return [pscustomobject]@{
                Root = $root
                Package = $package
            }
        }
        default {
            throw "unsupported parameter set: $ParameterSetName"
        }
    }
}

function Get-MetricSummary {
    param(
        [Parameter(Mandatory)][string]$Text,
        [Parameter(Mandatory)][string]$Name
    )

    $matches = [regex]::Matches($Text, "$Name=(\d+)")
    if ($matches.Count -eq 0) {
        return $null
    }

    $values = foreach ($match in $matches) {
        [int64]::Parse($match.Groups[1].Value, [System.Globalization.CultureInfo]::InvariantCulture)
    }

    $measure = $values | Measure-Object -Average -Maximum -Minimum

    [pscustomobject]@{
        count = $values.Count
        average = [math]::Round([double]$measure.Average, 2)
        minimum = [int64]$measure.Minimum
        maximum = [int64]$measure.Maximum
    }
}

function Get-LiveConnectSummary {
    param(
        [Parameter(Mandatory)][string]$ClientExe,
        [Parameter(Mandatory)][string]$Destination,
        [string]$Username,
        [string]$Password,
        [int]$ConnectSeconds,
        [string]$ConnectionLogPath,
        [string]$Multitransport,
        [int]$Width,
        [int]$Height
    )

    $logPath = if ($ConnectionLogPath) {
        $ConnectionLogPath
    } else {
        Join-Path ([System.IO.Path]::GetTempPath()) ("ironrdp-live-connect-{0:yyyyMMdd-HHmmss}.log" -f (Get-Date))
    }

    $logDirectory = Split-Path -Parent $logPath
    if ($logDirectory) {
        New-Item -ItemType Directory -Force -Path $logDirectory | Out-Null
    }

    if (Test-Path -LiteralPath $logPath) {
        Remove-Item -LiteralPath $logPath -Force
    }

    $arguments = [System.Collections.Generic.List[string]]::new()
    $arguments.Add($Destination)

    if ($Username) {
        $arguments.Add('--username')
        $arguments.Add($Username)
    }

    if ($Password) {
        $arguments.Add('--password')
        $arguments.Add($Password)
    }

    $arguments.Add('--width')
    $arguments.Add($Width.ToString([System.Globalization.CultureInfo]::InvariantCulture))
    $arguments.Add('--height')
    $arguments.Add($Height.ToString([System.Globalization.CultureInfo]::InvariantCulture))
    $arguments.Add('--clipboard-type')
    $arguments.Add('none')
    $arguments.Add('--no-server-pointer')
    $arguments.Add('--log-file')
    $arguments.Add($logPath)

    if ($Multitransport -ne 'off') {
        $arguments.Add('--multitransport')
        $arguments.Add($Multitransport)
    }

    $startInfo = @{
        FilePath = $ClientExe
        ArgumentList = $arguments
        WorkingDirectory = Split-Path -Parent $ClientExe
        PassThru = $true
        WindowStyle = 'Minimized'
        Environment = @{
            IRONRDP_LOG = 'info,ironrdp_client=trace,ironrdp_connector=debug,ironrdp_session=debug'
        }
    }

    $process = Start-Process @startInfo
    $startedAt = Get-Date
    $process | Wait-Process -Timeout $ConnectSeconds -ErrorAction SilentlyContinue
    $timedOut = -not $process.HasExited
    $closedGracefully = $false

    if ($timedOut) {
        try {
            $closedGracefully = $process.CloseMainWindow()
        } catch {
            $closedGracefully = $false
        }

        Start-Sleep -Seconds 5
        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id -Force
            $process | Wait-Process -Timeout 5 -ErrorAction SilentlyContinue
        }
    }

    $logText = if (Test-Path -LiteralPath $logPath) {
        Get-Content -LiteralPath $logPath -Raw
    } else {
        ''
    }

    $logTail = if (Test-Path -LiteralPath $logPath) {
        Get-Content -LiteralPath $logPath -Tail 120 | Out-String
    } else {
        $null
    }

    $counts = [ordered]@{
        connectionStart = [regex]::Matches($logText, 'Begin connection procedure').Count
        connectionEstablished = [regex]::Matches($logText, 'Connection established').Count
        firstImageUpdate = [regex]::Matches($logText, 'First image update emitted').Count
        imageUpdates = [regex]::Matches($logText, 'Emitted image update').Count
        firstPresentedFrame = [regex]::Matches($logText, 'First frame presented to the window').Count
        presentedFrames = [regex]::Matches($logText, 'Presented frame').Count
        reconnects = [regex]::Matches($logText, 'Restarting session with updated desktop size').Count
        multitransportAbort = [regex]::Matches($logText, 'Multitransport request received').Count
        connectionErrors = [regex]::Matches($logText, 'Connection error').Count
        activeSessionErrors = [regex]::Matches($logText, 'Active session error').Count
        surfaceFailures = [regex]::Matches($logText, 'Failed to present surface buffer').Count
    }

    $status = if ($counts.presentedFrames -gt 0 -or $counts.firstPresentedFrame -gt 0) {
        'session-rendering'
    } elseif ($counts.imageUpdates -gt 0 -or $counts.firstImageUpdate -gt 0) {
        'session-active'
    } elseif ($counts.connectionEstablished -gt 0) {
        'connected-no-frame'
    } elseif ($counts.connectionStart -gt 0) {
        'connection-started'
    } else {
        'launch-only'
    }

    [pscustomobject]@{
        destination = $Destination
        username = $Username
        timedOut = $timedOut
        closeRequested = $closedGracefully
        exitCode = if ($process.HasExited) { $process.ExitCode } else { $null }
        startedAt = $startedAt
        durationSeconds = [math]::Round(((Get-Date) - $startedAt).TotalSeconds, 2)
        status = $status
        logPath = $logPath
        logCounts = [pscustomobject]$counts
        copyMicros = Get-MetricSummary -Text $logText -Name 'copy_micros'
        convertMicros = Get-MetricSummary -Text $logText -Name 'convert_micros'
        presentMicros = Get-MetricSummary -Text $logText -Name 'present_micros'
        backendTotalMicros = Get-MetricSummary -Text $logText -Name 'backend_total_micros'
        logTail = $logTail
    }
}

$rootResult = Resolve-PackageRoot -ParameterSetName $PSCmdlet.ParameterSetName
if ($rootResult -is [string]) {
    $root = $rootResult
    $package = $null
} else {
    $root = $rootResult.Root
    $package = $rootResult.Package
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

$connection = $null
if ($LaunchHost -and $Password) {
    $connection = Get-LiveConnectSummary `
        -ClientExe $clientExe `
        -Destination $LaunchHost `
        -Username $Username `
        -Password $Password `
        -ConnectSeconds $ConnectSeconds `
        -ConnectionLogPath $ConnectionLogPath `
        -Multitransport $Multitransport `
        -Width $Width `
        -Height $Height
} elseif ($LaunchHost) {
    $arguments = @($LaunchHost)
    if ($Username) {
        $arguments += @('--username', $Username)
    }

    Write-Host "Launching IronRDP client against $LaunchHost" -ForegroundColor Yellow
    Start-Process -FilePath $clientExe -ArgumentList $arguments -WorkingDirectory (Split-Path -Parent $clientExe)
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
    packageFamilyName = if ($PSCmdlet.ParameterSetName -eq 'Msix') { $package.PackageFamilyName } else { $null }
    connection = $connection
}

$result | Format-List | Out-String | Write-Host
$result
