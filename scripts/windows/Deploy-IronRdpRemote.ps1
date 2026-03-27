#requires -Version 7.0

<#
.SYNOPSIS
    Deploy IronRDP package to a remote Windows machine via SSH.
.DESCRIPTION
    Copies a portable bundle zip to the remote host, expands it, runs the
    install script, and executes a smoke test to verify the deployment.

    The remote machine must have OpenSSH server running and PowerShell 7+
    available as 'pwsh' on PATH.  Password-less SSH (key auth) is strongly
    preferred; pass -SshKeyPath when the private key is not in the default
    ssh-agent keyring.

    Exit codes
        0   Deployment and all requested tests passed.
        1   Deployment or required smoke test failed (details in result object).
.PARAMETER BundlePath
    Path to the portable bundle zip produced by build.ps1 (e.g.
    IronRDP-dtm-p1gen7-1.2.3-portable.zip).
.PARAMETER RemoteHost
    Hostname or IP address of the target Windows machine (e.g. dtm-p1gen7 or
    172.23.187.173).
.PARAMETER RemoteUser
    SSH username on the remote host.  Defaults to the current $env:USERNAME.
.PARAMETER SshKeyPath
    Optional path to a PEM/OpenSSH private key.  When omitted, ssh/scp use
    the key already loaded in ssh-agent.
.PARAMETER RemoteTempDir
    Working directory created on the remote machine for bundle transfer and
    extraction.  Defaults to C:\Temp\IronRDP-deploy.
.PARAMETER RemoteInstallRoot
    Explicit install root passed to Install-IronRdpPackage.ps1 on the remote.
    When omitted the install script uses its own default
    ($env:LOCALAPPDATA\Programs\IronRDP).
.PARAMETER SmokeTestHost
    If set, run a live-connect smoke test from the remote machine against this
    RDP host after installation completes.
.PARAMETER SmokeTestUsername
    RDP username for the live-connect smoke test.
.PARAMETER SmokeTestPassword
    RDP password for the live-connect smoke test.
.PARAMETER SmokeTestConnectSeconds
    How long (seconds) to hold the RDP connection open during the smoke test.
    Defaults to 15.
.PARAMETER Force
    Passed through to Install-IronRdpPackage.ps1 to allow overwriting an
    existing installation on the remote machine.
.EXAMPLE
    ./Deploy-IronRdpRemote.ps1 -BundlePath T:\artifacts\IronRDP-portable.zip -RemoteHost dtm-p1gen7
.EXAMPLE
    ./Deploy-IronRdpRemote.ps1 `
        -BundlePath T:\artifacts\IronRDP-portable.zip `
        -RemoteHost dtm-p1gen7 `
        -SmokeTestHost 172.23.187.173 `
        -SmokeTestUsername IronRdpLab `
        -SmokeTestPassword 'TempIronRdp!2026' `
        -Force
.EXAMPLE
    ./Deploy-IronRdpRemote.ps1 `
        -BundlePath T:\artifacts\IronRDP-portable.zip `
        -RemoteHost dtm-p1gen7 `
        -SshKeyPath ~/.ssh/id_ed25519 `
        -RemoteInstallRoot 'C:\IronRDP' `
        -Force | ConvertTo-Json -Depth 6
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$BundlePath,

    [Parameter(Mandatory)]
    [string]$RemoteHost,

    [string]$RemoteUser = $env:USERNAME,

    [string]$SshKeyPath,

    [string]$RemoteTempDir = 'C:\Temp\IronRDP-deploy',

    [string]$RemoteInstallRoot,

    # If set, run a live-connect smoke test against this host after install
    [string]$SmokeTestHost,
    [string]$SmokeTestUsername,
    [string]$SmokeTestPassword,
    [int]$SmokeTestConnectSeconds = 15,

    [switch]$Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

function Write-Step {
    param([string]$Message)
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-StepOk {
    param([string]$Message)
    Write-Host "    OK  $Message" -ForegroundColor Green
}

function Build-SshArgs {
    # Returns a [string[]] of common ssh/scp flags (key path only; -o flags
    # that suppress host-key prompts can be added here when needed).
    if ($SshKeyPath) {
        return @('-i', $SshKeyPath)
    }
    return @()
}

function Invoke-Ssh {
    param(
        [Parameter(Mandatory)][string]$Command
    )

    $sshArgs = Build-SshArgs
    $sshArgs += @("${RemoteUser}@${RemoteHost}", $Command)

    Write-Verbose "ssh $($sshArgs -join ' ')"
    & ssh @sshArgs
    if ($LASTEXITCODE -ne 0) {
        throw "ssh command failed with exit code $LASTEXITCODE.  Command: $Command"
    }
}

# ---------------------------------------------------------------------------
# Step 0 — Validate inputs
# ---------------------------------------------------------------------------

Write-Step 'Validating inputs'

$resolvedBundle = $null
try {
    $resolvedBundle = (Resolve-Path -LiteralPath $BundlePath).Path
} catch {
    throw "bundle not found: $BundlePath"
}

if (-not $resolvedBundle.EndsWith('.zip', [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "bundle must be a .zip file, got: $resolvedBundle"
}

$bundleItem = Get-Item -LiteralPath $resolvedBundle
$bundleFileName = $bundleItem.Name
$bundleSizeBytes = $bundleItem.Length

Write-StepOk "Bundle: $resolvedBundle ($([math]::Round($bundleSizeBytes / 1MB, 2)) MB)"
Write-StepOk "Remote target: ${RemoteUser}@${RemoteHost}"

# Warn if ssh or scp are not on PATH — fail fast before touching the network.
foreach ($tool in 'ssh', 'scp') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "$tool not found on PATH — install OpenSSH client before running this script"
    }
}

# ---------------------------------------------------------------------------
# Step 1 — Ensure remote temp directory exists
# ---------------------------------------------------------------------------

Write-Step "Ensuring remote temp directory: $RemoteTempDir"

# Use cmd.exe syntax because the remote shell is determined by the sshd
# configuration and may be cmd.exe by default on Windows Server.
# We switch to pwsh immediately in subsequent steps.
Invoke-Ssh "if not exist `"$RemoteTempDir`" mkdir `"$RemoteTempDir`""

