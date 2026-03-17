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
    [ValidateSet('default', 'windows', 'stub', 'none')]
    [string]$ClipboardType = 'default',
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

function Get-HostClipboardState {
    try {
        return [pscustomobject]@{
            available = $true
            text = (Get-Clipboard -Raw -ErrorAction Stop)
        }
    } catch {
        return [pscustomobject]@{
            available = $false
            error = ($_ | Out-String).Trim()
        }
    }
}

function Set-HostClipboardText {
    param([Parameter(Mandatory)][string]$Text)

    Set-Clipboard -Value $Text -ErrorAction Stop

    [pscustomobject]@{
        text = $Text
        length = $Text.Length
    }
}

function Restore-HostClipboardState {
    param([object]$State)

    if (-not $State -or -not $State.available) {
        return
    }

    Set-Clipboard -Value $State.text -ErrorAction SilentlyContinue
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
            interactiveSession = $process -and $process.SessionId -gt 0
            sessionZeroFallback = $process -and $process.SessionId -eq 0
        }
    } finally {
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue | Out-Null
    }
} -ArgumentList $Workload, $Credential.UserName, $plainPassword -ErrorAction Stop
}

function Get-GuestFeatureSnapshot {
    param(
        [Parameter(Mandatory)][string]$VmName,
        [Parameter(Mandatory)][System.Management.Automation.PSCredential]$Credential
    )

    Invoke-Command -VMName $VmName -Credential $Credential -ScriptBlock {
        $audioServices = @(
            Get-Service -Name Audiosrv, AudioEndpointBuilder -ErrorAction SilentlyContinue |
                Select-Object Name, Status, StartType
        )

        [pscustomobject]@{
            clipboardCmdletsAvailable = [bool](Get-Command Set-Clipboard -ErrorAction SilentlyContinue) -and
                [bool](Get-Command Get-Clipboard -ErrorAction SilentlyContinue)
            audioServices = $audioServices
            termService = Get-Service -Name TermService -ErrorAction SilentlyContinue | Select-Object Name, Status, StartType
        }
    }
}

