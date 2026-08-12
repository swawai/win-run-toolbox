[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$OutputEncoding = New-Object Text.UTF8Encoding($false)

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$TemplateEntry = Join-Path $RepoRoot 'Favorites\template.rdp1.cmd'
$ScratchRoot = Join-Path (Join-Path $RepoRoot 'data\rdp-client') (
    '.shadow-test-' + [Guid]::NewGuid().ToString('N')
)
$Entry = Join-Path $ScratchRoot 'account.rdp.cmd'
$FakeSshEntry = Join-Path $ScratchRoot 'windows-admin.ssh.cmd'
$FakeSshScript = Join-Path $ScratchRoot 'capture-shadow-stdin.ps1'
$CapturePath = Join-Path $ScratchRoot 'ssh-arguments.txt'
$SourceCapturePath = Join-Path $ScratchRoot 'ssh-source.ps1'
$StartCapturePath = Join-Path $ScratchRoot 'shadow-start.txt'
$DoctorCapturePath = Join-Path $ScratchRoot 'shadow-doctor.txt'
$ManageCapturePath = Join-Path $ScratchRoot 'shadow-manage.txt'
$Runtime = Join-Path $ScratchRoot '_lib\rdp_client'

. (Join-Path $PSScriptRoot '..\entry.ps1')
. (Join-Path $PSScriptRoot '..\peer-ssh.ps1')

function Invoke-ShadowTestEntry {
    param(
        [string[]]$Arguments,
        [int]$ExpectedExitCode
    )

    $PreviousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Output = (& $Entry @Arguments 2>&1 | Out-String)
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousPreference
    }
    if ($ExitCode -ne $ExpectedExitCode) {
        throw "Unexpected exit code $ExitCode.`n$Output"
    }
    return $Output
}