Write-StepOk 'Remote temp directory ready'

# ---------------------------------------------------------------------------
# Step 2 — Copy bundle via SCP
# ---------------------------------------------------------------------------

$remoteZipPath = "$RemoteTempDir\$bundleFileName"

Write-Step "Copying bundle to ${RemoteUser}@${RemoteHost}:${remoteZipPath}"

$scpArgs = Build-SshArgs
$scpArgs += @($resolvedBundle, "${RemoteUser}@${RemoteHost}:$remoteZipPath")

Write-Verbose "scp $($scpArgs -join ' ')"
& scp @scpArgs
if ($LASTEXITCODE -ne 0) {
    throw "SCP transfer failed with exit code $LASTEXITCODE"
}

Write-StepOk 'Bundle transferred'

# ---------------------------------------------------------------------------
# Step 3 — Expand and install on remote machine via SSH (pwsh inline script)
# ---------------------------------------------------------------------------

Write-Step 'Expanding bundle and running install on remote machine'

$installArgs = if ($Force) { '-Force' } else { '' }
if ($RemoteInstallRoot) {
    $installArgs = "$installArgs -InstallRoot '$RemoteInstallRoot'"
}
$installArgs = $installArgs.Trim()

# The here-string is passed as a single -Command argument.  We use single
# backtick-escaped dollars so the variables are evaluated on the REMOTE side.
$remoteInstallScript = @"
Set-StrictMode -Version Latest
`$ErrorActionPreference = 'Stop'

`$bundlePath = '$remoteZipPath'
`$expandDir  = Join-Path '$RemoteTempDir' 'expanded'

if (Test-Path -LiteralPath `$expandDir) {
    Remove-Item -LiteralPath `$expandDir -Recurse -Force
}

Expand-Archive -LiteralPath `$bundlePath -DestinationPath `$expandDir -Force
Write-Host 'Bundle expanded'

`$installScript = Get-ChildItem -Path `$expandDir -Filter 'Install-IronRdpPackage.ps1' -Recurse -ErrorAction SilentlyContinue |
    Select-Object -First 1

if (-not `$installScript) {
    throw 'Install-IronRdpPackage.ps1 not found in bundle'
}

Write-Host "Running: `$(`$installScript.FullName) $installArgs"
& `$installScript.FullName $installArgs
if (`$LASTEXITCODE -and `$LASTEXITCODE -ne 0) {
    throw "Install script exited with code `$LASTEXITCODE"
}
Write-Host 'Install completed'
"@

$sshCommonArgs = Build-SshArgs
$sshCommonArgs += @("${RemoteUser}@${RemoteHost}")

Write-Verbose "Remote install script:`n$remoteInstallScript"
& ssh @sshCommonArgs "pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -Command $remoteInstallScript"
if ($LASTEXITCODE -ne 0) {
    throw "Remote install step failed with exit code $LASTEXITCODE"
}

Write-StepOk 'Remote install completed'

# ---------------------------------------------------------------------------
# Step 4 — Basic smoke test on remote (binary probe via pwsh)
# ---------------------------------------------------------------------------

Write-Step 'Running binary probe smoke test on remote machine'

