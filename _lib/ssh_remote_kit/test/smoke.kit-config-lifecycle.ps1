$ErrorActionPreference = "Stop"

$script:LifecycleRepoRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
$script:KitRoot = Join-Path $script:LifecycleRepoRoot "_lib\ssh_remote_kit"
. (Join-Path $script:LifecycleRepoRoot "_lib\test_support\template-entry.ps1")
$script:Entry = New-SwawKitTestTemplateEntry `
    -RepoRoot $script:LifecycleRepoRoot `
    -TemplateName "template.vps1.cmd" `
    -EntryName "test.template.vps1.cmd"
$script:DataSshConfig = Join-Path $script:LifecycleRepoRoot "data\ssh_config"
. (Join-Path $script:KitRoot "ssh_config.ps1")
$script:EntryHostAlias = Get-RemoteKitEntryHostAlias $script:Entry
$script:GeneratedConfig = Get-RemoteKitGeneratedSshConfigPath $script:LifecycleRepoRoot $script:EntryHostAlias

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

function Get-UserConfigText {
    param([Parameter(Mandatory=$true)] [string]$Profile)

    $path = Join-Path $Profile ".ssh\config"
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        return ""
    }

    return [System.IO.File]::ReadAllText($path)
}

function Assert-ManagedIncludeState {
    param(
        [Parameter(Mandatory=$true)] [string]$Profile,
        [Parameter(Mandatory=$true)] [bool]$ExpectedInstalled,
        [Parameter(Mandatory=$true)] [string]$Message
    )

    $text = Get-UserConfigText $Profile
    $installed = $text.Contains("swaw-kit host=$script:EntryHostAlias")
    Assert-True ($installed -eq $ExpectedInstalled) $Message
}

function Assert-ManagedIncludeCount {
    param(
        [Parameter(Mandatory=$true)] [string]$Profile,
        [Parameter(Mandatory=$true)] [int]$ExpectedCount,
        [Parameter(Mandatory=$true)] [string]$Message
    )

    $text = Get-UserConfigText $Profile
    $count = ([regex]::Matches($text, [regex]::Escape("swaw-kit host=$script:EntryHostAlias"))).Count
    Assert-True ($count -eq $ExpectedCount) $Message
}

function Initialize-FakeTools {
    param([Parameter(Mandatory=$true)] [string]$BinDir)

    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    $encoding = [System.Text.ASCIIEncoding]::new()
    [System.IO.File]::WriteAllText((Join-Path $BinDir "ssh.cmd"), "@echo off`r`necho FAKE_SSH %*>>`"%REMOTE_KIT_FAKE_LOG%`"`r`necho /home/root`r`nexit /b 0`r`n", $encoding)
    [System.IO.File]::WriteAllText((Join-Path $BinDir "scp.cmd"), "@echo off`r`necho FAKE_SCP %*>>`"%REMOTE_KIT_FAKE_LOG%`"`r`nexit /b 0`r`n", $encoding)
    [System.IO.File]::WriteAllText((Join-Path $BinDir "code.cmd"), "@echo off`r`necho FAKE_CODE %*>>`"%REMOTE_KIT_FAKE_LOG%`"`r`nexit /b 0`r`n", $encoding)
    [System.IO.File]::WriteAllText((Join-Path $BinDir "cursor.cmd"), "@echo off`r`nif /i `"%~1`"==`"--help`" (`r`n  echo   --classic  Open the classic IDE`r`n  exit /b 0`r`n)`r`necho FAKE_CURSOR %*>>`"%REMOTE_KIT_FAKE_LOG%`"`r`nexit /b 0`r`n", $encoding)
    [System.IO.File]::WriteAllText(
        (Join-Path $BinDir "PowerShell.cmd"),
        "@echo off`r`necho %* | findstr /i /c:`"entry-bootstrap.ps1`" >nul`r`nif not errorlevel 1 exit /b 0`r`n`"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`" %*`r`nexit /b %ERRORLEVEL%`r`n",
        $encoding
    )
}

