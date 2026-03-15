[CmdletBinding(DefaultParameterSetName = 'Package')]
param(
    [Parameter(ParameterSetName = 'Package')]
    [string]$PackageRoot = (Split-Path -Parent $PSScriptRoot),

    [Parameter(ParameterSetName = 'Install', Mandatory = $true)]
    [string]$InstallRoot,

    [Parameter(ParameterSetName = 'Msix', Mandatory = $true)]
    [string]$MsixPackageName,

    [string]$VmName = 'WS2025-ReFS-Repair',
    [string]$Username = 'IronRdpLab',
    [string]$Password = 'TempIronRdp!2026',
    [ValidateSet('quick', 'full')]
    [string]$ScenarioSet = 'quick',
    [int]$DurationSeconds = 30,
    [int]$SampleIntervalMs = 500,
    [ValidateSet('off', 'prefer-reliable', 'reliable', 'prefer-lossy', 'lossy')]
    [string]$Multitransport = 'off',
    [string]$GuestWorkload = 'notepad',
    [int]$Width = 1280,
    [int]$Height = 720,
    [string]$OutputRoot
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

function Resolve-ReachableGuestEndpoint {
    param([Parameter(Mandatory)][string]$VmName)

    $vm = Get-VM -Name $VmName -ErrorAction Stop
    $candidateIps = (Get-VMNetworkAdapter -VMName $VmName).IPAddresses |
        Where-Object { $_ -match '^\d+\.\d+\.\d+\.\d+$' -and $_ -ne '127.0.0.1' }

    if (-not $candidateIps) {
        throw "no IPv4 addresses were reported for Hyper-V guest '$VmName'"
    }

    $reachable = foreach ($ip in $candidateIps) {
        $probe = Test-NetConnection -ComputerName $ip -Port 3389 -InformationLevel Detailed -WarningAction SilentlyContinue
        [pscustomobject]@{
            ipAddress = $ip
            tcpTestSucceeded = [bool]$probe.TcpTestSucceeded
            remoteAddress = $probe.RemoteAddress
            interfaceAlias = $probe.InterfaceAlias
        }
    }

    $selected = $reachable | Where-Object tcpTestSucceeded | Select-Object -First 1
    if (-not $selected) {
        throw "no reachable RDP endpoint found for Hyper-V guest '$VmName'"
    }

    [pscustomobject]@{
        vm = $vm
        selected = $selected
        reachable = $reachable
    }
}

function New-GuestCredential {
    param(
        [Parameter(Mandatory)][string]$Username,
        [Parameter(Mandatory)][string]$Password
    )

    New-Object System.Management.Automation.PSCredential(".\$Username", (ConvertTo-SecureString $Password -AsPlainText -Force))
}

function Ensure-WindowInterop {
    if ('IronRdpWindowInterop' -as [type]) {
        return
    }

    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class IronRdpWindowInterop {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool MoveWindow(IntPtr hWnd, int x, int y, int nWidth, int nHeight, bool bRepaint);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetCursorPos(int X, int Y);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint dwData, UIntPtr dwExtraInfo);
}
"@
}

function Get-WindowRectObject {
    param([Parameter(Mandatory)][IntPtr]$Handle)

    Ensure-WindowInterop
    $rect = New-Object IronRdpWindowInterop+RECT
    if (-not [IronRdpWindowInterop]::GetWindowRect($Handle, [ref]$rect)) {
        throw "failed to query window rectangle for handle $Handle"
    }

    [pscustomobject]@{
        Left = $rect.Left
        Top = $rect.Top
        Right = $rect.Right
        Bottom = $rect.Bottom
        Width = $rect.Right - $rect.Left
        Height = $rect.Bottom - $rect.Top
    }
}

function Wait-ForMainWindow {
    param(
        [Parameter(Mandatory)][int]$ProcessId,
        [int]$TimeoutSeconds = 15
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
        if (-not $proc) {
            return $null
        }

        $proc.Refresh()
        if ($proc.MainWindowHandle -ne 0) {
            return $proc
        }

        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)

    return $proc
}