$remoteSmokeScript = @"
Set-StrictMode -Version Latest
`$ErrorActionPreference = 'Stop'

`$expandDir = Join-Path '$RemoteTempDir' 'expanded'
`$smokeScript = Get-ChildItem -Path `$expandDir -Filter 'Invoke-IronRdpSmokeTest.ps1' -Recurse -ErrorAction SilentlyContinue |
    Select-Object -First 1

if (-not `$smokeScript) {
    Write-Warning 'Invoke-IronRdpSmokeTest.ps1 not found in bundle — skipping binary probe'
    return
}

Write-Host "Running: `$(`$smokeScript.FullName)"
`$result = & `$smokeScript.FullName
`$result | ConvertTo-Json -Depth 6 -Compress
"@

Write-Verbose "Remote smoke script:`n$remoteSmokeScript"
$remoteSmokeOutput = & ssh @sshCommonArgs "pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -Command $remoteSmokeScript"
if ($LASTEXITCODE -ne 0) {
    throw "Remote binary probe failed with exit code $LASTEXITCODE"
}

$remoteSmokeResult = $null
try {
    # The last non-empty line should be the JSON result object.
    $jsonLine = ($remoteSmokeOutput | Where-Object { $_ -match '^\{' } | Select-Object -Last 1)
    if ($jsonLine) {
        $remoteSmokeResult = $jsonLine | ConvertFrom-Json
    }
} catch {
    Write-Warning "Could not parse remote smoke test JSON output: $_"
}

Write-StepOk "Remote binary probe passed"

# ---------------------------------------------------------------------------
# Step 5 — Optional live-connect smoke test (run from remote against $SmokeTestHost)
# ---------------------------------------------------------------------------

$liveConnectResult = $null

if ($SmokeTestHost) {
    Write-Step "Running live-connect smoke test: remote -> $SmokeTestHost (${SmokeTestConnectSeconds}s)"

    $smokeCredArgs = ''
    if ($SmokeTestUsername) { $smokeCredArgs += " -Username '$SmokeTestUsername'" }
    if ($SmokeTestPassword) { $smokeCredArgs += " -Password '$SmokeTestPassword'" }
    $smokeCredArgs = $smokeCredArgs.Trim()

    $remoteLiveScript = @"
Set-StrictMode -Version Latest
`$ErrorActionPreference = 'Stop'

`$expandDir = Join-Path '$RemoteTempDir' 'expanded'
`$smokeScript = Get-ChildItem -Path `$expandDir -Filter 'Invoke-IronRdpSmokeTest.ps1' -Recurse -ErrorAction SilentlyContinue |
    Select-Object -First 1

if (-not `$smokeScript) {
    throw 'Invoke-IronRdpSmokeTest.ps1 not found in bundle — cannot run live-connect test'
}

Write-Host "Running live-connect smoke test: `$(`$smokeScript.FullName) -LaunchHost '$SmokeTestHost' -ConnectSeconds $SmokeTestConnectSeconds $smokeCredArgs"
`$result = & `$smokeScript.FullName -LaunchHost '$SmokeTestHost' -ConnectSeconds $SmokeTestConnectSeconds $smokeCredArgs
`$result | ConvertTo-Json -Depth 6 -Compress
"@

    Write-Verbose "Remote live-connect script:`n$remoteLiveScript"
    $liveOutput = & ssh @sshCommonArgs "pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -Command $remoteLiveScript"
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "Live-connect smoke test exited with code $LASTEXITCODE on remote (session may still have connected)"
    }

    try {
        $jsonLine = ($liveOutput | Where-Object { $_ -match '^\{' } | Select-Object -Last 1)
        if ($jsonLine) {
            $liveConnectResult = $jsonLine | ConvertFrom-Json
        }
    } catch {
        Write-Warning "Could not parse live-connect result JSON: $_"
    }

    if ($liveConnectResult) {
        $statusColor = if ($liveConnectResult.status -in 'session-rendering', 'session-active', 'connected-no-frame') {
            'Green'
        } else {
            'Yellow'
        }
        Write-Host "    Live-connect status: $($liveConnectResult.status)" -ForegroundColor $statusColor
    }
}

# ---------------------------------------------------------------------------
# Step 6 — Structured result output
# ---------------------------------------------------------------------------

$deployResult = [pscustomobject]@{
    remoteHost       = $RemoteHost
    remoteUser       = $RemoteUser
    bundlePath       = $resolvedBundle
    bundleFileName   = $bundleFileName
    bundleSizeBytes  = $bundleSizeBytes
    deployTimestamp  = (Get-Date -Format 'o')
    installResult    = 'success'
    remoteSmokeTest  = $remoteSmokeResult
    liveConnectTest  = $liveConnectResult
}

Write-Host ''
Write-Host 'Deployment result:' -ForegroundColor White
$deployResult | Format-List | Out-String | Write-Host

$deployResult
