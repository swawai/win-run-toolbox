$ErrorActionPreference = "Stop"

$script:RepoRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
$script:KitRoot = Join-Path $script:RepoRoot "_lib\ssh_remote_kit"

. (Join-Path $script:KitRoot "ps_common.ps1")
. (Join-Path $script:KitRoot "key_manager.openssh.ps1")

function Assert-True {
    param(
        [Parameter(Mandatory=$true)] [bool]$Condition,
        [Parameter(Mandatory=$true)] [string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Contains {
    param(
        [Parameter(Mandatory=$true)] [string]$Text,
        [Parameter(Mandatory=$true)] [string]$Expected,
        [Parameter(Mandatory=$true)] [string]$Message
    )

    Assert-True ($Text.Contains($Expected)) $Message
}

function Initialize-DummyContext {
    param(
        [string]$SshConfigPath = "",
        [string]$SshHostAlias = ""
    )

    $params = @{
        Port = 2222
        RemoteHost = "example.invalid"
        RemoteUser = "root"
        SshKeyPath = "C:\keys with spaces\id_rsa"
        ModuleRoot = $script:KitRoot
        UploadSubdir = "smoke_key_manager"
    }

    if (-not [string]::IsNullOrWhiteSpace($SshConfigPath)) {
        $params.SshConfigPath = $SshConfigPath
    }

    if (-not [string]::IsNullOrWhiteSpace($SshHostAlias)) {
        $params.SshHostAlias = $SshHostAlias
    }

    [void](Initialize-RemoteKitContext `
        @params)
}

function Test-PayloadEmbedsHelperAndPubkeyForSingleSshConnection {
    $helper = "#!/usr/bin/env bash`necho helper`n"
    $pubkey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAISMOKE smoke@example"

    $payload = New-RemoteKitKeyManagerOpenSshPayload `
        -Action "add" `
        -SshdMode "check-sshd" `
        -HelperContent $helper `
        -PublicKeyLine $pubkey `
        -Token "SMOKE_TOKEN"

    Assert-Contains $payload "cat > `"`$script_path`" <<'REMOTE_KIT_SMOKE_TOKEN_SCRIPT'" "payload should write helper script via heredoc."
    Assert-Contains $payload "cat > `"`$pubkey_path`" <<'REMOTE_KIT_SMOKE_TOKEN_PUBKEY'" "payload should write public key via heredoc."
    Assert-Contains $payload $pubkey "payload should contain the public key line."
    Assert-Contains $payload "bash `"`$script_path`" 'add' `"`$pubkey_path`" 'check-sshd'" "payload should execute helper with add action."
    Assert-Contains $payload '${TMPDIR:-/tmp}/swaw-kit-ssh-remote.XXXXXXXXXX' "payload should use the canonical remote temp namespace."
    Assert-Contains $payload "trap cleanup EXIT" "payload should clean remote temp files."
    Assert-True (-not $payload.Contains("`r")) "payload should use LF line endings."
}

function Test-PasswordBootstrapArgsUseOneInteractiveSshCommand {
    Initialize-DummyContext

    $args = @(New-RemoteKitKeyManagerOpenSshArgs -PasswordBootstrap)
    $joined = $args -join " "

    Assert-Contains $joined "-o BatchMode=no" "password bootstrap should allow password prompts."
    Assert-Contains $joined "-o PubkeyAuthentication=no" "password bootstrap should force password path for the target."
    Assert-Contains $joined "-o PreferredAuthentications=password,keyboard-interactive" "password bootstrap should prefer password and keyboard-interactive auth."
    Assert-Contains $joined "-p 2222" "password bootstrap should include configured port."
    Assert-Contains $joined "root@example.invalid" "password bootstrap should include configured remote target."
    Assert-True ($args[-1] -eq "bash -s") "password bootstrap should execute one remote bash stdin command."
    Assert-True (-not ($args -contains "-n")) "password bootstrap must not close stdin."
    Assert-True (-not $joined.Contains("BatchMode=yes")) "password bootstrap must not disable password prompts."
}

function Test-KeyAuthArgsKeepBatchModeButStillUseSingleSshCommand {
    Initialize-DummyContext

    $args = @(New-RemoteKitKeyManagerOpenSshArgs)
    $joined = $args -join " "

    Assert-Contains $joined "-o BatchMode=yes" "key auth should fail fast without prompting."
    Assert-True (-not $joined.Contains("PubkeyAuthentication=no")) "key auth should not disable public-key auth."
    Assert-True ($args[-1] -eq "bash -s") "key auth should execute one remote bash stdin command."
}

function Test-ConfigHostArgsUseConfigAliasWithoutDirectOverrides {
    Initialize-DummyContext -SshConfigPath "D:\repo data\vps1.config" -SshHostAlias "vps1"

    $args = @(New-RemoteKitKeyManagerOpenSshArgs -PasswordBootstrap)
    $joined = $args -join " "

    Assert-Contains $joined "-F D:\repo data\vps1.config" "config host mode should pass the generated ssh config."
    Assert-Contains $joined "vps1 bash -s" "config host mode should use the Host alias as target."
    Assert-True (-not ($args -contains "-i")) "config host mode should not override IdentityFile with -i."
    Assert-True (-not ($args -contains "-p")) "config host mode should not override Port with -p."
    Assert-True (-not $joined.Contains("StrictHostKeyChecking")) "config host mode should not override host-key policy from ssh_config."
    Assert-True (-not $joined.Contains("root@example.invalid")) "config host mode should not build user@host target."
}

function Test-SelectIdentityFileFromOpenSshEffectiveConfig {
    $effective = @'
user userA
hostname A.example.invalid
identityfile none
identityfile ~/.ssh/id_vps1
identityfile C:/Users/Smoke User/.ssh/fallback
'@

    $selected = Select-RemoteKitOpenSshIdentityFile `
        -EffectiveConfigText $effective `
        -UserProfile "C:\Users\Smoke User"

    Assert-True ($selected -eq "C:\Users\Smoke User\.ssh\id_vps1") "key manager should select the first usable OpenSSH identityfile and expand ~."
}

function Test-PersistentArtifactsUseCanonicalNames {
    $helperSource = [System.IO.File]::ReadAllText((Join-Path $script:KitRoot "authorized_keys.sh"))
    Assert-Contains $helperSource "/etc/ssh/sshd_config.d/00-swaw-kit-ssh-remote-pubkey-auth.conf" "the remote sshd drop-in should use the canonical swaw-kit name."
    Assert-Contains $helperSource "# Managed by swaw-kit SSH Remote key.fix/key.add.fix" "the remote sshd marker should use the canonical swaw-kit owner."
    Assert-Contains $helperSource ".swaw-kit-ssh-remote-backup-" "persistent remote backups should use the canonical swaw-kit namespace."
}

try {
    Test-PayloadEmbedsHelperAndPubkeyForSingleSshConnection
    Test-PasswordBootstrapArgsUseOneInteractiveSshCommand
    Test-KeyAuthArgsKeepBatchModeButStillUseSingleSshCommand
    Test-ConfigHostArgsUseConfigAliasWithoutDirectOverrides
    Test-SelectIdentityFileFromOpenSshEffectiveConfig
    Test-PersistentArtifactsUseCanonicalNames
    Write-Host "ssh remote kit key-manager smoke ok" -ForegroundColor Green
} finally {
    $ctx = $null
    try { $ctx = Get-RemoteKitContext } catch { }
    if ($ctx -and (Test-Path -LiteralPath $ctx.UploadTempRoot)) {
        Remove-Item -LiteralPath $ctx.UploadTempRoot `
            -Recurse `
            -Force `
            -ErrorAction SilentlyContinue
    }
}
