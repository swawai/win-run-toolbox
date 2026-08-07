<#
.SYNOPSIS
  Generate and include embedded OpenSSH config blocks from entry scripts.
#>

param(
    [ValidateSet("write","install","ensure","remove")]
    [string]$Action = "write",
    [string]$EntryFile,
    [string]$RepoRoot,
    [string]$UserProfile = $env:USERPROFILE
)

$script:RemoteKitSshConfigIncludeId = "8f6a9d72-4a7e-4b42-95cb-8bc20d9f5c31"
$script:RemoteKitSshConfigAfterLabel = "REMOTE_KIT_AFTER_SSH_CONFIG"
$script:RemoteKitSelfHostToken = "___self___"
. (Join-Path $PSScriptRoot 'remote-shell.ps1')

function ConvertTo-RemoteKitLfText {
    param([AllowNull()] [string]$Text)

    if ($null -eq $Text) {
        $Text = ""
    }

    if ($Text.Length -gt 0 -and [int][char]$Text[0] -eq 0xFEFF) {
        $Text = $Text.Substring(1)
    }

    $textLf = $Text -replace "`r`n", "`n" -replace "`r", "`n"
    if (-not $textLf.EndsWith("`n")) {
        $textLf += "`n"
    }

    return $textLf
}

function ConvertTo-RemoteKitSshConfigPath {
    param([Parameter(Mandatory=$true)] [string]$Path)

    return ([System.IO.Path]::GetFullPath($Path)).Replace("\", "/")
}

function ConvertTo-RemoteKitSafeConfigName {
    param([Parameter(Mandatory=$true)] [string]$HostAlias)

    $safe = $HostAlias -replace '[^A-Za-z0-9._-]', '_'
    if ([string]::IsNullOrWhiteSpace($safe)) {
        throw "Host alias produces an empty config file name: $HostAlias"
    }

    return $safe
}

function Get-RemoteKitEntryHostAlias {
    param([Parameter(Mandatory=$true)] [string]$EntryFile)

    if (-not (Test-Path -LiteralPath $EntryFile -PathType Leaf)) {
        throw "Entry file not found: $EntryFile"
    }

    $entryPath = [System.IO.Path]::GetFullPath($EntryFile)
    $entryName = [System.IO.Path]::GetFileNameWithoutExtension($entryPath)
    $safeName = ConvertTo-RemoteKitSafeConfigName $entryName
    $identityText = $entryPath.Replace("\", "/").ToLowerInvariant()
    $identityBytes = [System.Text.Encoding]::UTF8.GetBytes($identityText)
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hashBytes = $sha256.ComputeHash($identityBytes)
    } finally {
        $sha256.Dispose()
    }

    $hashText = -join @($hashBytes | ForEach-Object { $_.ToString("x2") })
    return "$safeName-$($hashText.Substring(0, 12))"
}

function Protect-RemoteKitSshConfigFile {
    param([Parameter(Mandatory=$true)] [string]$Path)

    if (-not $IsWindows -and $null -ne $IsWindows) {
        return
    }

    $currentUserName = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    & icacls.exe $Path /inheritance:r /grant:r "${currentUserName}:F" "SYSTEM:F" | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to protect SSH config ACL: $Path"
    }
}

