[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$OutputEncoding = New-Object Text.UTF8Encoding($false)

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$RuntimeRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$TemplateEntry = Join-Path $RepoRoot 'Favorites\template.rdp1.cmd'
$ScratchRoot = Join-Path (Join-Path $RepoRoot 'data\rdp-client') (
    '.session-test-' + [Guid]::NewGuid().ToString('N')
)
$Runtime = Join-Path $ScratchRoot '_lib\rdp_client'
$Entry = Join-Path $ScratchRoot 'account.rdp.cmd'
$FakeSshEntry = Join-Path $ScratchRoot 'peer.ssh.cmd'
$ConnectCapture = Join-Path $ScratchRoot 'connect.txt'
$ListCapture = Join-Path $ScratchRoot 'list.txt'

. (Join-Path $RuntimeRoot 'entry.ps1')
. (Join-Path $RuntimeRoot 'peer-ssh.ps1')
. (Join-Path $RuntimeRoot 'session.ps1')

function Assert-ThrowsContaining {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [Parameter(Mandatory = $true)][string]$Expected
    )

    try {
        & $Action
        throw "Expected failure containing '$Expected'."
    } catch {
        if (-not $_.Exception.Message.Contains($Expected)) {
            throw
        }
    }
}

function Invoke-SessionTestEntry {
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
        throw "Unexpected exit code $ExitCode for '$($Arguments -join ' ')'.`n$Output"
    }
    return $Output
}

try {
    foreach ($Value in @('0', '2', '4294967295')) {
        $Resolved = Resolve-RdpClientSessionId -Value $Value
        if ([uint64]$Resolved -ne [uint64]$Value) {
            throw "Session ID was not preserved: $Value"
        }
    }
    foreach ($Value in @('abc', '-1', '4294967296')) {
        Assert-ThrowsContaining `
            -Action { Resolve-RdpClientSessionId -Value $Value } `
            -Expected 'Session ID'
    }

    $TargetSession = [pscustomobject]@{
        Id = 2
        UserName = 'Administrator'
        DomainName = 'TEST-SERVER'
        SessionName = 'Console'
        State = 'Active'
        Locked = $false
        Terminal = 'console'
        IsConsole = $true
        ClientName = ''
    }
    $OtherSession = [pscustomobject]@{
        Id = 3
        UserName = 'other-user'
        DomainName = 'TEST-SERVER'
        SessionName = 'RDP-Tcp#3'
        State = 'Disconnected'
        Locked = $true
        Terminal = 'rdp'
        IsConsole = $false
        ClientName = 'CLIENT'
    }
    $State = [pscustomobject]@{
        Version = 1
        ComputerName = 'TEST-SERVER'
        ConsoleSessionId = [uint64]2
        SingleSessionPerUser = 1
        Sessions = @($TargetSession, $OtherSession)
    }

    $ById = Resolve-RdpClientSessionSelection `
        -State $State `
        -EntryUserName 'administrator' `
        -SessionId ([uint32]2)
    $ShadowConsole = Resolve-RdpClientShadowConsoleSession -State $State
    if ($ById.Id -ne 2 -or $ShadowConsole.Id -ne 2) {
        throw 'ID reconnect and console Shadow must resolve their target session.'
    }

    Assert-ThrowsContaining -Action {
        Resolve-RdpClientSessionSelection `
            -State $State `
            -EntryUserName 'other-user' `
            -SessionId ([uint32]2)
    } -Expected 'belongs to'

    $AmbiguousState = [pscustomobject]@{
        ConsoleSessionId = [uint64]2
        SingleSessionPerUser = 1
        Sessions = @(
            $TargetSession,
            [pscustomobject]@{
                Id = 4
                UserName = 'Administrator'
                DomainName = 'TEST-SERVER'
                State = 'Disconnected'
                Terminal = 'rdp'
            }
        )
    }
    Assert-ThrowsContaining -Action {
        Resolve-RdpClientSessionSelection `
            -State $AmbiguousState `
            -EntryUserName 'administrator' `
            -SessionId ([uint32]2)
    } -Expected 'multiple sessions'

    $NoConsoleState = [pscustomobject]@{
        ConsoleSessionId = [uint64][uint32]::MaxValue
        SingleSessionPerUser = 1
        Sessions = @($TargetSession)
    }
    Assert-ThrowsContaining -Action {
        Resolve-RdpClientShadowConsoleSession -State $NoConsoleState
    } -Expected 'no session attached'

    $EmptyConsole = [pscustomobject]@{
        Id = 5
        UserName = ''
        DomainName = ''
        SessionName = 'Console'
        State = 'Connected'
        Locked = $null
        Terminal = 'console'
        IsConsole = $true
        ClientName = ''
    }
    $EmptyConsoleState = [pscustomobject]@{
        ConsoleSessionId = [uint64]5
        SingleSessionPerUser = 1
        Sessions = @($EmptyConsole, $OtherSession)
    }
    if ((Get-RdpClientSessionDisplayUserName -Session $EmptyConsole) -ne '-') {
        throw 'An empty console username should render as a dash.'
    }
    Assert-ThrowsContaining -Action {
        Resolve-RdpClientShadowConsoleSession -State $EmptyConsoleState
    } -Expected 'no logged-on user desktop'

    $UnsafePolicyState = [pscustomobject]@{
        ConsoleSessionId = [uint64]2
        SingleSessionPerUser = 0
        Sessions = @($TargetSession)
    }
    Assert-ThrowsContaining -Action {
        Resolve-RdpClientSessionSelection `
            -State $UnsafePolicyState `
            -EntryUserName 'administrator' `
            -SessionId ([uint32]2)
    } -Expected 'fSingleSessionPerUser is 0'

    $Json = ConvertTo-Json -InputObject $State -Depth 5 -Compress
    $Marker = 'RDP_CLIENT_SESSION_STATE_V1:' + [Convert]::ToBase64String(
        [Text.Encoding]::UTF8.GetBytes($Json)
    )