function Invoke-Entry {
    param(
        [Parameter(Mandatory=$true)] [string]$FakeBin,
        [Parameter(Mandatory=$true)] [string]$Profile,
        [Parameter(Mandatory=$true)] [string]$Log,
        [Parameter(Mandatory=$true)] [string]$Arguments
    )

    $command = @(
        "set ""PATH=$FakeBin;%PATH%""",
        "set ""USERPROFILE=$Profile""",
        "set ""REMOTE_KIT_FAKE_LOG=$Log""",
        """$script:Entry"" $Arguments"
    ) -join " && "

    cmd /d /c $command | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "entry command failed ($LASTEXITCODE): $Arguments"
    }
}

function Test-CmdShellInitializationIsAppliedOnce {
    $scenario = New-Scenario
    $originalEntryText = [System.IO.File]::ReadAllText($script:Entry)
    try {
        $cmdEntryText = $originalEntryText.Replace(
            '___RemoteShell___ posix',
            '___RemoteShell___ win.cmd'
        )
        Assert-True `
            ($cmdEntryText -ne $originalEntryText) `
            'test entry should expose the default posix remote-shell macro.'
        [System.IO.File]::WriteAllText(
            $script:Entry,
            $cmdEntryText,
            [System.Text.UTF8Encoding]::new($false)
        )
        Invoke-Entry $scenario.FakeBin $scenario.Profile $scenario.Log "-- qwinsta"

        $logText = [System.IO.File]::ReadAllText($scenario.Log)
        Assert-Contains `
            $logText `
            '"chcp 65001>nul & qwinsta"' `
            "win.cmd profile should initialize UTF-8 inside the SSH command.`n$logText"
    } finally {
        [System.IO.File]::WriteAllText(
            $script:Entry,
            $originalEntryText,
            [System.Text.UTF8Encoding]::new($false)
        )
        Remove-Item -LiteralPath $scenario.Root -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function New-Scenario {
    $root = Join-Path ([System.IO.Path]::GetTempPath()) ("remote-kit-lifecycle-" + [guid]::NewGuid().ToString("N"))
    $fakeBin = Join-Path $root "bin"
    $profile = Join-Path $root "profile"
    $workspace = Join-Path $root "workspace"
    $log = Join-Path $root "fake.log"
    New-Item -ItemType Directory -Path $profile, $workspace -Force | Out-Null
    Initialize-FakeTools $fakeBin

    return [pscustomobject]@{
        Root = $root
        FakeBin = $fakeBin
        Profile = $profile
        Workspace = $workspace
        Log = $log
    }
}

function Test-DefaultCommandWritesGeneratedConfigOnly {
    $scenario = New-Scenario
    try {
        Invoke-Entry `
            $scenario.FakeBin `
            $scenario.Profile `
            $scenario.Log `
            "-- echo ABC== name=value"

        Assert-True (Test-Path -LiteralPath $script:GeneratedConfig -PathType Leaf) "default command should generate repo-local ssh config."
        Assert-ManagedIncludeState $scenario.Profile $false "default command should not install user ssh Include."
        $logText = [System.IO.File]::ReadAllText($scenario.Log)
        Assert-Contains `
            $logText `
            '"echo ABC== name=value"' `
            "posix shell metadata should preserve '=' in the remote command."
        Assert-True `
            (-not $logText.Contains('chcp 65001')) `
            "posix shell metadata should not add Windows initialization."
    } finally {
        Remove-Item -LiteralPath $scenario.Root -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-ExplicitInstallAndRemoveManageUserInclude {
    $scenario = New-Scenario
    try {
        Invoke-Entry $scenario.FakeBin $scenario.Profile $scenario.Log "config.install"
        Invoke-Entry $scenario.FakeBin $scenario.Profile $scenario.Log "config.install"
        Assert-ManagedIncludeState $scenario.Profile $true "config.install should install user ssh Include."
        Assert-ManagedIncludeCount $scenario.Profile 1 "config.install should be idempotent and not duplicate managed Include."
        Assert-True (Test-Path -LiteralPath $script:GeneratedConfig -PathType Leaf) "config.install should keep generated config."

        Invoke-Entry $scenario.FakeBin $scenario.Profile $scenario.Log "config.remove"
        Assert-ManagedIncludeState $scenario.Profile $false "config.remove should remove user ssh Include."
        Assert-True (-not (Test-Path -LiteralPath $script:GeneratedConfig -PathType Leaf)) "config.remove should delete generated config."
    } finally {
        Remove-Item -LiteralPath $scenario.Root -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-RemoteSshEditorInstallsIncludeButSftpModeDoesNot {
    $scenario = New-Scenario
    try {
        Invoke-Entry $scenario.FakeBin $scenario.Profile $scenario.Log "code /var/www"
        Assert-ManagedIncludeState $scenario.Profile $true "code remote-ssh mode should install user ssh Include."

        Invoke-Entry $scenario.FakeBin $scenario.Profile $scenario.Log "config.remove"
        Invoke-Entry $scenario.FakeBin $scenario.Profile $scenario.Log "code :/var/www ""$($scenario.Workspace)"""

        Assert-ManagedIncludeState $scenario.Profile $false "code SFTP mode should not install user ssh Include."
        $sftpConfig = [System.IO.File]::ReadAllText((Join-Path $scenario.Workspace ".vscode\SFTP.json"))
        Assert-Contains $sftpConfig '"sshConfigPath":' "SFTP mode should point extension config at generated ssh config."
    } finally {
        Remove-Item -LiteralPath $scenario.Root -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$hadDataSshConfig = Test-Path -LiteralPath $script:DataSshConfig
$hadGeneratedConfig = Test-Path -LiteralPath $script:GeneratedConfig -PathType Leaf
$generatedConfigText = if ($hadGeneratedConfig) {
    [System.IO.File]::ReadAllText($script:GeneratedConfig)
} else {
    $null
}
try {
    Test-CmdShellInitializationIsAppliedOnce
    Test-DefaultCommandWritesGeneratedConfigOnly
    Test-ExplicitInstallAndRemoveManageUserInclude
    Test-RemoteSshEditorInstallsIncludeButSftpModeDoesNot
    Write-Host "ssh remote kit config lifecycle smoke ok" -ForegroundColor Green
} finally {
    if ($hadGeneratedConfig) {
        $configDir = Split-Path -Parent $script:GeneratedConfig
        if (-not (Test-Path -LiteralPath $configDir -PathType Container)) {
            New-Item -ItemType Directory -Path $configDir -Force | Out-Null
        }
        [System.IO.File]::WriteAllText($script:GeneratedConfig, $generatedConfigText, [System.Text.UTF8Encoding]::new($false))
    } elseif (Test-Path -LiteralPath $script:GeneratedConfig -PathType Leaf) {
        Remove-Item -LiteralPath $script:GeneratedConfig -Force -ErrorAction SilentlyContinue
    }

    if (-not $hadDataSshConfig -and (Test-Path -LiteralPath $script:DataSshConfig)) {
        Remove-Item -LiteralPath $script:DataSshConfig -Recurse -Force -ErrorAction SilentlyContinue
    }
    Remove-SwawKitTestTemplateEntry `
        -RepoRoot $script:LifecycleRepoRoot `
        -EntryPath $script:Entry
}
