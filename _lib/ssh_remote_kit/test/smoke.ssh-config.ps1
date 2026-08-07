$ErrorActionPreference = "Stop"

$script:SmokeRepoRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
$script:KitRoot = Join-Path $script:SmokeRepoRoot "_lib\ssh_remote_kit"

. (Join-Path $script:KitRoot "ssh_config.ps1")

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

function Assert-NotContains {
    param(
        [Parameter(Mandatory=$true)] [string]$Text,
        [Parameter(Mandatory=$true)] [string]$Unexpected,
        [Parameter(Mandatory=$true)] [string]$Message
    )

    Assert-True (-not $Text.Contains($Unexpected)) $Message
}

function Assert-ThrowsLike {
    param(
        [Parameter(Mandatory=$true)] [scriptblock]$Action,
        [Parameter(Mandatory=$true)] [string]$Pattern,
        [Parameter(Mandatory=$true)] [string]$Message
    )

    try {
        & $Action
    } catch {
        Assert-True ($_.Exception.Message -like $Pattern) $Message
        return
    }

    throw $Message
}

function New-SmokeEntryFile {
    param(
        [Parameter(Mandatory=$true)] [string]$Root,
        [string]$Name = "vps1.cmd",
        [string]$HostName = "A.example.invalid",
        [AllowEmptyString()] [string]$RemoteShell = '',
        [string]$RemoteShellMacro = '___RemoteShell___'
    )

    if (-not (Test-Path -LiteralPath $Root -PathType Container)) {
        New-Item -ItemType Directory -Path $Root -Force | Out-Null
    }

    $entry = Join-Path $Root $Name
    $shellLine = if ($RemoteShell.Length -gt 0) {
        "  $RemoteShellMacro $RemoteShell # smoke profile"
    } else {
        ''
    }
    $content = @"
@echo off & chcp 65001 >nul & setlocal & goto :REMOTE_KIT_AFTER_SSH_CONFIG
:::::::::::::::::::::::::::::::::::::::::::::::::::
:: ignored cmd decoration
rem ignored cmd comment
  @REM ignored cmd comment too

# preserved OpenSSH comment
Host ___self___ # managed entry identity
$shellLine
  HostName $HostName
  User userA
  Port 2222
  IdentityFile ~/.ssh/id_vps1
  ProxyCommand ssh -W %h:%p bastion
  LocalCommand echo %USERPROFILE%

:::::::::::::::::::::::::::::::::::::::::::::::::::
:: ignored lower decoration
REM ignored lower comment
:REMOTE_KIT_AFTER_SSH_CONFIG
"@

    [System.IO.File]::WriteAllText($entry, $content, [System.Text.UTF8Encoding]::new($false))
    return $entry
}