function Invoke-RdpClientPeerSshPowerShell {
        param(
            [string]$SshEntryPath,
            [string]$RemoteSource,
            [int]$TimeoutSeconds
        )

        if (-not $RemoteSource.Contains('WTSEnumerateSessions') -or
            -not $RemoteSource.Contains('WTSGetActiveConsoleSessionId')) {
            throw 'The transported query must use structured WTS APIs.'
        }
        return [pscustomobject]@{
            ExitCode = 0
            Output = @($Marker)
        }
    }
    $Transported = Get-RdpClientPeerSessionState -SshEntryPath 'unused.cmd'
    if ($Transported.ComputerName -ne 'TEST-SERVER' -or
        @($Transported.Sessions).Count -ne 2) {
        throw 'The structured peer session state was not decoded correctly.'
    }

    $RemoteQuery = Join-Path $RuntimeRoot 'session-query.remote.ps1'
    $RemoteOutput = (& PowerShell.exe -NoLogo -NoProfile `
        -ExecutionPolicy Bypass -File $RemoteQuery 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 0 -or
        $RemoteOutput -notmatch 'RDP_CLIENT_SESSION_STATE_V1:[A-Za-z0-9+/=]+') {
        throw "The local WTS session query did not run successfully.`n$RemoteOutput"
    }

    [IO.Directory]::CreateDirectory($Runtime) | Out-Null
    [IO.File]::Copy($TemplateEntry, $Entry)
    [IO.File]::Copy((Join-Path $RuntimeRoot 'client.cmd'), (Join-Path $Runtime 'client.cmd'))
    [IO.File]::WriteAllText(
        $FakeSshEntry,
        "@echo off`r`nexit /b 0`r`n",
        (New-Object Text.UTF8Encoding($false))
    )
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

    $FakeConnect = @'