function Get-ClientCapabilityProfile {
    param(
        [Parameter(Mandatory)][string]$ClipboardType,
        [Parameter(Mandatory)][object]$GuestFeatureSnapshot
    )

    [pscustomobject]@{
        rendering = [pscustomobject]@{
            status = 'supported'
            mode = 'software-present'
            backend = 'softbuffer'
            validation = 'frame cadence, timing, and screenshot capture'
        }
        dynamicResize = [pscustomobject]@{
            status = 'supported'
            validation = 'resize scenario plus reconnect/deactivation tracing'
        }
        clipboard = [pscustomobject]@{
            status = if ($ClipboardType -eq 'none') { 'disabled-by-harness' } else { 'channel-observed' }
            mode = $ClipboardType
            guestClipboardAvailable = [bool]$GuestFeatureSnapshot.clipboardCmdletsAvailable
            validation = 'channel attach/init/ack plus host clipboard mutation and client CLIPRDR log activity'
        }
        audio = [pscustomobject]@{
            status = 'channel-observed'
            backend = 'rdpsnd-native/cpal'
            guestAudioServices = $GuestFeatureSnapshot.audioServices
            validation = 'channel wiring plus rdpsnd playback-path log activity'
        }
        deviceRedirection = [pscustomobject]@{
            status = 'unsupported'
            backend = 'NoopRdpdrBackend'
            note = 'drive, printer, usb, and generic RDPDR coverage are not implemented in this fork yet'
        }
    }
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

function Get-RatioValue {
    param(
        [double]$Numerator,
        [double]$Denominator
    )

    if ($Denominator -le 0) {
        return $null
    }

    return [math]::Round($Numerator / $Denominator, 4)
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
        resizeSurfaceFailures = @($entries | Where-Object { $_.message -eq 'Failed to resize drawing surface' }).Count
        surfaceResizeEvents = @($entries | Where-Object { $_.message -like 'Resized presentation surface*' }).Count
        overwrittenFrames = @($entries | Where-Object { $_.message -like 'Overwriting unpresented frame buffer*' }).Count
        replacedPresentedFrames = @($entries | Where-Object { $_.message -like 'Replacing already presented frame buffer*' }).Count
        framePacingFires = @($entries | Where-Object { $_.message -eq 'Frame pacing timer fired, emitting coalesced frame' }).Count
        resizeReconnectRequests = @($entries | Where-Object { $_.message -like 'Resize requires reconnect*' }).Count
        sameSizeReconnectGuard = @($entries | Where-Object { $_.message -like '*too many resize reconnects without a desktop size change*' }).Count
        missingDecompressorWarnings = @($entries | Where-Object { $_.message -eq 'Received compressed FastPath data but no decompressor is configured' }).Count
        bulkDecompressionFailures = @($entries | Where-Object { $_.message -like '*bulk decompression failed*' }).Count
        reactivationCompletions = @($entries | Where-Object { $_.message -like 'Deactivation-Reactivation Sequence completed*' }).Count
        clipboardForwarded = @($entries | Where-Object { $_.message -like 'Forwarding local clipboard event*' }).Count
        clipboardHandled = @($entries | Where-Object { $_.message -like 'Handling clipboard event*' }).Count
        cliprdrAttached = @($entries | Where-Object { $_.message -eq 'Attach CLIPRDR channel' }).Count
        cliprdrInitialized = @($entries | Where-Object { $_.message -like 'CLIPRDR(clipboard) virtual channel has been initialized*' }).Count
        cliprdrFormatListAck = @($entries | Where-Object { $_.message -like 'CLIPRDR(clipboard) Remote has received format list successfully*' }).Count
        cliprdrFailures = @($entries | Where-Object { $_.message -like 'CLIPRDR(clipboard) failed:*' }).Count
        clipboardBackendErrors = @($entries | Where-Object { $_.message -like 'Clipboard backend error:*' }).Count
        clipboardUnavailable = @($entries | Where-Object { $_.message -eq 'Clipboard event received, but Cliprdr is not available' }).Count
        audioChannelEnabled = @($entries | Where-Object { $_.message -eq 'Enable RDPSND playback channel' }).Count
        audioFormatChanges = @($entries | Where-Object { $_.message -eq 'New audio format' }).Count
        audioUnderruns = @($entries | Where-Object { $_.message -eq 'Playback rx underrun' }).Count
        deviceRedirectionUnsupported = @($entries | Where-Object { $_.message -eq 'Configured RDPDR backend backend=NoopRdpdrBackend' }).Count
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
        derived = [pscustomobject]@{
            overwritePerPresentedFrame = Get-RatioValue -Numerator $counts.overwrittenFrames -Denominator $counts.presentedFrames
            overwritePerImageUpdate = Get-RatioValue -Numerator $counts.overwrittenFrames -Denominator $counts.imageUpdates
            presentedPerImageUpdate = Get-RatioValue -Numerator $counts.presentedFrames -Denominator $counts.imageUpdates
            firstImageToFrameMs = if ($firstImage -and $firstFrame) { [math]::Round(($firstFrame.timestamp - $firstImage.timestamp).TotalMilliseconds, 2) } else { $null }
        }
        counts = [pscustomobject]$counts
        tail = Get-Content -LiteralPath $Path -Tail 120 | Out-String
    }
}

function Get-ClipboardObservationStage {
    param(
        [Parameter(Mandatory)][object]$LogSummary,
        [Parameter()][object]$HostClipboardResult
    )

    $counts = $LogSummary.counts
    if ($counts.cliprdrAttached -le 0) { return 'disabled-or-unwired' }
    if ($counts.clipboardHandled -gt 0) { return 'client-handled' }
    if ($counts.clipboardForwarded -gt 0) { return 'local-forwarded' }
    if ($counts.cliprdrFormatListAck -gt 0) { return 'remote-ack-observed' }
    if ($counts.cliprdrInitialized -gt 0) { return 'initialized' }
    if ($HostClipboardResult) { return 'host-mutation-triggered' }
    return 'channel-attached'
}

function Get-AudioObservationStage {
    param([Parameter(Mandatory)][object]$LogSummary)

    $counts = $LogSummary.counts
    if ($counts.audioFormatChanges -gt 0) { return 'playback-observed' }
    if ($counts.audioChannelEnabled -gt 0) { return 'channel-wired' }
    return 'not-observed'
}

