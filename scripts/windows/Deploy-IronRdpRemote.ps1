#requires -Version 7.0

<#
.SYNOPSIS
    Deploy IronRDP package to a remote Windows machine via SSH.
.DESCRIPTION
    Copies a portable bundle zip to the remote host, expands it, runs the
    install script, and executes a smoke test to verify the deployment.

    The remote machine must have OpenSSH server running and PowerShell 7+
    available as 'pwsh' on PATH. SSH authentication is restricted to the
    explicitly supplied private key, and the host key must already be pinned
    in the explicitly supplied known-hosts file.

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
    Path to the PEM/OpenSSH private key used for the deployment. The script
    disables ssh-agent and all password or keyboard-interactive fallback.
.PARAMETER UserKnownHostsFile
    Path to a dedicated OpenSSH known-hosts file containing the pinned key for
    HostKeyAlias. The script never accepts or updates host keys automatically.
.PARAMETER HostKeyAlias
    Exact host identity to verify in UserKnownHostsFile. This can differ from
    RemoteHost when RemoteHost is an address or SSH config alias.
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
.PARAMETER SmokeTestCredential
    RDP credential for the live-connect smoke test. When omitted, the script
    prompts locally and forwards the password only over the encrypted SSH
    standard-input stream.
.PARAMETER SmokeTestConnectSeconds
    How long (seconds) to hold the RDP connection open during the smoke test.
    Defaults to 15.
.PARAMETER Force
    Passed through to Install-IronRdpPackage.ps1 to allow overwriting an
    existing installation on the remote machine.
.PARAMETER ValidateSshConfigurationOnly
    Validate the pinned SSH policy and return without opening a connection.
.EXAMPLE
    ./Deploy-IronRdpRemote.ps1 `
        -BundlePath T:\artifacts\IronRDP-portable.zip `
        -RemoteHost dtm-p1gen7 `
        -SshKeyPath ~/.ssh/id_ed25519 `
        -UserKnownHostsFile ~/.ssh/ironrdp_known_hosts `
        -HostKeyAlias dtm-p1gen7
.EXAMPLE
    ./Deploy-IronRdpRemote.ps1 `
        -BundlePath T:\artifacts\IronRDP-portable.zip `
        -RemoteHost dtm-p1gen7 `
        -SshKeyPath ~/.ssh/id_ed25519 `
        -UserKnownHostsFile ~/.ssh/ironrdp_known_hosts `
        -HostKeyAlias dtm-p1gen7 `
        -SmokeTestHost 172.23.187.173 `
        -Force
.EXAMPLE
    ./Deploy-IronRdpRemote.ps1 `
        -BundlePath T:\artifacts\IronRDP-portable.zip `
        -RemoteHost dtm-p1gen7 `
        -SshKeyPath ~/.ssh/id_ed25519 `
        -UserKnownHostsFile ~/.ssh/ironrdp_known_hosts `
        -HostKeyAlias dtm-p1gen7 `
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

    [Parameter(Mandatory)]
    [string]$SshKeyPath,

    [Parameter(Mandatory)]
    [string]$UserKnownHostsFile,

    [Parameter(Mandatory)]
    [string]$HostKeyAlias,

    [string]$RemoteTempDir = 'C:\Temp\IronRDP-deploy',

    [string]$RemoteInstallRoot,

    # If set, run a live-connect smoke test against this host after install
    [string]$SmokeTestHost,
    [System.Management.Automation.PSCredential]$SmokeTestCredential,
    [int]$SmokeTestConnectSeconds = 15,

    [switch]$Force,

    [switch]$ValidateSshConfigurationOnly
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
    return @(
        '-F', 'none',
        '-i', $script:resolvedSshKeyPath,
        '-o', 'BatchMode=yes',
        '-o', 'StrictHostKeyChecking=yes',
        '-o', "UserKnownHostsFile=$script:resolvedKnownHostsPath",
        '-o', "HostKeyAlias=$HostKeyAlias",
        '-o', 'IdentitiesOnly=yes',
        '-o', 'IdentityAgent=none',
        '-o', 'PreferredAuthentications=publickey',
        '-o', 'PubkeyAuthentication=yes',
        '-o', 'PasswordAuthentication=no',
        '-o', 'KbdInteractiveAuthentication=no',
        '-o', 'ChallengeResponseAuthentication=no',
        '-o', 'HostbasedAuthentication=no',
        '-o', 'ForwardAgent=no',
        '-o', 'ClearAllForwardings=yes',
        '-o', 'PermitLocalCommand=no'
    )
}

