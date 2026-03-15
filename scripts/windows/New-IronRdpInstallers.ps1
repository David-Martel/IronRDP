[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$PackageRoot,

    [Parameter(Mandatory)]
    [string]$OutputRoot,

    [string]$Publisher = 'CN=David-Martel IronRDP Test',
    [string]$CertificatePath,
    [string]$CertificatePassword,
    [string]$OutputJsonPath,
    [string]$ReleaseRepo,
    [string]$ReleaseTag,
    [switch]$SkipMsix,
    [switch]$SkipMsi
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Resolve-SdkTool {
    param([Parameter(Mandatory)][string]$ToolName)

    $sdkBinRoot = 'C:\Program Files (x86)\Windows Kits\10\bin'
    $versionedTool = Get-ChildItem -LiteralPath $sdkBinRoot -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
        Sort-Object Name -Descending |
        ForEach-Object {
            Join-Path $_.FullName "x64\$ToolName"
        } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -First 1

    if ($versionedTool) {
        return $versionedTool
    }

    $fallback = Join-Path $sdkBinRoot "x64\$ToolName"
    if (Test-Path -LiteralPath $fallback) {
        return $fallback
    }

    throw "required Windows SDK tool not found: $ToolName"
}

function Ensure-WixTools {
    $installedWixBin = Get-ChildItem 'C:\Program Files (x86)' -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like 'WiX Toolset v3.*' } |
        Sort-Object Name -Descending |
        ForEach-Object { Join-Path $_.FullName 'bin' } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -First 1

    if ($installedWixBin) {
        return [pscustomobject]@{
            Candle = Join-Path $installedWixBin 'candle.exe'
            Light = Join-Path $installedWixBin 'light.exe'
            Heat = Join-Path $installedWixBin 'heat.exe'
        }
    }

    $candle = Get-Command candle.exe -ErrorAction SilentlyContinue
    $light = Get-Command light.exe -ErrorAction SilentlyContinue
    $heat = Get-Command heat.exe -ErrorAction SilentlyContinue

    if ($candle -and $light -and $heat) {
        return [pscustomobject]@{
            Candle = $candle.Source
            Light = $light.Source
            Heat = $heat.Source
        }
    }

    if (-not (Get-Command choco.exe -ErrorAction SilentlyContinue)) {
        throw 'WiX Toolset is required for MSI packaging and Chocolatey is unavailable for automatic install'
    }

    Write-Host 'Installing WiX Toolset via Chocolatey' -ForegroundColor Cyan
    & choco install wixtoolset -y --no-progress | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw 'failed to install WiX Toolset'
    }

    $wixBin = Get-ChildItem 'C:\Program Files (x86)' -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like 'WiX Toolset v3.*' } |
        Sort-Object Name -Descending |
        ForEach-Object { Join-Path $_.FullName 'bin' } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -First 1

    if (-not $wixBin) {
        throw 'WiX Toolset installation did not expose a supported bin directory'
    }

    return [pscustomobject]@{
        Candle = Join-Path $wixBin 'candle.exe'
        Light = Join-Path $wixBin 'light.exe'
        Heat = Join-Path $wixBin 'heat.exe'
    }
}

function ConvertTo-AppInstallerVersion {
    param([Parameter(Mandatory)][string]$VersionText)

    $parts = $VersionText -split '\.'
    if ($parts.Count -lt 4) {
        $parts = @($parts + @('0', '0', '0', '0'))[0..3]
    }

    return ($parts[0..3] -join '.')
}

function ConvertTo-MsiVersion {
    param([Parameter(Mandatory)][string]$VersionText)

    $parts = $VersionText -split '\.'
    if ($parts.Count -ge 4) {
        return '{0}.{1}.{2}' -f $parts[0], $parts[1], $parts[3]
    }

    if ($parts.Count -eq 3) {
        return $VersionText
    }

    if ($parts.Count -eq 2) {
        return '{0}.{1}.0' -f $parts[0], $parts[1]
    }

    return '{0}.0.0' -f $parts[0]
}

function Sanitize-Xml {
    param([Parameter(Mandatory)][string]$Value)

    return [System.Security.SecurityElement]::Escape($Value)
}