function Get-GuestWorkloadStage {
    param([Parameter()][object]$GuestWorkloadResult)

    if (-not $GuestWorkloadResult) { return 'not-requested' }
    if ($GuestWorkloadResult.PSObject.Properties.Name -contains 'error') { return 'launch-error' }
    if (-not $GuestWorkloadResult.started) { return 'not-started' }
    if ($GuestWorkloadResult.interactiveWindow) { return 'interactive-window' }
    if ($GuestWorkloadResult.interactiveSession) { return 'interactive-session-no-window' }
    if ($GuestWorkloadResult.sessionZeroFallback) { return 'session-0-fallback' }
    return 'background-process'
}

function Get-ScenarioDiagnosis {
    param(
        [Parameter(Mandatory)][object]$Scenario,
        [Parameter(Mandatory)][object]$LogSummary,
        [Parameter()][object]$GuestWorkloadResult
    )

    $counts = $LogSummary.counts
    $derived = $LogSummary.derived
    $latency = $LogSummary.latencyMs
    $signals = New-Object System.Collections.Generic.List[string]
    $primary = 'healthy'

    $workloadStage = Get-GuestWorkloadStage -GuestWorkloadResult $GuestWorkloadResult
    if ($workloadStage -eq 'session-0-fallback') {
        $signals.Add('guest workload only reached session 0')
    } elseif ($workloadStage -eq 'interactive-session-no-window') {
        $signals.Add('guest workload reached an interactive session without a visible top-level window')
    } elseif ($workloadStage -eq 'launch-error') {
        $signals.Add('guest workload launch returned an error')
    }

    if ($counts.connectionEstablished -le 0 -or $LogSummary.status -eq 'connection-started') {
        $primary = 'transport-limited'
        $signals.Add('connection never reached established state')
    } elseif ($counts.connectionErrors -gt 0 -or $latency.connectToEstablished -gt 1000) {
        $primary = 'transport-limited'
        if ($counts.connectionErrors -gt 0) {
            $signals.Add("connection errors observed: $($counts.connectionErrors)")
        }
        if ($latency.connectToEstablished -gt 1000) {
            $signals.Add("slow connect-to-established latency: $($latency.connectToEstablished) ms")
        }
    } elseif ($counts.activeSessionErrors -gt 0 -or $counts.missingDecompressorWarnings -gt 0 -or $counts.bulkDecompressionFailures -gt 0) {
        $primary = 'decode-limited'
        if ($counts.activeSessionErrors -gt 0) {
            $signals.Add("active session errors observed: $($counts.activeSessionErrors)")
        }
        if ($counts.missingDecompressorWarnings -gt 0) {
            $signals.Add('compressed fast-path data arrived without a decompressor')
        }
        if ($counts.bulkDecompressionFailures -gt 0) {
            $signals.Add("bulk decompression failures observed: $($counts.bulkDecompressionFailures)")
        }
    } elseif ($counts.imageUpdates -gt 0 -and $counts.presentedFrames -le 0) {
        $primary = 'present-limited'
        $signals.Add('image updates were emitted but no frames were presented')
    } elseif ($counts.surfaceFailures -gt 0 -or $counts.resizeSurfaceFailures -gt 0) {
        $primary = 'present-limited'
        if ($counts.surfaceFailures -gt 0) {
            $signals.Add("surface present failures observed: $($counts.surfaceFailures)")
        }
        if ($counts.resizeSurfaceFailures -gt 0) {
            $signals.Add("surface resize failures observed: $($counts.resizeSurfaceFailures)")
        }
    } elseif ($derived.overwritePerPresentedFrame -gt 0.25 -or $derived.firstImageToFrameMs -gt 120 -or ($LogSummary.presentMicros.average -gt 0 -and $LogSummary.convertMicros.average -gt 0 -and $LogSummary.presentMicros.average -ge ($LogSummary.convertMicros.average * 1.5))) {
        $primary = 'present-limited'
        if ($derived.overwritePerPresentedFrame -gt 0.25) {
            $signals.Add("high overwrite-per-presented-frame ratio: $($derived.overwritePerPresentedFrame)")
        }
        if ($derived.firstImageToFrameMs -gt 120) {
            $signals.Add("slow first-image-to-frame latency: $($derived.firstImageToFrameMs) ms")
        }
        if ($LogSummary.presentMicros.average -gt 0 -and $LogSummary.convertMicros.average -gt 0 -and $LogSummary.presentMicros.average -ge ($LogSummary.convertMicros.average * 1.5)) {
            $signals.Add("present cost dominates conversion cost: avg present $($LogSummary.presentMicros.average) us vs convert $($LogSummary.convertMicros.average) us")
        }
    } elseif ($latency.connectToFirstImage -gt 1200 -and $counts.imageUpdates -le 2) {
        $primary = 'transport-limited'
        $signals.Add("slow connect-to-first-image latency: $($latency.connectToFirstImage) ms")
    }

    if ($signals.Count -eq 0) {
        if ($Scenario.name -eq 'resize' -and $counts.resizeReconnectRequests -gt 0) {
            $signals.Add("resize required reconnects: $($counts.resizeReconnectRequests)")
        } elseif ($counts.audioChannelEnabled -gt 0 -and $counts.audioFormatChanges -le 0) {
            $signals.Add('audio channel is wired but guest-side playback is not yet proven')
        } else {
            $signals.Add('no dominant bottleneck detected in current scenario')
        }
    }

    [pscustomobject]@{
        primary = $primary
        workloadStage = $workloadStage
        signals = @($signals)
    }
}

