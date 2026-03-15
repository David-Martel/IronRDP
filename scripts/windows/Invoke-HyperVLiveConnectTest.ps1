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
    [int]$ConnectSeconds = 20,
    [ValidateSet('off', 'prefer-reliable', 'reliable', 'prefer-lossy', 'lossy')]
    [string]$Multitransport = 'off',
    [int]$Width = 1280,
    [int]$Height = 720
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

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

$smokeScript = Join-Path $PSScriptRoot 'Invoke-IronRdpSmokeTest.ps1'
if (-not (Test-Path -LiteralPath $smokeScript -PathType Leaf)) {
    throw "smoke test script not found: $smokeScript"
}

$connectLogPath = Join-Path ([System.IO.Path]::GetTempPath()) ("ironrdp-hyperv-{0}-{1:yyyyMMdd-HHmmss}.log" -f $VmName, (Get-Date))
$commonArgs = @{
    LaunchHost = $selected.ipAddress
    Username = $Username
    Password = $Password
    ConnectSeconds = $ConnectSeconds
    ConnectionLogPath = $connectLogPath
    Multitransport = $Multitransport
    Width = $Width
    Height = $Height
}

$smokeResult = switch ($PSCmdlet.ParameterSetName) {
    'Install' {
        & $smokeScript -InstallRoot $InstallRoot @commonArgs
    }
    'Package' {
        & $smokeScript -PackageRoot $PackageRoot @commonArgs
    }
    'Msix' {
        & $smokeScript -MsixPackageName $MsixPackageName @commonArgs
    }
}

$result = [pscustomobject]@{
    vmName = $vm.Name
    vmState = $vm.State.ToString()
    vmUptime = $vm.Uptime
    selectedIpAddress = $selected.ipAddress
    reachableAddresses = $reachable
    smoke = $smokeResult
}

$result | ConvertTo-Json -Depth 8
