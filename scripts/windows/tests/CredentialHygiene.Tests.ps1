Describe 'Windows tooling credential hygiene' {
    BeforeAll {
        $repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..')).Path
        $windowsScripts = Get-ChildItem -LiteralPath (Join-Path $repositoryRoot 'scripts\windows') -Filter '*.ps1'
        $buildScript = Get-Item -LiteralPath (Join-Path $repositoryRoot 'build.ps1')
        $workflow = Get-Item -LiteralPath (Join-Path $repositoryRoot '.github\workflows\windows-release.yml')
    }

    It 'does not declare plaintext password parameters in Windows scripts' {
        foreach ($file in @($buildScript) + @($windowsScripts)) {
            Get-Content -LiteralPath $file.FullName -Raw |
                Should -Not -Match '\[string\]\s*\$\w*Password\b' -Because $file.FullName
        }
    }

    It 'does not pass passwords through native process arguments' {
        foreach ($file in @($buildScript) + @($windowsScripts)) {
            $content = Get-Content -LiteralPath $file.FullName -Raw
            $content | Should -Not -Match '--password(?!-stdin)' -Because $file.FullName
            $content | Should -Not -Match '(?im)^.*\bcmdkey(?:\.exe)?\b.*\/(?:pass|p)\b' -Because $file.FullName
            $content | Should -Not -Match '(?im)^.*\bschtasks(?:\.exe)?\b.*\/rp\b' -Because $file.FullName
            $content | Should -Not -Match '(?im)^.*\bsigntool(?:\.exe)?\b.*\/p\b' -Because $file.FullName
        }
    }

    It 'does not publish a signing password as a workflow output' {
        Get-Content -LiteralPath $workflow.FullName -Raw |
            Should -Not -Match '(?im)GITHUB_OUTPUT.*password|outputs\.password'
    }

    It 'deletes temporary signing certificates with their private keys' {
        $installerScript = Get-Content -LiteralPath (Join-Path $repositoryRoot 'scripts\windows\New-IronRdpInstallers.ps1') -Raw
        $installerScript | Should -Match 'Remove-Item\s+-LiteralPath\s+\$certificateStorePath\s+-DeleteKey\s+-Force\s+-ErrorAction\s+Stop'
        $installerScript | Should -Not -Match 'Remove-Item\s+-LiteralPath\s+"Cert:\\CurrentUser\\My'
    }

    It 'uses the private standard-input channel for unattended RDP tests' {
        Get-Content -LiteralPath (Join-Path $repositoryRoot 'scripts\windows\Invoke-IronRdpSmokeTest.ps1') -Raw |
            Should -Match '--password-stdin'
        Get-Content -LiteralPath (Join-Path $repositoryRoot 'scripts\windows\Invoke-HyperVE2ESuite.ps1') -Raw |
            Should -Match '--password-stdin'
    }

    It 'fails closed around installer and non-interactive Hyper-V orchestration' {
        $content = Get-Content -LiteralPath $buildScript.FullName -Raw
        $content | Should -Match '\$global:LASTEXITCODE\s*=\s*0\s*\r?\n\s*&\s*\$installerScript'
        $content | Should -Match 'installer generation failed with exit code'
        $content | Should -Match '\[Console\]::IsInputRedirected'
        $content | Should -Match '\[Environment\]::UserInteractive'
        $content | Should -Match 'pass -HyperVCredential'
    }
}