function Resize-ClientWindow {
    param(
        [Parameter(Mandatory)][System.Diagnostics.Process]$Process,
        [Parameter(Mandatory)][int]$Width,
        [Parameter(Mandatory)][int]$Height
    )

    Ensure-WindowInterop
    $process = Wait-ForMainWindow -ProcessId $Process.Id -TimeoutSeconds 5
    if (-not $process -or $process.MainWindowHandle -eq 0) {
        throw "client window handle not available for resize"
    }

    $rect = Get-WindowRectObject -Handle $process.MainWindowHandle
    $null = [IronRdpWindowInterop]::SetForegroundWindow($process.MainWindowHandle)
    if (-not [IronRdpWindowInterop]::MoveWindow($process.MainWindowHandle, $rect.Left, $rect.Top, $Width, $Height, $true)) {
        throw "failed to resize client window"
    }

    [pscustomobject]@{
        width = $Width
        height = $Height
        left = $rect.Left
        top = $rect.Top
    }
}

function Invoke-ClientMouseTrace {
    param(
        [Parameter(Mandatory)][System.Diagnostics.Process]$Process
    )

    Ensure-WindowInterop
    $process = Wait-ForMainWindow -ProcessId $Process.Id -TimeoutSeconds 5
    if (-not $process -or $process.MainWindowHandle -eq 0) {
        throw "client window handle not available for mouse automation"
    }

    $rect = Get-WindowRectObject -Handle $process.MainWindowHandle
    $points = @(
        @{ x = $rect.Left + [math]::Max([int]($rect.Width * 0.20), 20); y = $rect.Top + [math]::Max([int]($rect.Height * 0.20), 20) },
        @{ x = $rect.Left + [math]::Max([int]($rect.Width * 0.80), 20); y = $rect.Top + [math]::Max([int]($rect.Height * 0.20), 20) },
        @{ x = $rect.Left + [math]::Max([int]($rect.Width * 0.80), 20); y = $rect.Top + [math]::Max([int]($rect.Height * 0.80), 20) },
        @{ x = $rect.Left + [math]::Max([int]($rect.Width * 0.20), 20); y = $rect.Top + [math]::Max([int]($rect.Height * 0.80), 20) },
        @{ x = $rect.Left + [math]::Max([int]($rect.Width * 0.50), 20); y = $rect.Top + [math]::Max([int]($rect.Height * 0.50), 20) }
    )

    $null = [IronRdpWindowInterop]::SetForegroundWindow($process.MainWindowHandle)
    foreach ($point in $points) {
        [IronRdpWindowInterop]::SetCursorPos($point.x, $point.y) | Out-Null
        Start-Sleep -Milliseconds 180
    }

    $center = $points[-1]
    [IronRdpWindowInterop]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [IronRdpWindowInterop]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)

    [pscustomobject]@{
        points = $points
        clickPoint = $center
    }
}

function Capture-WindowScreenshot {
    param(
        [Parameter(Mandatory)][System.Diagnostics.Process]$Process,
        [Parameter(Mandatory)][string]$Path
    )

    $process = Wait-ForMainWindow -ProcessId $Process.Id -TimeoutSeconds 5
    if (-not $process -or $process.MainWindowHandle -eq 0) {
        return $null
    }

    Ensure-WindowInterop
    Add-Type -AssemblyName System.Drawing
    $rect = Get-WindowRectObject -Handle $process.MainWindowHandle
    $bitmap = New-Object System.Drawing.Bitmap $rect.Width, $rect.Height
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
        } finally {
            $graphics.Dispose()
        }

        $directory = Split-Path -Parent $Path
        if ($directory) {
            New-Item -ItemType Directory -Force -Path $directory | Out-Null
        }
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
        return $Path
    } finally {
        $bitmap.Dispose()
    }
}