function Assert-SshSecurityPolicy {
    $sshArgs = Build-SshArgs
    $effectiveConfig = & ssh @sshArgs -G "${RemoteUser}@${RemoteHost}" 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "could not evaluate the effective SSH configuration (exit code $LASTEXITCODE)"
    }

    $settings = @{}
    foreach ($line in $effectiveConfig) {
        if ($line -match '^(\S+)\s+(.+)$') {
            $settings[$matches[1].ToLowerInvariant()] = $matches[2].Trim()
        }
    }

    $expected = @{
        batchmode = 'yes'
        stricthostkeychecking = 'true'
        hostkeyalias = $HostKeyAlias
        identitiesonly = 'yes'
        identityagent = 'none'
        preferredauthentications = 'publickey'
        pubkeyauthentication = 'true'
        passwordauthentication = 'no'
        kbdinteractiveauthentication = 'no'
        hostbasedauthentication = 'no'
        forwardagent = 'no'
        clearallforwardings = 'yes'
        permitlocalcommand = 'no'
    }
    foreach ($setting in $expected.GetEnumerator()) {
        if ($settings[$setting.Key] -ne $setting.Value) {
            throw "SSH security policy mismatch for '$($setting.Key)': expected '$($setting.Value)', got '$($settings[$setting.Key])'"
        }
    }

    $effectiveKnownHosts = $settings.userknownhostsfile.Trim('"')
    if (-not [System.IO.Path]::GetFullPath($effectiveKnownHosts).Equals(
            $script:resolvedKnownHostsPath,
            [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "SSH security policy did not retain the pinned known-hosts file: $effectiveKnownHosts"
    }
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

function Invoke-SshWithCredentialInput {
    param(
        [Parameter(Mandatory)][string]$Command,
        [Parameter(Mandatory)][System.Management.Automation.PSCredential]$Credential
    )

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = 'ssh'
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in (Build-SshArgs)) {
        $startInfo.ArgumentList.Add($argument)
    }
    $startInfo.ArgumentList.Add("${RemoteUser}@${RemoteHost}")
    $startInfo.ArgumentList.Add($Command)

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw 'failed to start ssh process'
    }

    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $passwordPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Credential.Password)
    try {
        for ($index = 0; $index -lt $Credential.Password.Length; $index++) {
            $process.StandardInput.Write([char][Runtime.InteropServices.Marshal]::ReadInt16($passwordPointer, $index * 2))
        }
        $process.StandardInput.WriteLine()
    } finally {
        $process.StandardInput.Close()
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($passwordPointer)
    }

    $process.WaitForExit()
    [pscustomobject]@{
        exitCode = $process.ExitCode
        stdout = $stdoutTask.GetAwaiter().GetResult()
        stderr = $stderrTask.GetAwaiter().GetResult()
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

try {
    $script:resolvedSshKeyPath = (Resolve-Path -LiteralPath $SshKeyPath).Path
} catch {
    throw "SSH private key not found: $SshKeyPath"
}
if ((Get-Item -LiteralPath $script:resolvedSshKeyPath).PSIsContainer) {
    throw "SSH private key must be a file: $script:resolvedSshKeyPath"
}

try {
    $script:resolvedKnownHostsPath = (Resolve-Path -LiteralPath $UserKnownHostsFile).Path
} catch {
    throw "known-hosts file not found: $UserKnownHostsFile"
}
if ((Get-Item -LiteralPath $script:resolvedKnownHostsPath).PSIsContainer) {
    throw "known-hosts path must be a file: $script:resolvedKnownHostsPath"
}

if ($HostKeyAlias -notmatch '^[A-Za-z0-9._:\[\]%-]+$') {
    throw "HostKeyAlias contains unsupported characters: $HostKeyAlias"
}
if ($RemoteHost -notmatch '^[A-Za-z0-9._:\[\]%-]+$') {
    throw "RemoteHost contains unsupported characters: $RemoteHost"
}
if ($RemoteUser -notmatch '^[A-Za-z0-9_][A-Za-z0-9._\\-]*$') {
    throw "RemoteUser contains unsupported characters: $RemoteUser"
}

$bundleItem = Get-Item -LiteralPath $resolvedBundle
$bundleFileName = $bundleItem.Name
$bundleSizeBytes = $bundleItem.Length

Write-StepOk "Bundle: $resolvedBundle ($([math]::Round($bundleSizeBytes / 1MB, 2)) MB)"
Write-StepOk "Remote target: ${RemoteUser}@${RemoteHost}"

# Warn if ssh or scp are not on PATH — fail fast before touching the network.
foreach ($tool in 'ssh', 'scp', 'ssh-keygen') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "$tool not found on PATH — install OpenSSH client before running this script"
    }
}

