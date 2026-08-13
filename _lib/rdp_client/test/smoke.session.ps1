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
$DesktopCapture = Join-Path $ScratchRoot 'desktop.txt'

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
        ConnectTime = 100
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
        ConnectTime = 90
    }
    $State = [pscustomobject]@{
        Version = 1
        ComputerName = 'TEST-SERVER'
        ConsoleSessionId = [uint64]2
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

    $MultipleSessionState = [pscustomobject]@{
        ConsoleSessionId = [uint64]2
        Sessions = @(
            $TargetSession,
            [pscustomobject]@{
                Id = 4
                UserName = 'Administrator'
                DomainName = 'TEST-SERVER'
                SessionName = 'RDP-Tcp#4'
                State = 'Disconnected'
                Terminal = 'rdp'
                ClientName = 'CLIENT'
                ConnectTime = 80
            }
        )
    }
    $MultipleSelection = Resolve-RdpClientSessionSelection `
        -State $MultipleSessionState `
        -EntryUserName 'administrator' `
        -SessionId ([uint32]2)
    if ($MultipleSelection.Id -ne 2) {
        throw 'Exact-ID preflight must allow multiple sessions and policy 0.'
    }

    $NoConsoleState = [pscustomobject]@{
        ConsoleSessionId = [uint64][uint32]::MaxValue
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
        ConnectTime = 0
    }
    $EmptyConsoleState = [pscustomobject]@{
        ConsoleSessionId = [uint64]5
        Sessions = @($EmptyConsole, $OtherSession)
    }
    if ((Get-RdpClientSessionDisplayUserName -Session $EmptyConsole) -ne '-') {
        throw 'An empty console username should render as a dash.'
    }
    Assert-ThrowsContaining -Action {
        Resolve-RdpClientShadowConsoleSession -State $EmptyConsoleState
    } -Expected 'no logged-on user desktop'

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

        $script:ObservedSessionQueryTimeout = $TimeoutSeconds

        if (-not $RemoteSource.Contains('WTSEnumerateSessions') -or
            -not $RemoteSource.Contains('WTSGetActiveConsoleSessionId') -or
            -not $RemoteSource.Contains('ConnectTime')) {
            throw 'The transported query must use structured WTS APIs and connection time.'
        }
        return [pscustomobject]@{
            ExitCode = 0
            Output = @($Marker)
        }
    }
    $Transported = Get-RdpClientPeerSessionState -SshEntryPath 'unused.cmd'
    if ($Transported.ComputerName -ne 'TEST-SERVER' -or
        @($Transported.Sessions).Count -ne 2 -or
        $script:ObservedSessionQueryTimeout -ne 60) {
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
    $FakeDesktop = @'
param(
    [string]$Action,
    [string]$EntryFile,
    [string]$SshEntryFile,
    [string]$SessionId,
    [string]$CommandName,
    [string]$X,
    [string]$Y,
    [switch]$Display,
    [string]$Timeout,
    [string]$OutputPath,
    [string]$ScriptPath
)
[IO.File]::WriteAllLines($env:RDP_SESSION_DESKTOP_CAPTURE, @(
    "Action=$Action",
    "SessionId=$SessionId",
    "X=$X",
    "Y=$Y",
    "Display=$($Display.IsPresent)",
    "Timeout=$Timeout",
    "OutputPath=$OutputPath",
    "ScriptPath=$ScriptPath"
))
exit 0
'@
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'desktop.ps1'),
        $FakeDesktop,
        (New-Object Text.UTF8Encoding($false))
    )

    $env:RDP_SESSION_CONNECT_CAPTURE = $ConnectCapture
    $env:RDP_SESSION_LIST_CAPTURE = $ListCapture
    $env:RDP_SESSION_DESKTOP_CAPTURE = $DesktopCapture
    Invoke-SessionTestEntry -Arguments @('.2') -ExpectedExitCode 0 | Out-Null
    $IdCapture = [IO.File]::ReadAllText($ConnectCapture)
    if (-not $IdCapture.Contains('Launch=True') -or
        -not $IdCapture.Contains('SessionId=2')) {
        throw "The .2 selector was not routed correctly.`n$IdCapture"
    }

    Invoke-SessionTestEntry `
        -Arguments @('.2', 'connect') `
        -ExpectedExitCode 0 |
        Out-Null
    $ExplicitConnect = [IO.File]::ReadAllText($ConnectCapture)
    if (-not $ExplicitConnect.Contains('SessionId=2')) {
        throw "The explicit connect action was not routed correctly.`n$ExplicitConnect"
    }

    $ScreenshotOutput = Join-Path $ScratchRoot 'capture image.png'
    Invoke-SessionTestEntry `
        -Arguments @(
            '.2',
            'screenshot',
            '--display',
            '--timeout',
            '60s',
            '--output',
            $ScreenshotOutput
        ) `
        -ExpectedExitCode 0 |
        Out-Null
    $ScreenshotCapture = [IO.File]::ReadAllText($DesktopCapture)
    foreach ($Expected in @(
        'Action=screenshot',
        'SessionId=2',
        'Display=True',
        'Timeout=60s',
        "OutputPath=$ScreenshotOutput"
    )) {
        if (-not $ScreenshotCapture.Contains($Expected)) {
            throw "Screenshot syntax lost '$Expected'.`n$ScreenshotCapture"
        }
    }

    Invoke-SessionTestEntry `
        -Arguments @('.2', 'pixel', '640', '360', '--display') `
        -ExpectedExitCode 0 |
        Out-Null
    $PixelCapture = [IO.File]::ReadAllText($DesktopCapture)
    foreach ($Expected in @(
        'Action=pixel',
        'X=640',
        'Y=360',
        'Display=True'
    )) {
        if (-not $PixelCapture.Contains($Expected)) {
            throw "Pixel syntax lost '$Expected'.`n$PixelCapture"
        }
    }

    Invoke-SessionTestEntry `
        -Arguments @('.2', 'click', '12', '34') `
        -ExpectedExitCode 0 |
        Out-Null
    $ClickCapture = [IO.File]::ReadAllText($DesktopCapture)
    if (-not $ClickCapture.Contains('Action=click') -or
        -not $ClickCapture.Contains('X=12') -or
        -not $ClickCapture.Contains('Y=34')) {
        throw "Click syntax was not routed correctly.`n$ClickCapture"
    }

    $WorkflowPath = Join-Path $ScratchRoot 'workflow with spaces.ps1'
    Invoke-SessionTestEntry `
        -Arguments @(
            '.2',
            'script',
            $WorkflowPath,
            '--display',
            '--timeout',
            '60s'
        ) `
        -ExpectedExitCode 0 |
        Out-Null
    $ScriptCapture = [IO.File]::ReadAllText($DesktopCapture)
    foreach ($Expected in @(
        'Action=script',
        'SessionId=2',
        'Display=True',
        'Timeout=60s',
        "ScriptPath=$WorkflowPath"
    )) {
        if (-not $ScriptCapture.Contains($Expected)) {
            throw "Script syntax lost '$Expected'.`n$ScriptCapture"
        }
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
        [string[]]@('.2', 'connect', 'unexpected'),
        [string[]]@('.2', 'pixel', '640'),
        [string[]]@('.2', 'click', '640', '360', '--output', 'x.png'),
        [string[]]@('.2', 'script'),
        [string[]]@('.2', 'script', 'workflow.ps1', '--output', 'x.png'),
        [string[]]@('.2', 'screenshot', '--display', '--display'),
        [string[]]@('.2', 'screenshot', '--timeout'),
        [string[]]@('.2', 'screenshot', '--output'),
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
    Remove-Item Env:RDP_SESSION_DESKTOP_CAPTURE -ErrorAction SilentlyContinue
    if ([IO.Directory]::Exists($ScratchRoot)) {
        [IO.Directory]::Delete($ScratchRoot, $true)
    }
}