function Invoke-GuestWorkload {
    param(
        [Parameter(Mandatory)][string]$VmName,
        [Parameter(Mandatory)][System.Management.Automation.PSCredential]$Credential,
        [Parameter(Mandatory)][string]$Workload
    )

    if ([string]::IsNullOrWhiteSpace($Workload) -or $Workload -eq 'none') {
        return $null
    }

    $plainPassword = $Credential.GetNetworkCredential().Password

    Invoke-Command -VMName $VmName -Credential $Credential -ScriptBlock {
        param($Workload, $UserName, $Password)

        $startMap = @{
            notepad = @{ filePath = 'notepad.exe'; processName = 'notepad'; arguments = $null }
            calc = @{ filePath = 'calc.exe'; processName = 'CalculatorApp'; arguments = $null }
            powershell = @{ filePath = 'powershell.exe'; processName = 'powershell'; arguments = '-NoLogo' }
            mspaint = @{ filePath = 'mspaint.exe'; processName = 'mspaint'; arguments = $null }
        }

        $workloadSpec = $startMap[$Workload]
        if (-not $workloadSpec) {
            throw "unsupported guest workload: $Workload"
        }

        $taskName = "IronRdp-E2E-$([Guid]::NewGuid().ToString('N'))"
        $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
        $process = $null
        $launchMode = 'scheduled-task'
        $scheduledTaskError = $null

        try {
            try {
                $action = if ([string]::IsNullOrWhiteSpace($workloadSpec.arguments)) {
                    New-ScheduledTaskAction -Execute $workloadSpec.filePath
                } else {
                    New-ScheduledTaskAction -Execute $workloadSpec.filePath -Argument $workloadSpec.arguments
                }

                $normalizedUserName = $UserName -replace '^[.\\]+', ''
                $principal = New-ScheduledTaskPrincipal -UserId $normalizedUserName -LogonType InteractiveOrPassword -RunLevel Highest
                $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable
                $task = New-ScheduledTask -Action $action -Principal $principal -Settings $settings
                Register-ScheduledTask -TaskName $taskName -InputObject $task -Password $Password -Force -ErrorAction Stop | Out-Null
                Start-ScheduledTask -TaskName $taskName -ErrorAction Stop
            } catch {
                $launchMode = 'start-process-fallback'
                $scheduledTaskError = ($_ | Out-String).Trim()
                $process = if ([string]::IsNullOrWhiteSpace($workloadSpec.arguments)) {
                    Start-Process -FilePath $workloadSpec.filePath -PassThru
                } else {
                    Start-Process -FilePath $workloadSpec.filePath -ArgumentList $workloadSpec.arguments -PassThru
                }
            }

            do {
                Start-Sleep -Milliseconds 200
                if (-not $process -or $launchMode -eq 'scheduled-task') {
                    $process = Get-Process -Name $workloadSpec.processName -ErrorAction SilentlyContinue |
                        Sort-Object StartTime -Descending |
                        Select-Object -First 1
                }

                if ($process) {
                    $process.Refresh()
                }
            } while ($stopwatch.Elapsed.TotalSeconds -lt 12 -and (-not $process -or $process.MainWindowHandle -eq 0))

            [pscustomobject]@{
                workload = $Workload
                launchMs = [math]::Round($stopwatch.Elapsed.TotalMilliseconds, 2)
                pid = if ($process) { $process.Id } else { $null }
                mainWindowHandle = if ($process) { $process.MainWindowHandle } else { $null }
                sessionId = if ($process) { $process.SessionId } else { $null }
                processName = if ($process) { $process.ProcessName } else { $null }
                launchMode = $launchMode
                scheduledTaskError = $scheduledTaskError
                started = $null -ne $process
                interactiveWindow = $process -and $process.MainWindowHandle -ne 0
            }
        } finally {
            Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue | Out-Null
        }
    } -ArgumentList $Workload, $Credential.UserName, $plainPassword -ErrorAction Stop
}

function New-RdpBlockRule {
    param(
        [Parameter(Mandatory)][string]$RemoteAddress
    )

    $ruleName = "IronRDP HyperV Outage $([Guid]::NewGuid().ToString('N'))"
    New-NetFirewallRule -DisplayName $ruleName -Direction Outbound -Action Block -Protocol TCP -RemoteAddress $RemoteAddress -RemotePort 3389 | Out-Null
    $ruleName
}

function Remove-RdpBlockRule {
    param([string]$RuleName)

    if ([string]::IsNullOrWhiteSpace($RuleName)) {
        return
    }

    Remove-NetFirewallRule -DisplayName $RuleName -ErrorAction SilentlyContinue | Out-Null
}