function Read-RemoteKitEmbeddedSshConfigDocument {
    param([Parameter(Mandatory=$true)] [string]$EntryFile)

    if (-not (Test-Path -LiteralPath $EntryFile -PathType Leaf)) {
        throw "Entry file not found: $EntryFile"
    }

    $text = [System.IO.File]::ReadAllText($EntryFile)
    $text = ConvertTo-RemoteKitLfText $text
    $lines = $text -split "`n"
    $escapedLabel = [regex]::Escape($script:RemoteKitSshConfigAfterLabel)
    $gotoPattern = "(?:^|&)\s*goto\s+:$escapedLabel\s*$"
    $labelPattern = "^\s*:$escapedLabel\s*$"
    $startIndex = -1
    $endIndex = -1

    for ($index = 0; $index -lt $lines.Count; $index++) {
        $line = $lines[$index].TrimEnd("`r")
        if ($line -match $gotoPattern) {
            if ($startIndex -ge 0) {
                throw "Embedded ssh_config has multiple goto boundaries: $EntryFile"
            }
            $startIndex = $index
            continue
        }

        if ($startIndex -ge 0 -and $line -match $labelPattern) {
            $endIndex = $index
            break
        }
    }

    if ($startIndex -lt 0 -or $endIndex -lt 0 -or $endIndex -le $startIndex) {
        throw "Embedded ssh_config block not found in entry file: $EntryFile"
    }

    $selected = New-Object System.Collections.Generic.List[string]
    for ($index = $startIndex + 1; $index -lt $endIndex; $index++) {
        $line = $lines[$index].TrimEnd("`r")
        if ([string]::IsNullOrWhiteSpace($line)) {
            continue
        }
        if ($line -match "^\s*::") {
            continue
        }
        if ($line -match "^\s*@?rem(?:\s|$)") {
            continue
        }
        $selected.Add($line)
    }

    $selectedLines = $selected.ToArray()
    $remoteShell = Get-RemoteKitRemoteShell -Lines $selectedLines
    $configLines = @($selectedLines | Where-Object {
        -not (Test-RemoteKitRemoteShellDirectiveLine -Line $_)
    })
    $configText = ($configLines -join "`n")
    $tokenCount = ([regex]::Matches($configText, [regex]::Escape($script:RemoteKitSelfHostToken))).Count
    if ($tokenCount -ne 1) {
        throw "Embedded ssh_config must contain exactly one '$script:RemoteKitSelfHostToken' token: $EntryFile"
    }

    $hostAlias = Get-RemoteKitEntryHostAlias -EntryFile $EntryFile
    $selfHostPattern = "^(\s*Host\s+)$([regex]::Escape($script:RemoteKitSelfHostToken))(\s*(?:#.*)?)$"
    $selfHostRegex = [regex]::new(
        $selfHostPattern,
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase -bor
            [System.Text.RegularExpressions.RegexOptions]::Multiline
    )
    if ($selfHostRegex.Matches($configText).Count -ne 1) {
        throw "The '$script:RemoteKitSelfHostToken' token must be the only Host value on its line: $EntryFile"
    }

    $configText = $selfHostRegex.Replace(
        $configText,
        ('${1}' + $hostAlias + '${2}'),
        1
    )
    return [pscustomobject]@{
        ConfigText  = ConvertTo-RemoteKitLfText $configText
        RemoteShell = $remoteShell
    }
}

function Get-RemoteKitEmbeddedSshConfigText {
    param([Parameter(Mandatory=$true)] [string]$EntryFile)

    return (Read-RemoteKitEmbeddedSshConfigDocument -EntryFile $EntryFile).ConfigText
}

function Get-RemoteKitGeneratedSshConfigPath {
    param(
        [Parameter(Mandatory=$true)] [string]$RepoRoot,
        [Parameter(Mandatory=$true)] [string]$HostAlias
    )

    $safeName = ConvertTo-RemoteKitSafeConfigName $HostAlias
    $root = [System.IO.Path]::GetFullPath($RepoRoot)
    return Join-Path (Join-Path $root "data\ssh_config") "$safeName.config"
}

