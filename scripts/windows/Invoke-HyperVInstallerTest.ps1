[CmdletBinding()]
param(
    [string]$VmName = 'WS2025-ReFS-Repair',
    [string]$VhdPath = 'T:\HyperV\WS2025-ReFS-Repair\boot.vhdx',
    [Parameter(Mandatory)]
    [string]$MsiPath,
    [string]$ServiceName = 'IronRdpMsiBootstrap',
    [string]$BootstrapRelativePath = 'ProgramData\IronRDP\msi-e2e',
    [string]$LogRelativePath = 'ProgramData\IronRDP\msi-e2e-logs',
    [string]$TestUsername = 'IronRdpLab',
    [string]$TestPassword = 'TempIronRdp!2026',
    [int]$BootWaitSeconds = 150,
    [switch]$StopVmAfterTest
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-VhdWindowsDrive {
    param([Parameter(Mandatory)][string]$MountedVhdPath)

    $mounted = Mount-VHD -Path $MountedVhdPath -Passthru
    $disk = $mounted | Get-Disk
    $partition = $disk | Get-Partition | Where-Object DriveLetter | Sort-Object Size -Descending | Select-Object -First 1
    if (-not $partition) {
        throw "no mounted partition with a drive letter found for $MountedVhdPath"
    }

    return "$($partition.DriveLetter):"
}

function Set-OfflineBootstrapService {
    param(
        [Parameter(Mandatory)][string]$WindowsDrive,
        [Parameter(Mandatory)][string]$ServiceName,
        [Parameter(Mandatory)][string]$BootstrapRelativePath
    )

    $systemHivePath = Join-Path $WindowsDrive 'Windows\System32\Config\SYSTEM'
    $mountName = "IRTEST_$([Guid]::NewGuid().ToString('N'))"
    $regRoot = "HKLM\$mountName"
    & reg load $regRoot $systemHivePath | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "failed to load guest SYSTEM hive from $systemHivePath"
    }

    try {
        foreach ($controlSet in 'ControlSet001', 'ControlSet002') {
            $serviceKey = "Registry::$regRoot\$controlSet\Services\$ServiceName"
            if (Test-Path -LiteralPath $serviceKey) {
                Remove-Item -LiteralPath $serviceKey -Recurse -Force
            }

            New-Item -Path $serviceKey -Force | Out-Null
            New-ItemProperty -Path $serviceKey -Name Type -PropertyType DWord -Value 0x10 -Force | Out-Null
            New-ItemProperty -Path $serviceKey -Name Start -PropertyType DWord -Value 2 -Force | Out-Null
            New-ItemProperty -Path $serviceKey -Name ErrorControl -PropertyType DWord -Value 1 -Force | Out-Null
            New-ItemProperty -Path $serviceKey -Name ImagePath -PropertyType ExpandString -Value "C:\Windows\System32\cmd.exe /c start `"`" /min C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File C:\$BootstrapRelativePath\bootstrap.ps1" -Force | Out-Null
            New-ItemProperty -Path $serviceKey -Name DisplayName -PropertyType String -Value 'IronRDP MSI E2E Bootstrap' -Force | Out-Null
            New-ItemProperty -Path $serviceKey -Name Description -PropertyType String -Value 'One-shot IronRDP MSI install and smoke test bootstrap' -Force | Out-Null
            New-ItemProperty -Path $serviceKey -Name ObjectName -PropertyType String -Value 'LocalSystem' -Force | Out-Null
        }
    } finally {
        & reg unload $regRoot | Out-Null
    }
}

$resolvedMsiPath = (Resolve-Path -LiteralPath $MsiPath).Path

if ((Get-VM -Name $VmName).State -ne 'Off') {
    Stop-VM -Name $VmName -Force | Out-Null
    Start-Sleep -Seconds 5
}

if (Get-DiskImage -ImagePath $VhdPath -ErrorAction SilentlyContinue | Where-Object Attached) {
    Dismount-VHD -Path $VhdPath
}

$windowsDrive = Get-VhdWindowsDrive -MountedVhdPath $VhdPath
try {
    $bootstrapDir = Join-Path $windowsDrive $BootstrapRelativePath
    $logDir = Join-Path $windowsDrive $LogRelativePath

    if (Test-Path -LiteralPath $bootstrapDir) {
        Remove-Item -LiteralPath $bootstrapDir -Recurse -Force
    }
    if (Test-Path -LiteralPath $logDir) {
        Remove-Item -LiteralPath $logDir -Recurse -Force
    }

    New-Item -ItemType Directory -Force -Path $bootstrapDir, $logDir | Out-Null
    Copy-Item -LiteralPath $resolvedMsiPath -Destination (Join-Path $bootstrapDir 'IronRDP.msi') -Force

    $bootstrapScript = @"
`$ErrorActionPreference = 'Stop'
`$logRoot = 'C:\$LogRelativePath'
New-Item -ItemType Directory -Force -Path `$logRoot | Out-Null
function Write-Step([string]`$Message) {
    Add-Content -Path (Join-Path `$logRoot 'progress.log') -Value ('[' + (Get-Date).ToString('o') + '] ' + `$Message)
}
Write-Step 'bootstrap starting'
Start-Transcript -Path (Join-Path `$logRoot 'transcript.txt') -Force
try {
    Write-Step 'enabling terminal server and firewall'
    reg add 'HKLM\SYSTEM\CurrentControlSet\Control\Terminal Server' /v fDenyTSConnections /t REG_DWORD /d 0 /f | Out-Null
    Enable-NetFirewallRule -DisplayGroup 'Remote Desktop' | Out-Null
    Start-Service -Name TermService -ErrorAction SilentlyContinue

    Write-Step 'creating test account'
    `$securePassword = ConvertTo-SecureString '$TestPassword' -AsPlainText -Force
    if (-not (Get-LocalUser -Name '$TestUsername' -ErrorAction SilentlyContinue)) {
        New-LocalUser -Name '$TestUsername' -Password `$securePassword -PasswordNeverExpires -AccountNeverExpires | Out-Null
    }
    Add-LocalGroupMember -Group 'Administrators' -Member '$TestUsername' -ErrorAction SilentlyContinue
    Add-LocalGroupMember -Group 'Remote Desktop Users' -Member '$TestUsername' -ErrorAction SilentlyContinue

    Write-Step 'starting MSI install'
    `$msiLog = Join-Path `$logRoot 'msi-install.log'
    `$process = Start-Process -FilePath 'msiexec.exe' -ArgumentList '/i', 'C:\$BootstrapRelativePath\IronRDP.msi', '/qn', '/norestart', '/l*v', `$msiLog -Wait -PassThru
    if (`$process.ExitCode -ne 0) {
        throw "msiexec failed with exit code `$(`$process.ExitCode)"
    }

    Write-Step 'running client smoke commands'
    `$clientExe = 'C:\Program Files\IronRDP\client\ironrdp-client.exe'
    `$version = (& `$clientExe --version | Out-String).Trim()
    `$help = (& `$clientExe --help | Out-String)
    `$services = Get-Service TermService, WinRM, sshd -ErrorAction SilentlyContinue | Select-Object Name, Status, StartType
    `$addresses = Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { `$_.IPAddress -notlike '169.254*' -and `$_.IPAddress -ne '127.0.0.1' } | Select-Object InterfaceAlias,IPAddress

    [pscustomobject]@{
        hostname = `$env:COMPUTERNAME
        installed = Test-Path -LiteralPath `$clientExe
        version = `$version
        helpLength = `$help.Length
        services = `$services
        addresses = `$addresses
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path `$logRoot 'result.json') -Encoding UTF8

    Set-Content -Path (Join-Path `$logRoot 'MSI_SUCCESS.marker') -Value 'ok' -Encoding ASCII
    Write-Step 'bootstrap completed successfully'
}
catch {
    Write-Step ('bootstrap failed: ' + (`$_ | Out-String).Trim())
    `$_ | Out-String | Set-Content -Path (Join-Path `$logRoot 'ERROR.txt') -Encoding UTF8
    throw
}
finally {
    Stop-Transcript | Out-Null
}
"@

    Set-Content -LiteralPath (Join-Path $bootstrapDir 'bootstrap.ps1') -Value $bootstrapScript -Encoding UTF8
    Set-OfflineBootstrapService -WindowsDrive $windowsDrive -ServiceName $ServiceName -BootstrapRelativePath $BootstrapRelativePath
} finally {
    Dismount-VHD -Path $VhdPath
}

Start-VM -Name $VmName | Out-Null
Start-Sleep -Seconds $BootWaitSeconds
$credential = New-Object System.Management.Automation.PSCredential(".\$TestUsername", (ConvertTo-SecureString $TestPassword -AsPlainText -Force))
$liveResult = $null
$liveError = $null

try {
    $liveResult = Invoke-Command -VMName $VmName -Credential $credential -ScriptBlock {
        $clientExe = 'C:\Program Files\IronRDP\client\ironrdp-client.exe'
        [pscustomobject]@{
            hostname = $env:COMPUTERNAME
            installed = Test-Path -LiteralPath $clientExe
            version = if (Test-Path -LiteralPath $clientExe) { (& $clientExe --version | Out-String).Trim() } else { $null }
            helpLength = if (Test-Path -LiteralPath $clientExe) { ((& $clientExe --help | Out-String).Length) } else { $null }
            services = Get-Service TermService, WinRM, sshd -ErrorAction SilentlyContinue | Select-Object Name, Status, StartType
            addresses = Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { $_.IPAddress -notlike '169.254*' -and $_.IPAddress -ne '127.0.0.1' } | Select-Object InterfaceAlias,IPAddress
            logs = [pscustomobject]@{
                success = Test-Path -LiteralPath 'C:\ProgramData\IronRDP\msi-e2e-logs\MSI_SUCCESS.marker'
                progress = if (Test-Path -LiteralPath 'C:\ProgramData\IronRDP\msi-e2e-logs\progress.log') { Get-Content -LiteralPath 'C:\ProgramData\IronRDP\msi-e2e-logs\progress.log' -Raw } else { $null }
                transcript = if (Test-Path -LiteralPath 'C:\ProgramData\IronRDP\msi-e2e-logs\transcript.txt') { Get-Content -LiteralPath 'C:\ProgramData\IronRDP\msi-e2e-logs\transcript.txt' -Raw } else { $null }
                error = if (Test-Path -LiteralPath 'C:\ProgramData\IronRDP\msi-e2e-logs\ERROR.txt') { Get-Content -LiteralPath 'C:\ProgramData\IronRDP\msi-e2e-logs\ERROR.txt' -Raw } else { $null }
                msiTail = if (Test-Path -LiteralPath 'C:\ProgramData\IronRDP\msi-e2e-logs\msi-install.log') { (Get-Content -LiteralPath 'C:\ProgramData\IronRDP\msi-e2e-logs\msi-install.log' -Tail 80 | Out-String) } else { $null }
            }
        }
    } -ErrorAction Stop
} catch {
    $liveError = $_ | Out-String
}

$vmIps = (Get-VMNetworkAdapter -VMName $VmName).IPAddresses | Where-Object { $_ -match '^\d+\.\d+\.\d+\.\d+$' -and $_ -ne '127.0.0.1' }
$rdpPort = if ($vmIps) { Test-NetConnection -ComputerName $vmIps[0] -Port 3389 -InformationLevel Detailed -WarningAction SilentlyContinue } else { $null }

$result = [pscustomobject]@{
    vmName = $VmName
    vmState = (Get-VM -Name $VmName).State.ToString()
    liveResult = $liveResult
    liveError = $liveError
    vmIpAddresses = $vmIps
    rdpPortOpen = if ($rdpPort) { $rdpPort.TcpTestSucceeded } else { $null }
}

if ($StopVmAfterTest) {
    Stop-VM -Name $VmName -Force | Out-Null
    $result | Add-Member -NotePropertyName 'vmStateAfterStop' -NotePropertyValue ((Get-VM -Name $VmName).State.ToString())
}

$result | ConvertTo-Json -Depth 8