function Get-Percentile {
    param(
        [Parameter(Mandatory)][double[]]$Values,
        [Parameter(Mandatory)][double]$Percentile
    )

    $Values = @($Values)
    if ($Values.Count -eq 0) {
        return $null
    }

    $sorted = $Values | Sort-Object
    $index = [math]::Ceiling(($Percentile / 100.0) * $sorted.Count) - 1
    $index = [math]::Max([math]::Min($index, $sorted.Count - 1), 0)
    [math]::Round([double]$sorted[$index], 2)
}

function Get-NumericSummary {
    param([double[]]$Values)

    $Values = @($Values)
    if (-not $Values -or $Values.Count -eq 0) {
        return $null
    }

    $measure = $Values | Measure-Object -Average -Maximum -Minimum
    [pscustomobject]@{
        count = $Values.Count
        average = [math]::Round([double]$measure.Average, 2)
        minimum = [math]::Round([double]$measure.Minimum, 2)
        maximum = [math]::Round([double]$measure.Maximum, 2)
        p95 = Get-Percentile -Values $Values -Percentile 95
    }
}

function Parse-IronRdpLog {
    param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "log file not found: $Path"
    }

    $logText = Get-Content -LiteralPath $Path -Raw
    $lineRegex = '^(?<timestamp>\S+)\s+(?<level>[A-Z]+)\s+(?<target>.+?):\s+(?<message>.*)$'
    $entries = New-Object System.Collections.Generic.List[object]
    foreach ($line in Get-Content -LiteralPath $Path) {
        $match = [regex]::Match($line, $lineRegex)
        if (-not $match.Success) {
            continue
        }

        $entries.Add([pscustomobject]@{
            timestamp = [DateTimeOffset]::Parse($match.Groups['timestamp'].Value, [System.Globalization.CultureInfo]::InvariantCulture)
            level = $match.Groups['level'].Value
            target = $match.Groups['target'].Value
            message = $match.Groups['message'].Value.Trim()
        })
    }

    $presented = @($entries | Where-Object { $_.message -like 'Presented frame*' })
    $emitted = @($entries | Where-Object { $_.message -like 'Emitted image update*' })
    $firstConnection = @($entries | Where-Object { $_.message -like '*Begin connection procedure*' }) | Select-Object -First 1
    $connected = @($entries | Where-Object { $_.message -like 'Connection established*' }) | Select-Object -First 1
    $firstImage = @($entries | Where-Object { $_.message -like 'First image update emitted*' }) | Select-Object -First 1
    $firstFrame = @($entries | Where-Object { $_.message -like 'First frame presented to the window*' }) | Select-Object -First 1

    $presentIntervals = @()
    for ($i = 1; $i -lt $presented.Count; $i++) {
        $presentIntervals += ($presented[$i].timestamp - $presented[$i - 1].timestamp).TotalMilliseconds
    }

    $imageIntervals = @()
    for ($i = 1; $i -lt $emitted.Count; $i++) {
        $imageIntervals += ($emitted[$i].timestamp - $emitted[$i - 1].timestamp).TotalMilliseconds
    }

    $fps = $null
    if ($presented.Count -gt 1) {
        $timespan = ($presented[-1].timestamp - $presented[0].timestamp).TotalSeconds
        if ($timespan -gt 0) {
            $fps = [math]::Round(($presented.Count - 1) / $timespan, 2)
        }
    }

    $copyValues = [regex]::Matches($logText, 'copy_micros=(\d+)') | ForEach-Object { [double]$_.Groups[1].Value }
    $convertValues = [regex]::Matches($logText, 'convert_micros=(\d+)') | ForEach-Object { [double]$_.Groups[1].Value }
    $presentValues = [regex]::Matches($logText, 'present_micros=(\d+)') | ForEach-Object { [double]$_.Groups[1].Value }
    $backendValues = [regex]::Matches($logText, 'backend_total_micros=(\d+)') | ForEach-Object { [double]$_.Groups[1].Value }
    $compressionRatios = [regex]::Matches($logText, 'compression_ratio=([0-9.]+)x') | ForEach-Object { [double]$_.Groups[1].Value }
    $bppMatches = [regex]::Matches($logText, 'RLE_BITMAP_STREAM bpp=(\d+)')
    $compressionTypeMatches = [regex]::Matches($logText, 'compression_type=Some\(([^)]+)\)')

    $bppSummary = @{}
    foreach ($match in $bppMatches) {
        $key = $match.Groups[1].Value
        if (-not $bppSummary.ContainsKey($key)) { $bppSummary[$key] = 0 }
        $bppSummary[$key]++
    }

    $compressionTypeSummary = @{}
    foreach ($match in $compressionTypeMatches) {
        $key = $match.Groups[1].Value
        if (-not $compressionTypeSummary.ContainsKey($key)) { $compressionTypeSummary[$key] = 0 }
        $compressionTypeSummary[$key]++
    }

    $counts = [ordered]@{
        connectionStart = @($entries | Where-Object { $_.message -like '*Begin connection procedure*' }).Count
        connectionEstablished = @($entries | Where-Object { $_.message -like 'Connection established*' }).Count
        firstImageUpdate = @($entries | Where-Object { $_.message -like 'First image update emitted*' }).Count
        imageUpdates = $emitted.Count
        firstPresentedFrame = @($entries | Where-Object { $_.message -like 'First frame presented to the window*' }).Count
        presentedFrames = $presented.Count
        reconnects = @($entries | Where-Object { $_.message -eq 'Restarting session with updated desktop size' }).Count
        multitransportAbort = @($entries | Where-Object { $_.message -eq 'Multitransport request received (UDP transport not implemented)' }).Count
        connectionErrors = @($entries | Where-Object { $_.message -like 'Connection error*' }).Count
        activeSessionErrors = @($entries | Where-Object { $_.message -like 'Active session error*' }).Count
        shutdownDenied = @($entries | Where-Object { $_.message -eq 'ShutdownDenied received, session will be closed' }).Count
        surfaceFailures = @($entries | Where-Object { $_.message -eq 'Failed to present surface buffer' }).Count
        overwrittenFrames = @($entries | Where-Object { $_.message -like 'Overwriting unpresented frame buffer*' }).Count
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
        status = $status
        lineCount = $entries.Count
        frameCount = $presented.Count
        imageUpdateCount = $emitted.Count
        fps = $fps
        presentIntervalMs = Get-NumericSummary -Values $presentIntervals
        imageUpdateIntervalMs = Get-NumericSummary -Values $imageIntervals
        copyMicros = Get-NumericSummary -Values $copyValues
        convertMicros = Get-NumericSummary -Values $convertValues
        presentMicros = Get-NumericSummary -Values $presentValues
        backendTotalMicros = Get-NumericSummary -Values $backendValues
        compressionRatio = Get-NumericSummary -Values $compressionRatios
        bitmapBpp = [pscustomobject]$bppSummary
        compressionTypes = [pscustomobject]$compressionTypeSummary
        latencyMs = [pscustomobject]@{
            connectToEstablished = if ($firstConnection -and $connected) { [math]::Round(($connected.timestamp - $firstConnection.timestamp).TotalMilliseconds, 2) } else { $null }
            connectToFirstImage = if ($firstConnection -and $firstImage) { [math]::Round(($firstImage.timestamp - $firstConnection.timestamp).TotalMilliseconds, 2) } else { $null }
            connectToFirstFrame = if ($firstConnection -and $firstFrame) { [math]::Round(($firstFrame.timestamp - $firstConnection.timestamp).TotalMilliseconds, 2) } else { $null }
        }
        counts = [pscustomobject]$counts
        tail = Get-Content -LiteralPath $Path -Tail 120 | Out-String
    }
}