function Test-RemoteShellMacro {
    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("remote-kit-shell-" + [guid]::NewGuid().ToString("N"))
    try {
        $defaultEntry = New-SmokeEntryFile $tempRoot -Name 'default.cmd'
        $defaultDocument = Read-RemoteKitEmbeddedSshConfigDocument $defaultEntry
        Assert-True `
            ($defaultDocument.RemoteShell -eq 'posix') `
            'missing remote-shell macro should default to posix.'

        $cmdEntry = New-SmokeEntryFile `
            $tempRoot `
            -Name 'cmd.cmd' `
            -RemoteShell 'WIN.CMD' `
            -RemoteShellMacro '___REMOTESHELL___'
        $cmdDocument = Read-RemoteKitEmbeddedSshConfigDocument $cmdEntry
        Assert-True `
            ($cmdDocument.RemoteShell -eq 'win.cmd') `
            'remote-shell profiles should be case-insensitive and canonicalized.'
        Assert-True ($cmdDocument.ConfigText -notmatch '___RemoteShell___') `
            'active remote-shell macros should be removed from generated OpenSSH config.'

        $reservedEntry = New-SmokeEntryFile `
            $tempRoot `
            -Name 'reserved.cmd' `
            -RemoteShell 'win.powershell'
        $reservedDocument = Read-RemoteKitEmbeddedSshConfigDocument $reservedEntry
        Assert-True `
            ($reservedDocument.RemoteShell -eq 'win.powershell') `
            'reserved remote-shell profiles should be recognized before implementation.'

        $unknownEntry = New-SmokeEntryFile `
            $tempRoot `
            -Name 'unknown.cmd' `
            -RemoteShell 'linux.bash'
        Assert-ThrowsLike {
            Read-RemoteKitEmbeddedSshConfigDocument $unknownEntry
        } "*Unknown remote shell profile 'linux.bash'*" `
            'unknown remote-shell profiles should fail clearly.'

        $duplicateEntry = New-InvalidSmokeEntryFile `
            (Join-Path $tempRoot 'duplicate') `
            @(
                'Host ___self___'
                '  ___RemoteShell___ posix'
                '  ___RemoteShell___ win.cmd'
                '  HostName A.example.invalid'
            )
        Assert-ThrowsLike {
            Read-RemoteKitEmbeddedSshConfigDocument $duplicateEntry
        } '*at most one active ___RemoteShell___ directive*' `
            'duplicate active remote-shell macros should fail clearly.'

        $disabledEntry = New-InvalidSmokeEntryFile `
            (Join-Path $tempRoot 'disabled') `
            @(
                'Host ___self___'
                '  # ___RemoteShell___ win.cmd'
                '  HostName A.example.invalid'
            )
        $disabledDocument = Read-RemoteKitEmbeddedSshConfigDocument $disabledEntry
        Assert-True ($disabledDocument.RemoteShell -eq 'posix') `
            'commented remote-shell macros should be disabled.'
        Assert-NotContains $disabledDocument.ConfigText '___RemoteShell___' `
            'commented remote-shell macros should be removed from generated config.'

        $malformedEntry = New-InvalidSmokeEntryFile `
            (Join-Path $tempRoot 'malformed') `
            @(
                'Host ___self___'
                '  ___RemoteShell___'
                '  HostName A.example.invalid'
            )
        Assert-ThrowsLike {
            Read-RemoteKitEmbeddedSshConfigDocument $malformedEntry
        } '*Malformed ___RemoteShell___ directive*' `
            'malformed remote-shell macros should fail clearly.'
    } finally {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function New-InvalidSmokeEntryFile {
    param(
        [Parameter(Mandatory=$true)] [string]$Root,
        [Parameter(Mandatory=$true)] [string[]]$HostLines
    )

    New-Item -ItemType Directory -Path $Root -Force | Out-Null
    $entry = Join-Path $Root "invalid.cmd"
    $content = @(
        "@echo off & goto :REMOTE_KIT_AFTER_SSH_CONFIG"
        $HostLines
        ":REMOTE_KIT_AFTER_SSH_CONFIG"
    ) -join "`r`n"
    [System.IO.File]::WriteAllText($entry, $content, [System.Text.UTF8Encoding]::new($false))
    return $entry
}

function Test-EntryIdentityIsStableAndPathScoped {
    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("remote-kit-identity-" + [guid]::NewGuid().ToString("N"))
    try {
        $entryA = New-SmokeEntryFile (Join-Path $tempRoot "a")
        $entryB = New-SmokeEntryFile (Join-Path $tempRoot "b")
        $aliasA = Get-RemoteKitEntryHostAlias $entryA
        $aliasAAgain = Get-RemoteKitEntryHostAlias $entryA
        $aliasB = Get-RemoteKitEntryHostAlias $entryB

        Assert-True ($aliasA -match "^vps1-[0-9a-f]{12}$") "entry alias should be readable and use a 12-character lowercase hex suffix."
        Assert-True ($aliasA -eq $aliasAAgain) "the same entry path should always produce the same alias."
        Assert-True ($aliasA -ne $aliasB) "same-named entries in different directories should have different aliases."

        $changed = [System.IO.File]::ReadAllText($entryA).Replace("A.example.invalid", "B.example.invalid")
        [System.IO.File]::WriteAllText($entryA, $changed, [System.Text.UTF8Encoding]::new($false))
        Assert-True ((Get-RemoteKitEntryHostAlias $entryA) -eq $aliasA) "connection config changes should not change entry identity."
    } finally {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-ExtractPrettyEmbeddedConfig {
    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("remote-kit-extract-" + [guid]::NewGuid().ToString("N"))
    $oldUserProfile = $env:USERPROFILE
    try {
        $entry = New-SmokeEntryFile $tempRoot
        $env:USERPROFILE = "C:\Users\Must Not Expand"
        $alias = Get-RemoteKitEntryHostAlias $entry
        $text = Get-RemoteKitEmbeddedSshConfigText $entry

        Assert-Contains $text "Host $alias # managed entry identity" "self token should resolve to the derived entry alias."
        Assert-Contains $text "# preserved OpenSSH comment" "OpenSSH comments should be preserved."
        Assert-Contains $text "ProxyCommand ssh -W %h:%p bastion" "OpenSSH percent tokens should be preserved."
        Assert-Contains $text "LocalCommand echo %USERPROFILE%" "Windows environment variables should not be expanded implicitly."
        Assert-NotContains $text "___self___" "generated config should not retain the self token."
        Assert-NotContains $text "::" "CMD decoration comments should be omitted."
        Assert-NotContains $text "ignored cmd comment" "REM comments should be omitted."
        Assert-NotContains $text "REMOTE_KIT_AFTER_SSH_CONFIG" "container boundaries should be omitted."
        Assert-True (-not $text.Contains("`n`n")) "blank container lines should be omitted."
    } finally {
        $env:USERPROFILE = $oldUserProfile
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-SelfTokenValidation {
    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("remote-kit-self-" + [guid]::NewGuid().ToString("N"))
    try {
        $missing = New-InvalidSmokeEntryFile (Join-Path $tempRoot "missing") @(
            "Host other"
            "  HostName A.example.invalid"
        )
        Assert-ThrowsLike {
            Get-RemoteKitEmbeddedSshConfigText $missing
        } "*exactly one '___self___' token*" "missing self token should fail clearly."

        $duplicate = New-InvalidSmokeEntryFile (Join-Path $tempRoot "duplicate") @(
            "Host ___self___"
            "  HostName A.example.invalid"
            "Host ___self___"
            "  HostName B.example.invalid"
        )
        Assert-ThrowsLike {
            Get-RemoteKitEmbeddedSshConfigText $duplicate
        } "*exactly one '___self___' token*" "duplicate self tokens should fail clearly."

        $misplaced = New-InvalidSmokeEntryFile (Join-Path $tempRoot "misplaced") @(
            "Host other ___self___"
            "  HostName A.example.invalid"
        )
        Assert-ThrowsLike {
            Get-RemoteKitEmbeddedSshConfigText $misplaced
        } "*only Host value on its line*" "self token mixed with other Host values should fail clearly."
    } finally {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-RepoTemplateUsesSelfContract {
    $entry = Join-Path $script:SmokeRepoRoot "Favorites\template.vps1.cmd"
    $source = [System.IO.File]::ReadAllText($entry)
    $document = Read-RemoteKitEmbeddedSshConfigDocument $entry
    $generated = $document.ConfigText
    $alias = Get-RemoteKitEntryHostAlias $entry

    Assert-Contains $source "Host ___self___" "vps1 template should expose the self token instead of a user-managed alias."
    Assert-NotContains $source "%HOST%" "vps1 template should not maintain a second host identity."
    Assert-NotContains $source "remote-kit ssh-config begin" "vps1 template should not require visual parser markers."
    Assert-NotContains $source "remote-kit ssh-config end" "vps1 template should not require visual parser markers."
    Assert-NotContains $source 'REMOTE_KIT_REMOTE_COMMAND_PREFIX' "vps1 template should not expose a CMD environment prefix."
    Assert-Contains $source '___RemoteShell___ posix' "vps1 template should expose the default remote-shell macro."
    Assert-True ($document.RemoteShell -eq 'posix') "vps1 template should resolve to the posix shell."
    Assert-NotContains $generated '___RemoteShell___' "generated OpenSSH config should not retain private macros."
    Assert-Contains $generated "Host $alias" "the real vps1 template should be parseable."
    Assert-Contains $generated "IdentityFile ~/.ssh/id_rsa" "vps1 template should keep IdentityFile inside ssh_config."
}

function Test-WriteEmbeddedConfigDoesNotInstallManagedInclude {
    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("remote-kit-write-" + [guid]::NewGuid().ToString("N"))
    $fakeProfile = Join-Path $tempRoot "profile"
    try {
        New-Item -ItemType Directory -Path $fakeProfile -Force | Out-Null
        $entry = New-SmokeEntryFile $tempRoot
        $first = Write-RemoteKitEmbeddedSshConfig $entry $tempRoot $fakeProfile
        $second = Write-RemoteKitEmbeddedSshConfig $entry $tempRoot $fakeProfile

        Assert-True (Test-Path -LiteralPath $first.ConfigPath -PathType Leaf) "generated config should exist."
        Assert-True ($first.ConfigPath -eq $second.ConfigPath) "write should be idempotent for config path."
        Assert-True ([System.IO.Path]::GetFileName($first.ConfigPath) -eq "$($first.HostAlias).config") "config filename should carry the derived alias."
        Assert-True ($first.RemoteShell -eq 'posix') "write result should expose the resolved remote shell."

        $generated = [System.IO.File]::ReadAllText($first.ConfigPath)
        Assert-Contains $generated "Host $($first.HostAlias)" "generated config should contain the derived alias."
        Assert-Contains $generated "HostName A.example.invalid" "generated config should contain HostName."
        Assert-True (-not (Test-Path -LiteralPath $first.UserConfigPath -PathType Leaf)) "write should not install a user SSH Include."
    } finally {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-InstallEmbeddedConfigIsExactAndIdempotent {
    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("remote-kit-install-" + [guid]::NewGuid().ToString("N"))
    $fakeProfile = Join-Path $tempRoot "profile"
    $sshDir = Join-Path $fakeProfile ".ssh"
    try {
        New-Item -ItemType Directory -Path $sshDir -Force | Out-Null
        $entry = New-SmokeEntryFile $tempRoot
        $alias = Get-RemoteKitEntryHostAlias $entry
        $otherAlias = "$alias-extra"
        $existingConfig = @"
Include "D:/old/other.config" # swaw-kit host=$otherAlias id=$script:RemoteKitSshConfigIncludeId
Host github.com
  User git
"@
        [System.IO.File]::WriteAllText((Join-Path $sshDir "config"), $existingConfig, [System.Text.UTF8Encoding]::new($false))

        $first = Install-RemoteKitEmbeddedSshConfig $entry $tempRoot $fakeProfile
        $second = Install-RemoteKitEmbeddedSshConfig $entry $tempRoot $fakeProfile
        $userConfig = [System.IO.File]::ReadAllText($first.UserConfigPath)
        $markerCount = ([regex]::Matches($userConfig, [regex]::Escape($script:RemoteKitSshConfigIncludeId))).Count
        $aliasCount = ([regex]::Matches($userConfig, "host=$([regex]::Escape($alias))(?=\s|$)")).Count

        Assert-True ($first.ConfigPath -eq $second.ConfigPath) "install should be idempotent for config path."
        Assert-True ($markerCount -eq 2) "install should add one managed Include while preserving the other entry."
        Assert-True ($aliasCount -eq 1) "install should write the current alias once."
        Assert-Contains $userConfig "host=$otherAlias" "managed Include matching should be exact."
        Assert-True ($userConfig.IndexOf("host=$alias ") -lt $userConfig.IndexOf("Host github.com")) "managed Include should precede existing Host blocks."
    } finally {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-RemoveEmbeddedConfigRemovesManagedState {
    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("remote-kit-remove-" + [guid]::NewGuid().ToString("N"))
    $fakeProfile = Join-Path $tempRoot "profile"
    try {
        New-Item -ItemType Directory -Path $fakeProfile -Force | Out-Null
        $entry = New-SmokeEntryFile $tempRoot
        $installed = Install-RemoteKitEmbeddedSshConfig $entry $tempRoot $fakeProfile

        Remove-RemoteKitEmbeddedSshConfig $entry $tempRoot $fakeProfile

        Assert-True (-not (Test-Path -LiteralPath $installed.ConfigPath -PathType Leaf)) "remove should delete the generated config."
        $userConfig = [System.IO.File]::ReadAllText($installed.UserConfigPath)
        Assert-NotContains $userConfig "host=$($installed.HostAlias)" "remove should delete the matching managed Include."
    } finally {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Test-EntryIdentityIsStableAndPathScoped
Test-ExtractPrettyEmbeddedConfig
Test-SelfTokenValidation
Test-RemoteShellMacro
Test-RepoTemplateUsesSelfContract
Test-WriteEmbeddedConfigDoesNotInstallManagedInclude
Test-InstallEmbeddedConfigIsExactAndIdempotent
Test-RemoveEmbeddedConfigRemovesManagedState
Write-Host "ssh remote kit ssh-config smoke ok" -ForegroundColor Green