function Get-ScenarioHealth {
    param(
        [Parameter(Mandatory)][object]$Scenario,
        [Parameter(Mandatory)][object]$LogSummary,
        [AllowEmptyCollection()][object[]]$Events = @(),
        [Parameter()][object]$HostClipboardResult,
        [Parameter()][object]$GuestWorkloadResult
    )

    $failures = New-Object System.Collections.Generic.List[string]
    $warnings = New-Object System.Collections.Generic.List[string]
    $counts = $LogSummary.counts
    $derived = $LogSummary.derived
    $diagnosis = Get-ScenarioDiagnosis -Scenario $Scenario -LogSummary $LogSummary -GuestWorkloadResult $GuestWorkloadResult
    $resizeEvents = @($Events | Where-Object kind -eq 'resize')
    $resizeErrors = @($Events | Where-Object kind -eq 'resizeError')
    $hasClipboardError = $false
    $hasClipboardMutation = $false
    if ($HostClipboardResult) {
        $hasClipboardError = $HostClipboardResult.PSObject.Properties.Name -contains 'error'
        $hasClipboardMutation = $HostClipboardResult.PSObject.Properties.Name -contains 'after'
    }

    if ($counts.connectionEstablished -le 0) {
        $failures.Add('connection was never established')
    }
    if ($counts.firstPresentedFrame -le 0) {
        $failures.Add('no frame was presented')
    }
    if ($counts.connectionErrors -gt 0) {
        $failures.Add("connection errors observed: $($counts.connectionErrors)")
    }
    if ($counts.activeSessionErrors -gt 0) {
        $failures.Add("active session errors observed: $($counts.activeSessionErrors)")
    }
    if ($counts.surfaceFailures -gt 0) {
        $failures.Add("surface present failures observed: $($counts.surfaceFailures)")
    }
    if ($counts.resizeSurfaceFailures -gt 0) {
        $failures.Add("surface resize failures observed: $($counts.resizeSurfaceFailures)")
    }

    if ($Scenario.name -eq 'resize') {
        if ($resizeEvents.Count -ne $Scenario.resizeActions.Count) {
            $failures.Add("expected $($Scenario.resizeActions.Count) resize actions but observed $($resizeEvents.Count)")
        }
        if ($resizeErrors.Count -gt 0) {
            $failures.Add("resize automation errors observed: $($resizeErrors.Count)")
        }
        if ($counts.missingDecompressorWarnings -gt 0) {
            $failures.Add("post-reactivation decompressor warnings observed: $($counts.missingDecompressorWarnings)")
        }
        if ($counts.bulkDecompressionFailures -gt 0) {
            $failures.Add("bulk decompression failures observed: $($counts.bulkDecompressionFailures)")
        }
        if ($counts.sameSizeReconnectGuard -gt 0) {
            $failures.Add("resize reconnect guard triggered: $($counts.sameSizeReconnectGuard)")
        }
        if ($counts.surfaceResizeEvents -le 0) {
            $warnings.Add('no presentation-surface resize event was observed')
        }
        if ($counts.resizeReconnectRequests -gt 0) {
            $warnings.Add("resize requested reconnects: $($counts.resizeReconnectRequests)")
        }
        if ($counts.reactivationCompletions -le 0) {
            $warnings.Add('no deactivation-reactivation completion was observed')
        }
    }

    if ($counts.shutdownDenied -gt 0) {
        $warnings.Add("shutdown denied count: $($counts.shutdownDenied)")
    }
    if ($counts.cliprdrFailures -gt 0 -or $counts.clipboardBackendErrors -gt 0 -or $counts.clipboardUnavailable -gt 0) {
        $warnings.Add('clipboard channel reported failures or unavailable events')
    }
    if ($hasClipboardMutation -and -not $hasClipboardError -and $counts.clipboardHandled -le 0) {
        $warnings.Add('host clipboard mutation did not produce client-handled clipboard activity')
    }
    if ($counts.audioChannelEnabled -gt 0 -and $counts.audioFormatChanges -le 0) {
        $warnings.Add('audio channel was wired but no playback format change was observed')
    }
    if ($counts.audioUnderruns -gt 0) {
        $warnings.Add("audio underruns observed: $($counts.audioUnderruns)")
    }
    if ($derived.overwritePerPresentedFrame -gt 0.25) {
        $warnings.Add("high overwrite-per-presented-frame ratio: $($derived.overwritePerPresentedFrame)")
    }
    if ($Scenario.guestWorkloadAtSeconds -ne $null -and $GuestWorkloadResult) {
        switch ($diagnosis.workloadStage) {
            'launch-error' { $warnings.Add('guest workload launch failed and interactive workload timing is unavailable') }
            'not-started' { $warnings.Add('guest workload did not start') }
            'session-0-fallback' { $warnings.Add('guest workload only reached session 0 fallback') }
            'interactive-session-no-window' { $warnings.Add('guest workload reached an interactive session without a visible window') }
        }
    }

    [pscustomobject]@{
        passed = ($failures.Count -eq 0)
        failures = @($failures)
        warnings = @($warnings)
        clipboardStage = Get-ClipboardObservationStage -LogSummary $LogSummary -HostClipboardResult $HostClipboardResult
        audioStage = Get-AudioObservationStage -LogSummary $LogSummary
        diagnosis = $diagnosis
    }
}

