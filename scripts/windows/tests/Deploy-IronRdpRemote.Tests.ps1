Describe 'Deploy-IronRdpRemote SSH policy' {
    BeforeAll {
        $scriptPath = Join-Path $PSScriptRoot '..\Deploy-IronRdpRemote.ps1'
        $bundlePath = Join-Path $TestDrive 'bundle.zip'
        $keyPath = Join-Path $TestDrive 'id_ed25519'
        $knownHostsPath = Join-Path $TestDrive 'known_hosts'

        Set-Content -LiteralPath $bundlePath -Value 'validation-only placeholder'
        & ssh-keygen -q -t ed25519 -N '' -f $keyPath
        if ($LASTEXITCODE -ne 0) {
            throw 'failed to create an ephemeral SSH key for the test'
        }

        $publicKey = (Get-Content -LiteralPath "$keyPath.pub" -Raw).Trim().Split(' ')
        Set-Content -LiteralPath $knownHostsPath -Value "ironrdp-test $($publicKey[0]) $($publicKey[1])"
    }

    It 'validates a pinned host with explicit key-only authentication' {
        $result = & $scriptPath `
            -BundlePath $bundlePath `
            -RemoteHost 'example.invalid' `
            -RemoteUser 'ironrdp-test' `
            -SshKeyPath $keyPath `
            -UserKnownHostsFile $knownHostsPath `
            -HostKeyAlias 'ironrdp-test' `
            -ValidateSshConfigurationOnly

        $result.validated | Should -BeTrue
        $result.hostKeyAlias | Should -Be 'ironrdp-test'
        $result.sshKeyPath | Should -Be (Resolve-Path -LiteralPath $keyPath).Path
        $result.userKnownHostsFile | Should -Be (Resolve-Path -LiteralPath $knownHostsPath).Path
    }

    It 'fails closed when the pinned alias is absent' {
        {
            & $scriptPath `
                -BundlePath $bundlePath `
                -RemoteHost 'example.invalid' `
                -RemoteUser 'ironrdp-test' `
                -SshKeyPath $keyPath `
                -UserKnownHostsFile $knownHostsPath `
                -HostKeyAlias 'unexpected-host' `
                -ValidateSshConfigurationOnly
        } | Should -Throw "no pinned host key for 'unexpected-host'*"
    }

    It 'rejects an option-like SSH username' {
        {
            & $scriptPath `
                -BundlePath $bundlePath `
                -RemoteHost 'example.invalid' `
                -RemoteUser '-oProxyCommand=unexpected' `
                -SshKeyPath $keyPath `
                -UserKnownHostsFile $knownHostsPath `
                -HostKeyAlias 'ironrdp-test' `
                -ValidateSshConfigurationOnly
        } | Should -Throw 'RemoteUser contains unsupported characters*'
    }
}