function Summarize-ProcessSamples {
    param([object[]]$Samples)

    if (-not $Samples -or $Samples.Count -eq 0) {
        return $null
    }

    [pscustomobject]@{
        sampleCount = $Samples.Count
        cpuPercent = Get-NumericSummary -Values ($Samples | Where-Object { $null -ne $_.cpuPercent } | ForEach-Object { [double]$_.cpuPercent })
        workingSetMb = Get-NumericSummary -Values ($Samples | ForEach-Object { [double]$_.workingSetMb })
        privateMemoryMb = Get-NumericSummary -Values ($Samples | ForEach-Object { [double]$_.privateMemoryMb })
        handles = Get-NumericSummary -Values ($Samples | ForEach-Object { [double]$_.handles })
        threads = Get-NumericSummary -Values ($Samples | ForEach-Object { [double]$_.threads })
    }
}

function Get-ScenarioDefinitions {
    param([string]$ScenarioSet)

    $baseline = [pscustomobject]@{
        name = 'baseline'
        resizeActions = @()
        outageStartSeconds = $null
        outageDurationSeconds = $null
        mouseMoveAtSeconds = 8
        guestWorkloadAtSeconds = 4
    }

    $resize = [pscustomobject]@{
        name = 'resize'
        resizeActions = @(
            [pscustomobject]@{ atSeconds = 8; width = 1600; height = 900 },
            [pscustomobject]@{ atSeconds = 15; width = 1280; height = 720 }
        )
        outageStartSeconds = $null
        outageDurationSeconds = $null
        mouseMoveAtSeconds = 9
        guestWorkloadAtSeconds = 4
    }

    $outage = [pscustomobject]@{
        name = 'outage'
        resizeActions = @()
        outageStartSeconds = 10
        outageDurationSeconds = 4
        mouseMoveAtSeconds = 6
        guestWorkloadAtSeconds = 4
    }

    if ($ScenarioSet -eq 'quick') {
        return @($baseline, $resize)
    }

    return @($baseline, $resize, $outage)
}