function Get-SuiteHealthRollup {
    param([Parameter(Mandatory)][object[]]$Results)

    $baseline = $Results | Where-Object scenario -eq 'baseline' | Select-Object -First 1
    $resize = $Results | Where-Object scenario -eq 'resize' | Select-Object -First 1

    [pscustomobject]@{
        baselinePassed = if ($baseline) { [bool]$baseline.health.passed } else { $false }
        resizePassed = if ($resize) { [bool]$resize.health.passed } else { $false }
        clipboardObservedStage = @($Results | ForEach-Object { $_.health.clipboardStage } | Select-Object -Unique) -join ','
        audioObservedStage = @($Results | ForEach-Object { $_.health.audioStage } | Select-Object -Unique) -join ','
        workloadObservedStage = @($Results | ForEach-Object { $_.health.diagnosis.workloadStage } | Select-Object -Unique) -join ','
        primaryDiagnosis = @($Results | ForEach-Object { $_.health.diagnosis.primary } | Group-Object | Sort-Object Count -Descending | Select-Object -First 1).Name
        diagnosisSignals = @(
            $Results |
                ForEach-Object {
                    foreach ($signal in $_.health.diagnosis.signals) {
                        '{0}: {1}' -f $_.scenario, $signal
                    }
                } |
                Select-Object -Unique
        )
        worstOverwritePerPresentedFrame = (
            $Results |
                ForEach-Object { $_.log.derived.overwritePerPresentedFrame } |
                Where-Object { $null -ne $_ } |
                Measure-Object -Maximum
        ).Maximum
        worstConnectToFirstFrameMs = (
            $Results |
                ForEach-Object { $_.log.latencyMs.connectToFirstFrame } |
                Where-Object { $null -ne $_ } |
                Measure-Object -Maximum
        ).Maximum
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
    param(
        [string]$ScenarioSet,
        [string]$ClipboardType
    )

    $baseline = [pscustomobject]@{
        name = 'baseline'
        resizeActions = @()
        outageStartSeconds = $null
        outageDurationSeconds = $null
        mouseMoveAtSeconds = 8
        guestWorkloadAtSeconds = 4
        hostClipboardAtSeconds = if ($ClipboardType -eq 'none') { $null } else { 6 }
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
        hostClipboardAtSeconds = if ($ClipboardType -eq 'none') { $null } else { 6 }
    }

    $outage = [pscustomobject]@{
        name = 'outage'
        resizeActions = @()
        outageStartSeconds = 10
        outageDurationSeconds = 4
        mouseMoveAtSeconds = 6
        guestWorkloadAtSeconds = 4
        hostClipboardAtSeconds = if ($ClipboardType -eq 'none') { $null } else { 5 }
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
    $arguments.Add($ClipboardType)
    $arguments.Add('--no-server-pointer')
    $arguments.Add('--log-file')
    $arguments.Add($logPath)

    if ($Multitransport -ne 'off') {
        $arguments.Add('--multitransport')
        $arguments.Add($Multitransport)
    }

    $process = Start-Process -FilePath $ClientExe -ArgumentList $arguments -WorkingDirectory (Split-Path -Parent $ClientExe) -PassThru -Environment @{
        IRONRDP_LOG = 'info,ironrdp_client=trace,ironrdp_connector=debug,ironrdp_session=debug,ironrdp_cliprdr=trace,ironrdp_rdpsnd=debug,ironrdp_rdpsnd_native=debug'
    }

    $process = Wait-ForMainWindow -ProcessId $process.Id -TimeoutSeconds 15
    if (-not $process) {
        throw "failed to start client process for scenario '$($Scenario.name)'"
    }

    $startedAt = Get-Date
    $samples = New-Object System.Collections.Generic.List[object]
    $events = New-Object System.Collections.Generic.List[object]
    $guestWorkloadResult = $null
    $hostClipboardResult = $null
    $hostClipboardOriginal = $null
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

            if (-not $hostClipboardResult -and $Scenario.hostClipboardAtSeconds -ne $null -and $elapsed -ge $Scenario.hostClipboardAtSeconds) {
                try {
                    $hostClipboardOriginal = Get-HostClipboardState
                    $marker = "IronRDP Hyper-V clipboard test {0:o}" -f (Get-Date)
                    $hostClipboardResult = [pscustomobject]@{
                        before = $hostClipboardOriginal
                        after = Set-HostClipboardText -Text $marker
                    }
                    $events.Add([pscustomobject]@{ timestamp = $sampleAt.ToString('o'); kind = 'hostClipboardSet'; result = $hostClipboardResult.after })
                } catch {
                    $hostClipboardResult = [pscustomobject]@{
                        error = ($_ | Out-String).Trim()
                    }
                    $events.Add([pscustomobject]@{ timestamp = $sampleAt.ToString('o'); kind = 'hostClipboardError'; result = $hostClipboardResult.error })
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
        Restore-HostClipboardState -State $hostClipboardOriginal

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
    $health = Get-ScenarioHealth -Scenario $Scenario -LogSummary $logSummary -Events $events -HostClipboardResult $hostClipboardResult -GuestWorkloadResult $guestWorkloadResult

    $result = [pscustomobject]@{
        scenario = $Scenario.name
        destination = $Destination
        durationSeconds = $DurationSeconds
        sampleIntervalMs = $SampleIntervalMs
        guestWorkload = $guestWorkloadResult
        hostClipboard = $hostClipboardResult
        screenshotPath = if (Test-Path -LiteralPath $screenshotPath -PathType Leaf) { $screenshotPath } else { $null }
        samplesPath = $samplePath
        logPath = $logPath
        cpu = $cpuSummary
        log = $logSummary
        health = $health
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
$guestFeatureSnapshot = Get-GuestFeatureSnapshot -VmName $VmName -Credential $credential
$clientCapabilities = Get-ClientCapabilityProfile -ClipboardType $ClipboardType -GuestFeatureSnapshot $guestFeatureSnapshot
$scenarios = Get-ScenarioDefinitions -ScenarioSet $ScenarioSet -ClipboardType $ClipboardType
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
    guestFeatures = $guestFeatureSnapshot
    clientCapabilities = $clientCapabilities
    outputRoot = $outputPath
    scenarioSet = $ScenarioSet
    packageRoot = $root
    packageFamilyName = if ($package) { $package.PackageFamilyName } else { $null }
    health = Get-SuiteHealthRollup -Results $results
    results = $results
}

$summaryPath = Join-Path $outputPath 'suite-summary.json'
$summary | ConvertTo-Json -Depth 10 | Set-Content -Path $summaryPath -Encoding UTF8
$summary | ConvertTo-Json -Depth 10