function New-RemoteKitSshConfigIncludeLine {
    param(
        [Parameter(Mandatory=$true)] [string]$ConfigPath,
        [Parameter(Mandatory=$true)] [string]$HostAlias
    )

    $sshPath = ConvertTo-RemoteKitSshConfigPath $ConfigPath
    return "Include `"$sshPath`" # swaw-kit host=$HostAlias id=$script:RemoteKitSshConfigIncludeId"
}

function Test-RemoteKitManagedIncludeLine {
    param(
        [AllowNull()] [string]$Line,
        [Parameter(Mandatory=$true)] [string]$HostAlias
    )

    if ([string]::IsNullOrWhiteSpace($Line) -or -not $Line.Contains($script:RemoteKitSshConfigIncludeId)) {
        return $false
    }

    $hostPattern = "(^|\s)host=$([regex]::Escape($HostAlias))(?=\s|$)"
    return $Line -match $hostPattern
}

function Ensure-RemoteKitSshConfigInclude {
    param(
        [Parameter(Mandatory=$true)] [string]$UserConfigPath,
        [Parameter(Mandatory=$true)] [string]$IncludeLine,
        [Parameter(Mandatory=$true)] [string]$HostAlias
    )

    $dir = Split-Path -Parent $UserConfigPath
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }

    $lines = @()
    if (Test-Path -LiteralPath $UserConfigPath -PathType Leaf) {
        $lines = @(Get-Content -LiteralPath $UserConfigPath)
    }

    $filtered = @($lines | Where-Object {
        -not (Test-RemoteKitManagedIncludeLine -Line $_ -HostAlias $HostAlias)
    })
    $newLines = @($IncludeLine) + $filtered

    $newText = ($newLines -join "`r`n") + "`r`n"
    $oldText = if (Test-Path -LiteralPath $UserConfigPath -PathType Leaf) {
        [System.IO.File]::ReadAllText($UserConfigPath)
    } else {
        $null
    }

    if ($oldText -ne $newText) {
        if ($null -ne $oldText) {
            $stamp = Get-Date -Format "yyyyMMddHHmmss"
            Copy-Item -LiteralPath $UserConfigPath -Destination "$UserConfigPath.swaw-kit-ssh-remote-backup-$stamp" -Force
        }

        [System.IO.File]::WriteAllText($UserConfigPath, $newText, [System.Text.UTF8Encoding]::new($false))
        Protect-RemoteKitSshConfigFile $UserConfigPath
    }
}

function Remove-RemoteKitSshConfigInclude {
    param(
        [Parameter(Mandatory=$true)] [string]$UserConfigPath,
        [Parameter(Mandatory=$true)] [string]$HostAlias
    )

    if (-not (Test-Path -LiteralPath $UserConfigPath -PathType Leaf)) {
        return
    }

    $lines = @(Get-Content -LiteralPath $UserConfigPath)
    $filtered = @($lines | Where-Object {
        -not (Test-RemoteKitManagedIncludeLine -Line $_ -HostAlias $HostAlias)
    })

    if ($filtered.Count -ne $lines.Count) {
        [System.IO.File]::WriteAllText($UserConfigPath, (($filtered -join "`r`n") + "`r`n"), [System.Text.UTF8Encoding]::new($false))
        Protect-RemoteKitSshConfigFile $UserConfigPath
    }
}

function Write-RemoteKitEmbeddedSshConfig {
    param(
        [Parameter(Mandatory=$true)] [string]$EntryFile,
        [Parameter(Mandatory=$true)] [string]$RepoRoot,
        [Parameter(Mandatory=$true)] [string]$UserProfile
    )

    $hostAlias = Get-RemoteKitEntryHostAlias -EntryFile $EntryFile
    $document = Read-RemoteKitEmbeddedSshConfigDocument -EntryFile $EntryFile
    $configText = $document.ConfigText
    $configPath = Get-RemoteKitGeneratedSshConfigPath -RepoRoot $RepoRoot -HostAlias $HostAlias
    $configDir = Split-Path -Parent $configPath
    if (-not (Test-Path -LiteralPath $configDir -PathType Container)) {
        New-Item -ItemType Directory -Path $configDir -Force | Out-Null
    }

    [System.IO.File]::WriteAllText($configPath, $configText, [System.Text.UTF8Encoding]::new($false))
    Protect-RemoteKitSshConfigFile $configPath

    $userConfigPath = Join-Path (Join-Path $UserProfile ".ssh") "config"
    $includeLine = New-RemoteKitSshConfigIncludeLine -ConfigPath $configPath -HostAlias $HostAlias

    return [pscustomobject]@{
        HostAlias      = $HostAlias
        ConfigPath     = $configPath
        RemoteShell    = $document.RemoteShell
        UserConfigPath = $userConfigPath
        IncludeLine    = $includeLine
    }
}

function Install-RemoteKitEmbeddedSshConfig {
    param(
        [Parameter(Mandatory=$true)] [string]$EntryFile,
        [Parameter(Mandatory=$true)] [string]$RepoRoot,
        [Parameter(Mandatory=$true)] [string]$UserProfile
    )

    $result = Write-RemoteKitEmbeddedSshConfig `
        -EntryFile $EntryFile `
        -RepoRoot $RepoRoot `
        -UserProfile $UserProfile
    Ensure-RemoteKitSshConfigInclude `
        -UserConfigPath $result.UserConfigPath `
        -IncludeLine $result.IncludeLine `
        -HostAlias $result.HostAlias
    return $result
}

function Ensure-RemoteKitEmbeddedSshConfig {
    param(
        [Parameter(Mandatory=$true)] [string]$EntryFile,
        [Parameter(Mandatory=$true)] [string]$RepoRoot,
        [Parameter(Mandatory=$true)] [string]$UserProfile
    )

    return Install-RemoteKitEmbeddedSshConfig `
        -EntryFile $EntryFile `
        -RepoRoot $RepoRoot `
        -UserProfile $UserProfile
}