function Invoke-Scenario {
    param(
        [Parameter(Mandatory)][pscustomobject]$Scenario,
        [Parameter(Mandatory)][string]$ClientExe,
        [Parameter(Mandatory)][string]$Destination,
        [Parameter(Mandatory)][string]$VmName,
        [Parameter(Mandatory)][System.Management.Automation.PSCredential]$Credential,
        [Parameter(Mandatory)][string]$Username,
        [Parameter(Mandatory)][string]$Password,
        [Parameter(Mandatory)][string]$OutputRoot,
        [Parameter(Mandatory)][int]$DurationSeconds,
        [Parameter(Mandatory)][int]$SampleIntervalMs,
        [Parameter(Mandatory)][string]$Multitransport,
        [Parameter(Mandatory)][string]$GuestWorkload,
        [Parameter(Mandatory)][int]$Width,
        [Parameter(Mandatory)][int]$Height
    )

    $scenarioDir = Join-Path $OutputRoot $Scenario.name
    New-Item -ItemType Directory -Force -Path $scenarioDir | Out-Null
    $logPath = Join-Path $scenarioDir 'client.log'
    $samplePath = Join-Path $scenarioDir 'cpu-samples.json'
    $summaryPath = Join-Path $scenarioDir 'summary.json'
    $screenshotPath = Join-Path $scenarioDir 'client-window.png'

    if (Test-Path -LiteralPath $logPath) {
        Remove-Item -LiteralPath $logPath -Force
    }

    $arguments = [System.Collections.Generic.List[string]]::new()
    $arguments.Add($Destination)
    $arguments.Add('--username')
    $arguments.Add($Username)
    $arguments.Add('--password')
    $arguments.Add($Password)
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

    $process = Start-Process -FilePath $ClientExe -ArgumentList $arguments -WorkingDirectory (Split-Path -Parent $ClientExe) -PassThru -Environment @{
        IRONRDP_LOG = 'info,ironrdp_client=trace,ironrdp_connector=debug,ironrdp_session=debug'
    }

    $process = Wait-ForMainWindow -ProcessId $process.Id -TimeoutSeconds 15
    if (-not $process) {
        throw "failed to start client process for scenario '$($Scenario.name)'"
    }

    $startedAt = Get-Date
    $samples = New-Object System.Collections.Generic.List[object]
    $events = New-Object System.Collections.Generic.List[object]
    $guestWorkloadResult = $null
    $mouseMoved = $false
    $outageRule = $null
    $outageStarted = $false
    $outageRecovered = $false
    $resizeCompleted = New-Object System.Collections.Generic.HashSet[int]
    $previousCpu = $null
    $previousSampleAt = $null

    try {
        while ((Get-Date) -lt $startedAt.AddSeconds($DurationSeconds)) {
            $current = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
            if (-not $current) {
                break
            }
            $current.Refresh()

            $sampleAt = Get-Date
            $cpuPercent = $null
            if ($previousCpu -ne $null -and $previousSampleAt -ne $null) {
                $elapsedSeconds = ($sampleAt - $previousSampleAt).TotalSeconds
                if ($elapsedSeconds -gt 0) {
                    $cpuPercent = (($current.CPU - $previousCpu) / ($elapsedSeconds * [Environment]::ProcessorCount)) * 100.0
                }
            }

            $samples.Add([pscustomobject]@{
                timestamp = $sampleAt.ToString('o')
                elapsedSeconds = [math]::Round(($sampleAt - $startedAt).TotalSeconds, 2)
                cpuPercent = if ($cpuPercent -ne $null) { [math]::Round($cpuPercent, 2) } else { $null }
                workingSetMb = [math]::Round($current.WorkingSet64 / 1MB, 2)
                privateMemoryMb = [math]::Round($current.PrivateMemorySize64 / 1MB, 2)
                handles = $current.Handles
                threads = $current.Threads.Count
            })
            $previousCpu = $current.CPU
            $previousSampleAt = $sampleAt

            $elapsed = ($sampleAt - $startedAt).TotalSeconds

            if (-not $guestWorkloadResult -and $Scenario.guestWorkloadAtSeconds -ne $null -and $elapsed -ge $Scenario.guestWorkloadAtSeconds) {
                try {
                    $guestWorkloadResult = Invoke-GuestWorkload -VmName $VmName -Credential $Credential -Workload $GuestWorkload
                    $events.Add([pscustomobject]@{ timestamp = $sampleAt.ToString('o'); kind = 'guestWorkload'; result = $guestWorkloadResult })
                } catch {
                    $guestWorkloadResult = [pscustomobject]@{ workload = $GuestWorkload; started = $false; error = ($_ | Out-String).Trim() }
                    $events.Add([pscustomobject]@{ timestamp = $sampleAt.ToString('o'); kind = 'guestWorkloadError'; result = $guestWorkloadResult })
                }
            }

            if (-not $mouseMoved -and $Scenario.mouseMoveAtSeconds -ne $null -and $elapsed -ge $Scenario.mouseMoveAtSeconds) {
                try {
                    $mouseTrace = Invoke-ClientMouseTrace -Process $current
                    $events.Add([pscustomobject]@{ timestamp = $sampleAt.ToString('o'); kind = 'mouseTrace'; result = $mouseTrace })
                } catch {
                    $events.Add([pscustomobject]@{ timestamp = $sampleAt.ToString('o'); kind = 'mouseTraceError'; result = ($_ | Out-String).Trim() })
                }
                $mouseMoved = $true
            }

            for ($i = 0; $i -lt $Scenario.resizeActions.Count; $i++) {
                $action = $Scenario.resizeActions[$i]
                if (-not $resizeCompleted.Contains($i) -and $elapsed -ge $action.atSeconds) {
                    try {
                        $resizeResult = Resize-ClientWindow -Process $current -Width $action.width -Height $action.height
                        $events.Add([pscustomobject]@{ timestamp = $sampleAt.ToString('o'); kind = 'resize'; result = $resizeResult })
                    } catch {
                        $events.Add([pscustomobject]@{ timestamp = $sampleAt.ToString('o'); kind = 'resizeError'; result = ($_ | Out-String).Trim() })
                    }
                    $resizeCompleted.Add($i) | Out-Null
                }
            }

            if (-not $outageStarted -and $Scenario.outageStartSeconds -ne $null -and $elapsed -ge $Scenario.outageStartSeconds) {
                try {
                    $outageRule = New-RdpBlockRule -RemoteAddress $Destination
                    $events.Add([pscustomobject]@{ timestamp = $sampleAt.ToString('o'); kind = 'networkDropStarted'; result = [pscustomobject]@{ rule = $outageRule; remoteAddress = $Destination } })
                } catch {
                    $events.Add([pscustomobject]@{ timestamp = $sampleAt.ToString('o'); kind = 'networkDropStartError'; result = ($_ | Out-String).Trim() })
                }
                $outageStarted = $true
            }

            if ($outageStarted -and -not $outageRecovered -and $Scenario.outageDurationSeconds -ne $null -and $elapsed -ge ($Scenario.outageStartSeconds + $Scenario.outageDurationSeconds)) {
                Remove-RdpBlockRule -RuleName $outageRule
                $outageRecovered = $true
                $events.Add([pscustomobject]@{ timestamp = $sampleAt.ToString('o'); kind = 'networkDropEnded'; result = [pscustomobject]@{ rule = $outageRule } })
            }

            Start-Sleep -Milliseconds $SampleIntervalMs
        }
    } finally {
        Remove-RdpBlockRule -RuleName $outageRule

        $current = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
        if ($current) {
            try {
                Capture-WindowScreenshot -Process $current -Path $screenshotPath | Out-Null
            } catch {
                $events.Add([pscustomobject]@{ timestamp = (Get-Date).ToString('o'); kind = 'screenshotError'; result = ($_ | Out-String).Trim() })
            }

            try {
                $null = $current.CloseMainWindow()
            } catch {
            }

            Start-Sleep -Seconds 3
            $current = Get-Process -Id $current.Id -ErrorAction SilentlyContinue
            if ($current) {
                Stop-Process -Id $current.Id -Force
            }
        }
    }

    $logSummary = Parse-IronRdpLog -Path $logPath
    $cpuSummary = Summarize-ProcessSamples -Samples $samples

    $result = [pscustomobject]@{
        scenario = $Scenario.name
        destination = $Destination
        durationSeconds = $DurationSeconds
        sampleIntervalMs = $SampleIntervalMs
        guestWorkload = $guestWorkloadResult
        screenshotPath = if (Test-Path -LiteralPath $screenshotPath -PathType Leaf) { $screenshotPath } else { $null }
        samplesPath = $samplePath
        logPath = $logPath
        cpu = $cpuSummary
        log = $logSummary
        events = $events
    }

    $samples | ConvertTo-Json -Depth 6 | Set-Content -Path $samplePath -Encoding UTF8
    $result | ConvertTo-Json -Depth 8 | Set-Content -Path $summaryPath -Encoding UTF8

    $result
}