try {
    $RemoteDoctorPath = Join-Path $PSScriptRoot '..\shadow-doctor.remote.ps1'
    $RemoteDoctorSource = [IO.File]::ReadAllText(
        $RemoteDoctorPath,
        [Text.Encoding]::UTF8
    )
    foreach ($ExpectedDoctorCheck in @(
        'AllowRemoteRPC',
        'RemoteDesktop-Shadow-In-TCP',
        'swaw-kit-rdp-shadow-rpc',
        'swaw-kit-rdp-shadow-smb',
        'swaw-kit-rdp-shadow-transport',
        'RollbackPresent',
        'FPS-RPCSS-In-TCP',
        'FPS-SMB-In-TCP',
        'LanmanServer',
        'quser.exe'
    )) {
        if (-not $RemoteDoctorSource.Contains($ExpectedDoctorCheck)) {
            throw "Remote Shadow doctor is missing '$ExpectedDoctorCheck'."
        }
    }

    $RemoteManageQueryPath = Join-Path $PSScriptRoot '..\shadow-manage-query.remote.ps1'
    $RemoteManageQuerySource = [IO.File]::ReadAllText(
        $RemoteManageQueryPath,
        [Text.Encoding]::UTF8
    )
    $LocalManageSource = [IO.File]::ReadAllText(
        (Join-Path $PSScriptRoot '..\shadow-manage.ps1'),
        [Text.Encoding]::UTF8
    )
    foreach ($ExpectedManageQuery in @(
        'SSH_CONNECTION',
        'HKLM:\SOFTWARE\swaw-kit\rollback\rdp-client\shadow',
        'swaw-kit-rdp-shadow-rpc',
        'swaw-kit-rdp-shadow-smb',
        'swaw-kit-rdp-shadow-transport',
        'AllowRemoteRPCOriginalPresent',
        'ShadowOriginalPresent',
        'PeerAddress',
        'RemoteAddress',
        'FirewallRules'
    )) {
        if (-not $RemoteManageQuerySource.Contains($ExpectedManageQuery)) {
            throw "Remote Shadow state query is missing '$ExpectedManageQuery'."
        }
    }
    foreach ($ExpectedManageSource in @(
        'New-NetFirewallRule',
        'Remove-NetFirewallRule'
    )) {
        if (-not $LocalManageSource.Contains($ExpectedManageSource)) {
            throw "Shadow management is missing '$ExpectedManageSource'."
        }
    }
    if ($LocalManageSource.Contains('Enable-NetFirewallRule')) {
        throw 'Shadow enable must not mutate Windows built-in firewall rules.'
    }

    $ShadowId = Resolve-RdpClientShadowSessionId -Value '2'
    $ShadowArguments = @(New-RdpClientShadowMstscArgumentList `
        -Target 'account.rdp.home.arpa:3389' `
        -ShadowSessionId $ShadowId `
        -Control `
        -NoConsentPrompt)
    if ($ShadowArguments.Count -ne 4 -or
        $ShadowArguments[0] -ne '/v:account.rdp.home.arpa' -or
        $ShadowArguments[1] -ne '/shadow:2' -or
        $ShadowArguments[2] -ne '/control' -or
        $ShadowArguments[3] -ne '/noConsentPrompt') {
        throw "Unexpected mstsc Shadow arguments: $($ShadowArguments -join ' ')"
    }
    $CustomPortArguments = @(New-RdpClientShadowMstscArgumentList `
        -Target 'account.rdp.home.arpa:43389' `
        -ShadowSessionId $ShadowId)
    if ($CustomPortArguments[0] -ne '/v:account.rdp.home.arpa:43389') {
        throw 'Shadow startup should preserve a non-default RDP port.'
    }
    foreach ($InvalidShadowId in @('abc', '-1', '4294967296')) {
        try {
            $null = Resolve-RdpClientShadowSessionId -Value $InvalidShadowId
            throw "Invalid Shadow session ID was accepted: $InvalidShadowId"
        } catch {
            if ($_.Exception.Message -eq
                "Invalid Shadow session ID was accepted: $InvalidShadowId") {
                throw
            }
        }
    }

    [IO.Directory]::CreateDirectory($Runtime) | Out-Null
    [IO.File]::Copy($TemplateEntry, $Entry)
    foreach ($RuntimeFile in @('client.cmd')) {
        [IO.File]::Copy(
            (Join-Path (Join-Path $PSScriptRoot '..') $RuntimeFile),
            (Join-Path $Runtime $RuntimeFile)
        )
    }
    $EntryText = [IO.File]::ReadAllText($Entry, [Text.Encoding]::UTF8)
    $EntryText = [regex]::Replace(
        $EntryText,
        '(?m)^set "RDP_PEER_SSH_ENTRY=.*"\r?$',
        "set `"RDP_PEER_SSH_ENTRY=$FakeSshEntry`""
    )
    [IO.File]::WriteAllText(
        $Entry,
        $EntryText,
        (New-Object Text.UTF8Encoding($false))
    )

    $FakeStartSource = @'
param(
    [string]$EntryFile,
    [string]$SshEntryFile,
    [string]$SessionId,
    [string]$CommandName,
    [switch]$Control,
    [switch]$NoConsentPrompt,
    [switch]$Display,
    [string]$TsconSessionId
)
[IO.File]::WriteAllLines($env:RDP_SHADOW_START_CAPTURE, @(
    "EntryFile=$EntryFile",
    "SshEntryFile=$SshEntryFile",
    "SessionId=$SessionId",
    "CommandName=$CommandName",
    "Control=$($Control.IsPresent)",
    "NoConsentPrompt=$($NoConsentPrompt.IsPresent)",
    "Display=$($Display.IsPresent)",
    "TsconSessionId=$TsconSessionId"
))
exit 0
'@
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'shadow-start.ps1'),
        $FakeStartSource,
        (New-Object Text.UTF8Encoding($false))
    )

    $FakeDoctorSource = @'
param(
    [string]$EntryFile,
    [string]$SshEntryFile,
    [string]$CommandName
)
[IO.File]::WriteAllLines($env:RDP_SHADOW_DOCTOR_CAPTURE, @(
    "EntryFile=$EntryFile",
    "SshEntryFile=$SshEntryFile",
    "CommandName=$CommandName"
))
exit 0
'@
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'shadow-doctor.ps1'),
        $FakeDoctorSource,
        (New-Object Text.UTF8Encoding($false))
    )

    $FakeManageSource = @'
param(
    [string]$Action,
    [string]$SshEntryFile,
    [string]$RdpEntryFile,
    [string]$CommandName,
    [int]$Mode,
    [switch]$DryRun
)
[IO.File]::WriteAllLines($env:RDP_SHADOW_MANAGE_CAPTURE, @(
    "Action=$Action",
    "SshEntryFile=$SshEntryFile",
    "RdpEntryFile=$RdpEntryFile",
    "CommandName=$CommandName",
    "Mode=$Mode",
    "DryRun=$($DryRun.IsPresent)"
))
exit 0
'@
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'shadow-manage.ps1'),
        $FakeManageSource,
        (New-Object Text.UTF8Encoding($false))
    )

    $env:RDP_SHADOW_START_CAPTURE = $StartCapturePath
    try {
        Invoke-ShadowTestEntry `
            -Arguments @('.shadow', '2', '--no-consent', '--control') `
            -ExpectedExitCode 0 |
            Out-Null
    } finally {
        Remove-Item Env:RDP_SHADOW_START_CAPTURE -ErrorAction SilentlyContinue
    }
    $StartState = @([IO.File]::ReadAllLines($StartCapturePath))
    if ($StartState -notcontains 'SessionId=2' -or
        $StartState -notcontains 'Control=True' -or
        $StartState -notcontains 'NoConsentPrompt=True' -or
        $StartState -notcontains 'CommandName=account.rdp' -or
        -not ($StartState | Where-Object { $_ -like 'SshEntryFile=*windows-admin.ssh.cmd' }) -or
        -not ($StartState | Where-Object { $_ -like 'EntryFile=*account.rdp.cmd' })) {
        throw "Unexpected Shadow start dispatch: $($StartState -join '; ')"
    }

    $env:RDP_SHADOW_START_CAPTURE = $StartCapturePath
    try {
        Invoke-ShadowTestEntry `
            -Arguments @('.shadow', 'console', '--control', '--no-consent') `
            -ExpectedExitCode 0 |
            Out-Null
    } finally {
        Remove-Item Env:RDP_SHADOW_START_CAPTURE -ErrorAction SilentlyContinue
    }
    $ConsoleStartState = @([IO.File]::ReadAllLines($StartCapturePath))
    if ($ConsoleStartState -notcontains 'SessionId=console' -or
        $ConsoleStartState -notcontains 'Control=True' -or
        $ConsoleStartState -notcontains 'NoConsentPrompt=True') {
        throw "Unexpected console Shadow dispatch: $($ConsoleStartState -join '; ')"
    }

    $env:RDP_SHADOW_START_CAPTURE = $StartCapturePath
    try {
        Invoke-ShadowTestEntry `
            -Arguments @(
                '.shadow',
                'console',
                '--display',
                '--no-consent',
                '--control'
            ) `
            -ExpectedExitCode 0 |
            Out-Null
    } finally {
        Remove-Item Env:RDP_SHADOW_START_CAPTURE -ErrorAction SilentlyContinue
    }
    $DisplayStartState = @([IO.File]::ReadAllLines($StartCapturePath))
    if ($DisplayStartState -notcontains 'SessionId=console' -or
        $DisplayStartState -notcontains 'Display=True' -or
        $DisplayStartState -notcontains 'TsconSessionId=' -or
        $DisplayStartState -notcontains 'Control=True' -or
        $DisplayStartState -notcontains 'NoConsentPrompt=True') {
        throw "Unexpected console display dispatch: $($DisplayStartState -join '; ')"
    }

    $env:RDP_SHADOW_START_CAPTURE = $StartCapturePath
    try {
        Invoke-ShadowTestEntry `
            -Arguments @(
                '.shadow',
                'console',
                '--tscon',
                '3',
                '--control',
                '--no-consent'
            ) `
            -ExpectedExitCode 0 |
            Out-Null
    } finally {
        Remove-Item Env:RDP_SHADOW_START_CAPTURE -ErrorAction SilentlyContinue
    }
    $TsconStartState = @([IO.File]::ReadAllLines($StartCapturePath))
    if ($TsconStartState -notcontains 'SessionId=console' -or
        $TsconStartState -notcontains 'Display=False' -or
        $TsconStartState -notcontains 'TsconSessionId=3' -or
        $TsconStartState -notcontains 'Control=True' -or
        $TsconStartState -notcontains 'NoConsentPrompt=True') {
        throw "Unexpected console tscon dispatch: $($TsconStartState -join '; ')"
    }

    $env:RDP_SHADOW_DOCTOR_CAPTURE = $DoctorCapturePath
    try {
        Invoke-ShadowTestEntry `
            -Arguments @('.shadow', 'doctor') `
            -ExpectedExitCode 0 |
            Out-Null
    } finally {
        Remove-Item Env:RDP_SHADOW_DOCTOR_CAPTURE -ErrorAction SilentlyContinue
    }
    $DoctorState = @([IO.File]::ReadAllLines($DoctorCapturePath))
    if (-not ($DoctorState | Where-Object { $_ -like 'EntryFile=*account.rdp.cmd' }) -or
        -not ($DoctorState | Where-Object { $_ -like 'SshEntryFile=*windows-admin.ssh.cmd' }) -or
        $DoctorState -notcontains 'CommandName=account.rdp') {
        throw "Unexpected Shadow doctor dispatch: $($DoctorState -join '; ')"
    }

    $env:RDP_SHADOW_MANAGE_CAPTURE = $ManageCapturePath
    try {
        Invoke-ShadowTestEntry `
            -Arguments @('.peer', 'shadow', 'status') `
            -ExpectedExitCode 0 |
            Out-Null
        $StatusState = @([IO.File]::ReadAllLines($ManageCapturePath))
        if ($StatusState -notcontains 'Action=status' -or
            $StatusState -notcontains 'DryRun=False') {
            throw "Unexpected Shadow status dispatch: $($StatusState -join '; ')"
        }

        Invoke-ShadowTestEntry `
            -Arguments @('.peer', 'shadow', 'enable', '--dry-run') `
            -ExpectedExitCode 0 |
            Out-Null
        $EnableState = @([IO.File]::ReadAllLines($ManageCapturePath))
        if ($EnableState -notcontains 'Action=enable' -or
            $EnableState -notcontains 'DryRun=True' -or
            -not ($EnableState | Where-Object { $_ -like 'SshEntryFile=*windows-admin.ssh.cmd' })) {
            throw "Unexpected Shadow enable dispatch: $($EnableState -join '; ')"
        }

        Invoke-ShadowTestEntry `
            -Arguments @('.peer', 'shadow', 'mode', '2', '--dry-run') `
            -ExpectedExitCode 0 |
            Out-Null
        $ModeState = @([IO.File]::ReadAllLines($ManageCapturePath))
        if ($ModeState -notcontains 'Action=mode' -or
            $ModeState -notcontains 'Mode=2' -or
            $ModeState -notcontains 'DryRun=True') {
            throw "Unexpected Shadow mode dispatch: $($ModeState -join '; ')"
        }

        Invoke-ShadowTestEntry `
            -Arguments @('.peer', 'shadow', 'restore') `
            -ExpectedExitCode 0 |
            Out-Null
        $RestoreState = @([IO.File]::ReadAllLines($ManageCapturePath))
        if ($RestoreState -notcontains 'Action=restore' -or
            $RestoreState -notcontains 'DryRun=False') {
            throw "Unexpected Shadow restore dispatch: $($RestoreState -join '; ')"
        }
    } finally {
        Remove-Item Env:RDP_SHADOW_MANAGE_CAPTURE -ErrorAction SilentlyContinue
    }

    $FakeSshSource = @'
@echo off
PowerShell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%~dp0capture-shadow-stdin.ps1" %*
exit /b %ERRORLEVEL%
'@
    $FakeSshSource = [regex]::Replace($FakeSshSource, "`r?`n", "`r`n")
    [IO.File]::WriteAllText(
        $FakeSshEntry,
        $FakeSshSource,
        (New-Object Text.UTF8Encoding($false))
    )
    $FakeSshScriptSource = @'
$ascii = [Text.Encoding]::ASCII
[IO.File]::AppendAllText(
    $env:RDP_SHADOW_TEST_CAPTURE,
    ($args -join ' ') + [Environment]::NewLine,
    $ascii
)
if ($args[0] -ne 'copy') {
    if (-not [string]::IsNullOrWhiteSpace($env:RDP_SHADOW_FAKE_MANAGE_STATE)) {
        Write-Output $env:RDP_SHADOW_FAKE_MANAGE_STATE
    }
    Write-Output ' SESSIONNAME               USERNAME                 ID  STATE'
    Write-Output ' console                   Administrator             2  Active'
    exit 0
}
$outputStream = [IO.File]::Open(
    $env:RDP_SHADOW_TEST_SOURCE,
    [IO.FileMode]::Append,
    [IO.FileAccess]::Write,
    [IO.FileShare]::Read
)
try {
    $marker = $ascii.GetBytes("__RDP_CLIENT_SOURCE__`r`n")
    $outputStream.Write($marker, 0, $marker.Length)
    $sourceBytes = [IO.File]::ReadAllBytes(
        [IO.Path]::GetFullPath([string]$args[1])
    )
    $outputStream.Write($sourceBytes, 0, $sourceBytes.Length)
    $newline = $ascii.GetBytes("`r`n")
    $outputStream.Write($newline, 0, $newline.Length)
} finally {
    $outputStream.Dispose()
}
exit 0
'@
    [IO.File]::WriteAllText(
        $FakeSshScript,
        $FakeSshScriptSource,
        (New-Object Text.UTF8Encoding($false))
    )

    $env:RDP_SHADOW_TEST_CAPTURE = $CapturePath
    $env:RDP_SHADOW_TEST_SOURCE = $SourceCapturePath
    $FakeManageState = [ordered]@{
        ComputerName    = 'TEST-SERVER'
        IsAdministrator = $true
        SourceAddress   = '192.0.2.10'
        PeerAddress     = '192.168.1.115'
        ConnectionError = ''
        Services        = @(
            @{ Name = 'RpcSs'; Present = $true; Status = 'Running' },
            @{ Name = 'LanmanServer'; Present = $true; Status = 'Running' },
            @{ Name = 'TermService'; Present = $true; Status = 'Running' }
        )
        RdpSaPresent    = $true
        AllowRemoteRPC  = @{ Present = $false; Value = 0 }
        ShadowPolicy    = @{ Present = $false; Value = 0 }
        Rollback        = @{
            Present = $false
            Valid = $true
            Error = ''
            Version = 0
            AllowRemoteRPC = @{
                Managed = $false
                Present = 0
                Value = 0
            }
            ShadowPolicy = @{
                Managed = $false
                Present = 0
                Value = 0
            }
        }
        FirewallRules   = @(
            @{
                Name = 'swaw-kit-rdp-shadow-rpc'
                Enabled = $true
                RemoteAddress = '192.0.2.10'
            },
            @{
                Name = 'swaw-kit-rdp-shadow-smb'
                Enabled = $true
                RemoteAddress = '192.0.2.10'
            },
            @{
                Name = 'swaw-kit-rdp-shadow-transport'
                Enabled = $true
                RemoteAddress = '192.0.2.10'
            }
        )
    }
    $FakeManageJson = ConvertTo-Json -InputObject $FakeManageState -Depth 8 -Compress
    $env:RDP_SHADOW_FAKE_MANAGE_STATE = (
        'RDP_SHADOW_MANAGE_STATE_V3:' +
        [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($FakeManageJson))
    )
    try {
        $PreviousPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $StatusOutput = (& (Join-Path $PSScriptRoot '..\shadow-manage.ps1') `
                -Action status `
                -SshEntryFile $FakeSshEntry `
                -RdpEntryFile $Entry `
                -CommandName 'account.rdp' *>&1 | Out-String)
            $StatusExitCode = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $PreviousPreference
        }
        $StatusChecks = @(
            $StatusOutput.Contains(
                'AllowRemoteRPC                                      absent'
            ),
            $StatusOutput.Contains(
                'Shadow                                              absent (system default)'
            ),
            $StatusOutput.Contains(
                'Firewall\swaw-kit-rdp-shadow-rpc                    present remote=192.0.2.10'
            ),
            (-not $StatusOutput.Contains('Rollback: absent'))
        )
        if ($StatusExitCode -ne 0 -or $StatusChecks -contains $false) {
            throw "Unexpected compact Shadow status output (exit=$StatusExitCode checks=$($StatusChecks -join ',')).`n$StatusOutput"
        }

        if ([IO.File]::Exists($CapturePath)) {
            [IO.File]::Delete($CapturePath)
        }
        if ([IO.File]::Exists($SourceCapturePath)) {
            [IO.File]::Delete($SourceCapturePath)
        }
        $PreviousPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $null = & (Join-Path $PSScriptRoot '..\shadow-manage.ps1') `
                -Action enable `
                -SshEntryFile $FakeSshEntry `
                -RdpEntryFile $Entry `
                -CommandName 'account.rdp' 2>&1
            $ManageExitCode = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $PreviousPreference
        }
        if ($ManageExitCode -ne 0) {
            throw "The local Shadow management transport failed: $ManageExitCode"
        }
        $TransportedManageSources = @(
            [IO.File]::ReadAllText(
                $SourceCapturePath,
                [Text.Encoding]::ASCII
            ) -split '(?m)^__RDP_CLIENT_SOURCE__\r?\n' |
                Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
                ForEach-Object { $_ }
        )
        foreach ($TransportedSource in $TransportedManageSources) {
            $Tokens = $null
            $ParseErrors = $null
            [void][Management.Automation.Language.Parser]::ParseInput(
                $TransportedSource,
                [ref]$Tokens,
                [ref]$ParseErrors
            )
            if ($ParseErrors.Count -gt 0) {
                throw "Transported remote PowerShell does not parse: $($ParseErrors[0].Message)"
            }
        }
        if ($TransportedManageSources.Count -ne 3 -or
            -not ($TransportedManageSources | Where-Object {
                $_.Contains('RDP_SHADOW_MANAGE_STATE_V3:')
            }) -or
            -not ($TransportedManageSources | Where-Object {
                $_.Contains('AllowRemoteRPCOriginalPresent') -and
                $_.Contains('New-ItemProperty')
            }) -or
            -not ($TransportedManageSources | Where-Object {
                $_.Contains('New-NetFirewallRule')
            })) {
            throw 'The remote Shadow enable operations were not transported correctly.'
        }
    } finally {
        Remove-Item Env:RDP_SHADOW_TEST_CAPTURE -ErrorAction SilentlyContinue
        Remove-Item Env:RDP_SHADOW_TEST_SOURCE -ErrorAction SilentlyContinue
        Remove-Item Env:RDP_SHADOW_FAKE_MANAGE_STATE -ErrorAction SilentlyContinue
    }

    Write-Host 'rdp client Shadow tests: PASS' -ForegroundColor Green
} finally {
    Remove-Item Env:RDP_SHADOW_TEST_SOURCE -ErrorAction SilentlyContinue
    if ([IO.Directory]::Exists($ScratchRoot)) {
        [IO.Directory]::Delete($ScratchRoot, $true)
    }
}