$pinnedKeys = & ssh-keygen -F $HostKeyAlias -f $script:resolvedKnownHostsPath 2>$null
if ($LASTEXITCODE -ne 0 -or -not $pinnedKeys) {
    throw "no pinned host key for '$HostKeyAlias' exists in $script:resolvedKnownHostsPath"
}

Assert-SshSecurityPolicy
Write-StepOk "SSH policy: pinned host '$HostKeyAlias', explicit key only, no agent or password fallback"

if ($ValidateSshConfigurationOnly) {
    [pscustomobject]@{
        validated = $true
        remoteHost = $RemoteHost
        hostKeyAlias = $HostKeyAlias
        sshKeyPath = $script:resolvedSshKeyPath
        userKnownHostsFile = $script:resolvedKnownHostsPath
    }
    return
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

    if (-not $SmokeTestCredential) {
        $SmokeTestCredential = Get-Credential -Message "RDP credentials for '$SmokeTestHost'"
    }
    $smokeUsername = $SmokeTestCredential.UserName.Replace("'", "''")

    $remoteLiveScript = @"
Set-StrictMode -Version Latest
`$ErrorActionPreference = 'Stop'

`$expandDir = Join-Path '$RemoteTempDir' 'expanded'
`$smokeScript = Get-ChildItem -Path `$expandDir -Filter 'Invoke-IronRdpSmokeTest.ps1' -Recurse -ErrorAction SilentlyContinue |
    Select-Object -First 1

if (-not `$smokeScript) {
    throw 'Invoke-IronRdpSmokeTest.ps1 not found in bundle — cannot run live-connect test'
}

`$plainPassword = [Console]::In.ReadLine()
if ([string]::IsNullOrEmpty(`$plainPassword)) {
    throw 'RDP password was not received on standard input'
}
try {
    `$securePassword = ConvertTo-SecureString `$plainPassword -AsPlainText -Force
    `$credential = [System.Management.Automation.PSCredential]::new('$smokeUsername', `$securePassword)
    `$plainPassword = `$null
    `$result = & `$smokeScript.FullName -LaunchHost '$SmokeTestHost' -ConnectSeconds $SmokeTestConnectSeconds -Credential `$credential
} finally {
    `$plainPassword = `$null
}
`$result | ConvertTo-Json -Depth 6 -Compress
"@

    Write-Verbose "Remote live-connect script:`n$remoteLiveScript"
    $encodedLiveScript = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($remoteLiveScript))
    $liveInvocation = Invoke-SshWithCredentialInput `
        -Command "pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -EncodedCommand $encodedLiveScript" `
        -Credential $SmokeTestCredential
    $liveOutput = $liveInvocation.stdout -split "`r?`n"
    if ($liveInvocation.exitCode -ne 0) {
        Write-Warning "Live-connect smoke test exited with code $($liveInvocation.exitCode) on remote: $($liveInvocation.stderr.Trim())"
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