$rootResult = Resolve-PackageRoot -ParameterSetName $PSCmdlet.ParameterSetName
if ($rootResult -is [string]) {
    $root = $rootResult
    $package = $null
} else {
    $root = $rootResult.Root
    $package = $rootResult.Package
}

$clientExe = Join-Path $root 'client\ironrdp-client.exe'
if (-not (Test-Path -LiteralPath $clientExe -PathType Leaf)) {
    throw "client executable not found: $clientExe"
}

$outputPath = if ($OutputRoot) {
    $OutputRoot
} else {
    Join-Path ([System.IO.Path]::GetTempPath()) ("ironrdp-hyperv-suite-{0:yyyyMMdd-HHmmss}" -f (Get-Date))
}
New-Item -ItemType Directory -Force -Path $outputPath | Out-Null

$guest = Resolve-ReachableGuestEndpoint -VmName $VmName
$credential = New-GuestCredential -Username $Username -Password $Password
$scenarios = Get-ScenarioDefinitions -ScenarioSet $ScenarioSet
$results = New-Object System.Collections.Generic.List[object]

foreach ($scenario in $scenarios) {
    $results.Add((
        Invoke-Scenario `
            -Scenario $scenario `
            -ClientExe $clientExe `
            -Destination $guest.selected.ipAddress `
            -VmName $VmName `
            -Credential $credential `
            -Username $Username `
            -Password $Password `
            -OutputRoot $outputPath `
            -DurationSeconds $DurationSeconds `
            -SampleIntervalMs $SampleIntervalMs `
            -Multitransport $Multitransport `
            -GuestWorkload $GuestWorkload `
            -Width $Width `
            -Height $Height
    ))
}

$summary = [pscustomobject]@{
    vmName = $guest.vm.Name
    vmState = $guest.vm.State.ToString()
    vmUptime = $guest.vm.Uptime
    selectedIpAddress = $guest.selected.ipAddress
    reachableAddresses = $guest.reachable
    outputRoot = $outputPath
    scenarioSet = $ScenarioSet
    packageRoot = $root
    packageFamilyName = if ($package) { $package.PackageFamilyName } else { $null }
    results = $results
}

$summaryPath = Join-Path $outputPath 'suite-summary.json'
$summary | ConvertTo-Json -Depth 10 | Set-Content -Path $summaryPath -Encoding UTF8
$summary | ConvertTo-Json -Depth 10