param(
    [string]$EntryFile,
    [string]$SshEntryFile,
    [string]$CommandName,
    [switch]$Launch,
    [switch]$Force,
    [string]$SessionId
)
[IO.File]::WriteAllLines($env:RDP_SESSION_CONNECT_CAPTURE, @(
    "EntryFile=$EntryFile",
    "SshEntryFile=$SshEntryFile",
    "CommandName=$CommandName",
    "Launch=$($Launch.IsPresent)",
    "SessionId=$SessionId"
))
exit 0
'@
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'connect.ps1'),
        $FakeConnect,
        (New-Object Text.UTF8Encoding($false))
    )
    $FakeList = @'
param([string]$SshEntryFile, [string]$RdpEntryFile, [string]$CommandName)
[IO.File]::WriteAllLines($env:RDP_SESSION_LIST_CAPTURE, @(
    "SshEntryFile=$SshEntryFile",
    "RdpEntryFile=$RdpEntryFile",
    "CommandName=$CommandName"
))
exit 0
'@
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'session-list.ps1'),
        $FakeList,
        (New-Object Text.UTF8Encoding($false))
    )

    $env:RDP_SESSION_CONNECT_CAPTURE = $ConnectCapture
    $env:RDP_SESSION_LIST_CAPTURE = $ListCapture
    Invoke-SessionTestEntry -Arguments @('.2') -ExpectedExitCode 0 | Out-Null
    $IdCapture = [IO.File]::ReadAllText($ConnectCapture)
    if (-not $IdCapture.Contains('Launch=True') -or
        -not $IdCapture.Contains('SessionId=2')) {
        throw "The .2 selector was not routed correctly.`n$IdCapture"
    }

    $env:RDP_CLIENT_SESSION_PARAMETER = '-SessionId 99'
    try {
        Invoke-SessionTestEntry -Arguments @() -ExpectedExitCode 0 | Out-Null
    } finally {
        Remove-Item Env:RDP_CLIENT_SESSION_PARAMETER -ErrorAction SilentlyContinue
    }
    $DefaultCapture = [IO.File]::ReadAllText($ConnectCapture)
    if (-not $DefaultCapture.Contains('Launch=True') -or
        -not $DefaultCapture.Contains("SessionId=`r`n")) {
        throw (
            'The default connection inherited an internal session selector ' +
            "from its caller.`n$DefaultCapture"
        )
    }

    Invoke-SessionTestEntry -Arguments @('.list') -ExpectedExitCode 0 | Out-Null
    $CapturedList = [IO.File]::ReadAllText($ListCapture)
    if (-not $CapturedList.Contains("SshEntryFile=$FakeSshEntry") -or
        -not $CapturedList.Contains("RdpEntryFile=$Entry")) {
        throw "The .list command was not routed correctly.`n$CapturedList"
    }

    foreach ($Invalid in @(
        [string[]]@('.2', 'unexpected'),
        [string[]]@('.list', 'unexpected')
    )) {
        $InvalidOutput = Invoke-SessionTestEntry `
            -Arguments $Invalid `
            -ExpectedExitCode 1
        if (-not $InvalidOutput.Contains('Session usage:')) {
            throw "Invalid session syntax should show session usage.`n$InvalidOutput"
        }
    }

    foreach ($RemovedCommand in @('.abc', '.console')) {
        $UnknownOutput = Invoke-SessionTestEntry `
            -Arguments @($RemovedCommand) `
            -ExpectedExitCode 1
        if (-not $UnknownOutput.Contains("Unknown RDP command: $RemovedCommand")) {
            throw "A removed or nonnumeric selector should be unknown.`n$UnknownOutput"
        }
    }

    Write-Host 'rdp client session tests: PASS' -ForegroundColor Green
} finally {
    Remove-Item Env:RDP_SESSION_CONNECT_CAPTURE -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_SESSION_LIST_CAPTURE -ErrorAction SilentlyContinue
    if ([IO.Directory]::Exists($ScratchRoot)) {
        [IO.Directory]::Delete($ScratchRoot, $true)
    }
}