function Remove-RemoteKitEmbeddedSshConfig {
    param(
        [Parameter(Mandatory=$true)] [string]$EntryFile,
        [Parameter(Mandatory=$true)] [string]$RepoRoot,
        [Parameter(Mandatory=$true)] [string]$UserProfile
    )

    $hostAlias = Get-RemoteKitEntryHostAlias -EntryFile $EntryFile
    $userConfigPath = Join-Path (Join-Path $UserProfile ".ssh") "config"
    Remove-RemoteKitSshConfigInclude -UserConfigPath $userConfigPath -HostAlias $HostAlias

    $configPath = Get-RemoteKitGeneratedSshConfigPath -RepoRoot $RepoRoot -HostAlias $HostAlias
    if (Test-Path -LiteralPath $configPath -PathType Leaf) {
        Remove-Item -LiteralPath $configPath -Force
    }
}

function Invoke-RemoteKitSshConfigCli {
    if ([string]::IsNullOrWhiteSpace($UserProfile)) {
        throw "-UserProfile is required."
    }

    foreach ($required in @("EntryFile","RepoRoot")) {
        $value = Get-Variable -Name $required -ValueOnly
        if ([string]::IsNullOrWhiteSpace($value)) {
            throw "-$required is required for $Action."
        }
    }

    if ($Action -eq "remove") {
        $removedHostAlias = Get-RemoteKitEntryHostAlias -EntryFile $EntryFile
        Remove-RemoteKitEmbeddedSshConfig `
            -EntryFile $EntryFile `
            -RepoRoot $RepoRoot `
            -UserProfile $UserProfile
        Write-Output $removedHostAlias
        return
    }

    if ($Action -eq "install" -or $Action -eq "ensure") {
        $result = Install-RemoteKitEmbeddedSshConfig `
            -EntryFile $EntryFile `
            -RepoRoot $RepoRoot `
            -UserProfile $UserProfile
    } else {
        $result = Write-RemoteKitEmbeddedSshConfig `
            -EntryFile $EntryFile `
            -RepoRoot $RepoRoot `
            -UserProfile $UserProfile
    }

    Write-Output "config|$($result.ConfigPath)"
    Write-Output "shell|$($result.RemoteShell)"
}

if ($MyInvocation.InvocationName -ne ".") {
    Invoke-RemoteKitSshConfigCli
}