function Ensure-LauncherFiles {
    param([Parameter(Mandatory)][string]$InstallRoot)

    $launcherPs1 = Join-Path $InstallRoot 'Start-IronRdpClient.ps1'
    $launcherCmd = Join-Path $InstallRoot 'ironrdp-client.cmd'

    $launcherScript = @'
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments
)

$clientExe = Join-Path $PSScriptRoot 'client\ironrdp-client.exe'
& $clientExe @Arguments
exit $LASTEXITCODE
'@

    Set-Content -LiteralPath $launcherPs1 -Value $launcherScript -Encoding UTF8

    $cmdScript = "@echo off`r`nset SCRIPT_DIR=%~dp0`r`npwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File `"%SCRIPT_DIR%Start-IronRdpClient.ps1`" %*`r`n"
    Set-Content -LiteralPath $launcherCmd -Value $cmdScript -Encoding ASCII
}

function Initialize-InstallerStage {
    param(
        [Parameter(Mandatory)][string]$SourceRoot,
        [Parameter(Mandatory)][string]$StageRoot
    )

    if (Test-Path -LiteralPath $StageRoot) {
        Remove-Item -LiteralPath $StageRoot -Recurse -Force
    }

    New-Item -ItemType Directory -Force -Path $StageRoot | Out-Null
    foreach ($item in (Get-ChildItem -LiteralPath $SourceRoot -Force)) {
        Copy-Item -LiteralPath $item.FullName -Destination $StageRoot -Recurse -Force
    }

    Ensure-LauncherFiles -InstallRoot $StageRoot
    return $StageRoot
}

function New-AppAssets {
    param([Parameter(Mandatory)][string]$AssetsRoot)

    Add-Type -AssemblyName System.Drawing
    New-Item -ItemType Directory -Force -Path $AssetsRoot | Out-Null

    $specs = @(
        @{ Name = 'Square44x44Logo.png'; Size = 44 },
        @{ Name = 'Square150x150Logo.png'; Size = 150 },
        @{ Name = 'StoreLogo.png'; Size = 50 }
    )

    foreach ($spec in $specs) {
        $outputPath = Join-Path $AssetsRoot $spec.Name
        $bitmap = [System.Drawing.Bitmap]::new($spec.Size, $spec.Size)
        try {
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.Clear([System.Drawing.Color]::FromArgb(0x0B, 0x3A, 0x53))
                $brush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(0xF2, 0xF5, 0xF7))
                try {
                    $fontSize = [Math]::Max(12, [Math]::Floor($spec.Size / 3.2))
                    $font = [System.Drawing.Font]::new('Segoe UI', [float]$fontSize, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
                    try {
                        $format = [System.Drawing.StringFormat]::new()
                        $format.Alignment = [System.Drawing.StringAlignment]::Center
                        $format.LineAlignment = [System.Drawing.StringAlignment]::Center
                        $graphics.DrawString('IR', $font, $brush, [System.Drawing.RectangleF]::new(0, 0, $spec.Size, $spec.Size), $format)
                    } finally {
                        $font.Dispose()
                    }
                } finally {
                    $brush.Dispose()
                }
            } finally {
                $graphics.Dispose()
            }

            $bitmap.Save($outputPath, [System.Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $bitmap.Dispose()
        }
    }
}

function New-SigningMaterial {
    param(
        [Parameter(Mandatory)][string]$OutputDirectory,
        [Parameter(Mandatory)][string]$RequestedPublisher,
        [string]$ProvidedCertificatePath,
        [string]$ProvidedCertificatePassword
    )

    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

    if (-not [string]::IsNullOrWhiteSpace($ProvidedCertificatePath)) {
        $certPassword = if ([string]::IsNullOrWhiteSpace($ProvidedCertificatePassword)) {
            $null
        } else {
            ConvertTo-SecureString -String $ProvidedCertificatePassword -AsPlainText -Force
        }

        $certificate = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new(
            (Resolve-Path -LiteralPath $ProvidedCertificatePath).Path,
            $ProvidedCertificatePassword
        )

        $cerPath = Join-Path $OutputDirectory 'IronRDP-signing.cer'
        Export-Certificate -Cert $certificate -FilePath $cerPath -Type CERT | Out-Null

        return @{
            Publisher = $certificate.Subject
            PfxPath = (Resolve-Path -LiteralPath $ProvidedCertificatePath).Path
            Password = $ProvidedCertificatePassword
            CerPath = $cerPath
            Temporary = $false
        }
    }

    $subject = $RequestedPublisher
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $subject -CertStoreLocation 'Cert:\CurrentUser\My' -NotAfter (Get-Date).AddYears(2)
    $passwordText = [Guid]::NewGuid().ToString('N')
    $password = ConvertTo-SecureString -String $passwordText -AsPlainText -Force
    $pfxPath = Join-Path $OutputDirectory 'IronRDP-test-signing.pfx'
    $cerPath = Join-Path $OutputDirectory 'IronRDP-test-signing.cer'
    Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $password | Out-Null
    Export-Certificate -Cert $cert -FilePath $cerPath -Type CERT | Out-Null

    return @{
        Publisher = $cert.Subject
        PfxPath = $pfxPath
        Password = $passwordText
        CerPath = $cerPath
        Temporary = $true
        Thumbprint = $cert.Thumbprint
    }
}

function New-MsixInstaller {
    param(
        [Parameter(Mandatory)][string]$StageRoot,
        [Parameter(Mandatory)][string]$OutputDirectory,
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string]$PublisherName,
        [Parameter(Mandatory)][string]$CertificatePath,
        [Parameter(Mandatory)][string]$CertificatePassword,
        [string]$ReleaseRepo,
        [string]$ReleaseTag
    )

    $makeAppx = Resolve-SdkTool -ToolName 'makeappx.exe'
    $signTool = Resolve-SdkTool -ToolName 'signtool.exe'

    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

    $layoutRoot = Join-Path $OutputDirectory 'layout'
    if (Test-Path -LiteralPath $layoutRoot) {
        Remove-Item -LiteralPath $layoutRoot -Recurse -Force
    }

    $appInstallRoot = Join-Path $layoutRoot 'VFS\ProgramFilesX64\IronRDP'
    New-Item -ItemType Directory -Force -Path $appInstallRoot | Out-Null
    foreach ($item in (Get-ChildItem -LiteralPath $StageRoot -Force)) {
        Copy-Item -LiteralPath $item.FullName -Destination $appInstallRoot -Recurse -Force
    }

    $assetsRoot = Join-Path $layoutRoot 'Assets'
    New-AppAssets -AssetsRoot $assetsRoot

    $manifestPath = Join-Path $layoutRoot 'AppxManifest.xml'
    $publisherXml = Sanitize-Xml -Value $PublisherName
    $displayPublisher = $PublisherName -replace '^CN=', ''
    $versionXml = Sanitize-Xml -Value $Version
    $manifest = @"
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:desktop6="http://schemas.microsoft.com/appx/manifest/desktop/windows10/6"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap desktop6 rescap">
  <Identity Name="DavidMartel.IronRDP" Publisher="$publisherXml" Version="$versionXml" />
  <Properties>
    <DisplayName>IronRDP</DisplayName>
    <PublisherDisplayName>$(Sanitize-Xml -Value $displayPublisher)</PublisherDisplayName>
    <Description>Windows-native IronRDP client package</Description>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Resources>
    <Resource Language="en-us" />
  </Resources>
  <Applications>
    <Application Id="IronRdpClient" Executable="VFS\ProgramFilesX64\IronRDP\client\ironrdp-client.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements
        DisplayName="IronRDP"
        Description="Windows-native IronRDP client"
        BackgroundColor="#0B3A53"
        Square150x150Logo="Assets\Square150x150Logo.png"
        Square44x44Logo="Assets\Square44x44Logo.png" />
    </Application>
  </Applications>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>
</Package>
"@
    Set-Content -Path $manifestPath -Value $manifest -Encoding UTF8

    $msixName = 'IronRDP.msix'
    $msixPath = Join-Path $OutputDirectory $msixName
    if (Test-Path -LiteralPath $msixPath) {
        Remove-Item -LiteralPath $msixPath -Force
    }

    & $makeAppx pack /d $layoutRoot /p $msixPath /o /nv | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw 'MakeAppx packaging failed'
    }

    & $signTool sign /fd SHA256 /f $CertificatePath /p $CertificatePassword $msixPath | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw 'SignTool signing failed for MSIX package'
    }

    $artifacts = @(
        [pscustomobject]@{
            kind = 'msix-package'
            path = $msixPath
        }
    )

    if (-not [string]::IsNullOrWhiteSpace($ReleaseRepo)) {
        $releaseSegment = if ([string]::IsNullOrWhiteSpace($ReleaseTag)) { 'latest' } else { "download/$ReleaseTag" }
        $baseUri = "https://github.com/$ReleaseRepo/releases/$releaseSegment"
        $appInstallerName = 'IronRDP.appinstaller'
        $appInstallerPath = Join-Path $OutputDirectory $appInstallerName
        $appInstaller = @"
<?xml version="1.0" encoding="utf-8"?>
<AppInstaller
  xmlns="http://schemas.microsoft.com/appx/appinstaller/2018"
  Uri="$baseUri/$appInstallerName"
  Version="$versionXml">
  <MainPackage
    Name="DavidMartel.IronRDP"
    Publisher="$publisherXml"
    Version="$versionXml"
    Uri="$baseUri/$msixName" />
  <UpdateSettings>
    <OnLaunch HoursBetweenUpdateChecks="0" ShowPrompt="true" UpdateBlocksActivation="false" />
    <ForceUpdateFromAnyVersion>true</ForceUpdateFromAnyVersion>
  </UpdateSettings>
</AppInstaller>
"@
        Set-Content -Path $appInstallerPath -Value $appInstaller -Encoding UTF8
        $artifacts += [pscustomobject]@{
            kind = 'appinstaller'
            path = $appInstallerPath
        }
    }

    return $artifacts
}

function New-MsiInstaller {
    param(
        [Parameter(Mandatory)][string]$StageRoot,
        [Parameter(Mandatory)][string]$OutputDirectory,
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string]$Manufacturer,
        [Parameter(Mandatory)][string]$CertificatePath,
        [Parameter(Mandatory)][string]$CertificatePassword
    )

    $wix = Ensure-WixTools
    $signTool = Resolve-SdkTool -ToolName 'signtool.exe'
    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

    $wixWorkDir = Join-Path $OutputDirectory 'wix'
    New-Item -ItemType Directory -Force -Path $wixWorkDir | Out-Null
    $sourceDir = Join-Path $wixWorkDir 'SourceDir'
    if (Test-Path -LiteralPath $sourceDir) {
        Remove-Item -LiteralPath $sourceDir -Recurse -Force
    }
    Copy-Item -LiteralPath $StageRoot -Destination $sourceDir -Recurse -Force

    $harvestPath = Join-Path $wixWorkDir 'Harvested.wxs'
    $productPath = Join-Path $wixWorkDir 'Product.wxs'
    $msiVersion = ConvertTo-MsiVersion -VersionText $Version
    $product = @"
<?xml version="1.0" encoding="UTF-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Product
    Id="*"
    Name="IronRDP"
    Language="1033"
    Version="$msiVersion"
    Manufacturer="$(Sanitize-Xml -Value ($Manufacturer -replace '^CN=', ''))"
    UpgradeCode="C6F54A89-5D7C-45CA-83B0-52C0AA0E6D42">
    <Package InstallerVersion="500" Compressed="yes" InstallScope="perMachine" Platform="x64" />
    <MajorUpgrade DowngradeErrorMessage="A newer version of IronRDP is already installed." />
    <MediaTemplate EmbedCab="yes" CompressionLevel="high" />
    <Feature Id="MainFeature" Title="IronRDP" Level="1">
      <ComponentGroupRef Id="ProductComponents" />
    </Feature>
    <Property Id="ARPNOREPAIR" Value="1" />
  </Product>

  <Fragment>
    <Directory Id="TARGETDIR" Name="SourceDir">
      <Directory Id="ProgramFiles64Folder">
        <Directory Id="INSTALLFOLDER" Name="IronRDP" />
      </Directory>
    </Directory>
  </Fragment>
</Wix>
"@
    Set-Content -Path $productPath -Value $product -Encoding UTF8

    $wixObjDir = Join-Path $wixWorkDir 'obj'
    New-Item -ItemType Directory -Force -Path $wixObjDir | Out-Null
    Push-Location $wixWorkDir
    try {
        & $wix.Heat dir 'SourceDir' -dr INSTALLFOLDER -cg ProductComponents -gg -sfrag -srd -platform x64 -out 'Harvested.wxs' | Out-Host
        if ($LASTEXITCODE -ne 0) {
            throw 'WiX heat harvest failed'
        }

        $harvestContent = Get-Content -LiteralPath $harvestPath -Raw
        $harvestContent = $harvestContent -replace '<Component Id="', '<Component Win64="yes" Id="'
        Set-Content -LiteralPath $harvestPath -Value $harvestContent -Encoding UTF8

        & $wix.Candle -out (Join-Path $wixObjDir '') $productPath $harvestPath | Out-Host
        if ($LASTEXITCODE -ne 0) {
            throw 'WiX candle compile failed'
        }

        $msiPath = Join-Path $OutputDirectory 'IronRDP.msi'
        & $wix.Light -b . -out $msiPath (Join-Path $wixObjDir 'Product.wixobj') (Join-Path $wixObjDir 'Harvested.wixobj') | Out-Host
        if ($LASTEXITCODE -ne 0) {
            throw 'WiX light link failed'
        }

        & $signTool sign /fd SHA256 /f $CertificatePath /p $CertificatePassword $msiPath | Out-Host
        if ($LASTEXITCODE -ne 0) {
            throw 'SignTool signing failed for MSI package'
        }
    } finally {
        Pop-Location
    }

    return [pscustomobject]@{
        kind = 'msi-package'
        path = $msiPath
    }
}

$resolvedPackageRoot = (Resolve-Path -LiteralPath $PackageRoot).Path
$resolvedOutputRoot = (Resolve-Path -LiteralPath (New-Item -ItemType Directory -Force -Path $OutputRoot)).Path
$manifest = Get-Content -LiteralPath (Join-Path $resolvedPackageRoot 'build-manifest.json') -Raw | ConvertFrom-Json
$version = ConvertTo-AppInstallerVersion -VersionText $manifest.version.FileVersion

$signingDir = Join-Path $resolvedOutputRoot 'certificates'
$signing = New-SigningMaterial -OutputDirectory $signingDir -RequestedPublisher $Publisher -ProvidedCertificatePath $CertificatePath -ProvidedCertificatePassword $CertificatePassword

$stageRoot = Initialize-InstallerStage -SourceRoot $resolvedPackageRoot -StageRoot (Join-Path $resolvedOutputRoot 'stage')
$artifacts = New-Object System.Collections.Generic.List[object]

if (-not $SkipMsix) {
    foreach ($artifact in (New-MsixInstaller -StageRoot $stageRoot -OutputDirectory (Join-Path $resolvedOutputRoot 'msix') -Version $version -PublisherName $signing.Publisher -CertificatePath $signing.PfxPath -CertificatePassword $signing.Password -ReleaseRepo $ReleaseRepo -ReleaseTag $ReleaseTag)) {
        $artifacts.Add($artifact)
    }
}

if (-not $SkipMsi) {
    $artifacts.Add((New-MsiInstaller -StageRoot $stageRoot -OutputDirectory (Join-Path $resolvedOutputRoot 'msi') -Version $version -Manufacturer $signing.Publisher -CertificatePath $signing.PfxPath -CertificatePassword $signing.Password))
}

$intermediatePaths = @(
    $stageRoot,
    (Join-Path $resolvedOutputRoot 'msix\layout'),
    (Join-Path $resolvedOutputRoot 'msi\wix')
)

foreach ($path in $intermediatePaths) {
    if (Test-Path -LiteralPath $path) {
        Remove-Item -LiteralPath $path -Recurse -Force
    }
}

if ($signing.Temporary -and (Test-Path -LiteralPath $signing.PfxPath)) {
    Remove-Item -LiteralPath $signing.PfxPath -Force
}

$artifacts.Add([pscustomobject]@{
    kind = 'signing-certificate'
    path = $signing.CerPath
})

$output = [pscustomobject]@{
    publisher = $signing.Publisher
    version = $version
    artifacts = $artifacts
}

$json = $output | ConvertTo-Json -Depth 5
if (-not [string]::IsNullOrWhiteSpace($OutputJsonPath)) {
    Set-Content -LiteralPath $OutputJsonPath -Value $json -Encoding UTF8
}

$json
