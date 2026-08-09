[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$OutputEncoding = New-Object Text.UTF8Encoding($false)

$RuntimeRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $RuntimeRoot 'peer-ssh.ps1')
. (Join-Path $RuntimeRoot 'entry.ps1')
. (Join-Path $RuntimeRoot 'session.ps1')
. (Join-Path $RuntimeRoot 'session-connect.ps1')
. (Join-Path $RuntimeRoot 'shadow-console.ps1')

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

$EmptyConsole = [pscustomobject]@{
    Id = 5
    UserName = ''
    DomainName = ''
    SessionName = 'Console'
    State = 'Connected'
    Terminal = 'console'
    IsConsole = $true
    ClientName = ''
}
$ActiveConsole = [pscustomobject]@{
    Id = 2
    UserName = 'Administrator'
    DomainName = 'TEST-SERVER'
    SessionName = 'Console'
    State = 'Active'
    Terminal = 'console'
    IsConsole = $true
    ClientName = ''
}
$DisconnectedSession = [pscustomobject]@{
    Id = 3
    UserName = 'Administrator'
    DomainName = 'TEST-SERVER'
    SessionName = ''
    State = 'Disconnected'
    Terminal = 'detached'
    IsConsole = $false
    ClientName = 'CLIENT'
}
$LandingSession = [pscustomobject]@{
    Id = 6
    UserName = 'Administrator'
    DomainName = 'TEST-SERVER'
    SessionName = 'RDP-Tcp#6'
    State = 'Active'
    Terminal = 'rdp'
    IsConsole = $false
    ClientName = 'CLIENT'
    ConnectTime = 200
}
$LandingAtConsole = [pscustomobject]@{
    Id = 6
    UserName = 'Administrator'
    DomainName = 'TEST-SERVER'
    SessionName = 'Console'
    State = 'Active'
    Terminal = 'console'
    IsConsole = $true
    ClientName = ''
    ConnectTime = 200
}
$Session3AtConsole = [pscustomobject]@{
    Id = 3
    UserName = 'Administrator'
    DomainName = 'TEST-SERVER'
    SessionName = 'Console'
    State = 'Active'
    Terminal = 'console'
    IsConsole = $true
    ClientName = ''
}
$EmptyState = [pscustomobject]@{
    ConsoleSessionId = [uint64]5
    Sessions = @($EmptyConsole, $DisconnectedSession)
}
$OccupiedState = [pscustomobject]@{
    ConsoleSessionId = [uint64]2
    Sessions = @($ActiveConsole, $DisconnectedSession)
}

if ($null -ne (Get-RdpClientActiveConsoleSession -State $EmptyState)) {
    throw 'An empty Connected console must not be treated as an active desktop.'
}
if ((Get-RdpClientActiveConsoleSession -State $OccupiedState).Id -ne 2) {
    throw 'The active console user session was not detected.'
}
if ((Resolve-RdpClientConsoleTsconSession `
    -State $EmptyState `
    -SessionId '3').Id -ne 3) {
    throw 'An explicit tscon source session was not resolved.'
}

$CapturedGuardedSource = ''
function Invoke-RdpClientPeerSshPowerShell {
    param(
        [string]$SshEntryPath,
        [string]$RemoteSource,
        [int]$TimeoutSeconds
    )

    $script:CapturedGuardedSource = $RemoteSource
    $Result = [ordered]@{
        Version = 1
        Action = 'connect-console-if-empty'
        Outcome = 'console-occupied'
        ExitCode = 0
        Output = @()
    }
    $Json = ConvertTo-Json -InputObject $Result -Compress
    $Marker = 'RDP_CLIENT_SESSION_ROUTE_V1:' + (
        [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Json))
    )
    return [pscustomobject]@{ ExitCode = 0; Output = @($Marker) }
}

$GuardedResult = Invoke-RdpClientPeerSessionRoute `
    -SshEntryPath 'unused.cmd' `
    -Action 'connect-console-if-empty' `
    -SessionId ([uint32]6) `
    -DestinationSessionName 'console'
if ($GuardedResult.Outcome -ne 'console-occupied' -or
    -not $CapturedGuardedSource.Contains('WTSEnumerateSessions') -or
    -not $CapturedGuardedSource.Contains('connect-console-if-empty')) {
    throw 'Guarded console routing must query and route in one peer invocation.'
}

$ConnectSource = [IO.File]::ReadAllText(
    (Join-Path $RuntimeRoot 'connect.ps1'),
    [Text.Encoding]::UTF8
)
if (-not $ConnectSource.Contains('ReportMstscProcessId') -or
    -not $ConnectSource.Contains('RDP_CLIENT_MSTSC_PROCESS_V1:')) {
    throw 'Temporary ordinary RDP must report its exact mstsc process ID.'
}

$RouteCalls = New-Object 'Collections.Generic.List[string]'
$RouteMode = 'connected'
$WaitConsoleSession = $Session3AtConsole
$DisconnectCount = 0
$StopCount = 0
function Invoke-RdpClientPeerSessionRoute {
    param(
        [string]$SshEntryPath,
        [string]$Action,
        [uint32]$SessionId,
        [string]$DestinationSessionName
    )
    $script:RouteCalls.Add(('{0}:{1}:{2}' -f `
        $Action,
        $SessionId,
        $DestinationSessionName))
    if ($script:RouteMode -eq 'failure') {
        throw 'simulated console route failure'
    }
    return [pscustomobject]@{ Outcome = $script:RouteMode }
}
function Wait-RdpClientSessionAtConsole { return $script:WaitConsoleSession }
function Start-RdpClientDisplayBootstrap { return [pscustomobject]@{} }
function Wait-RdpClientLandingSession { return $LandingSession }
function Disconnect-RdpClientSessionDestination {
    $script:DisconnectCount++
    return [uint32]6
}
function Stop-RdpClientStartedMstsc { $script:StopCount++ }
function Get-RdpClientPeerSessionState { return $OccupiedState }

$Moved = Move-RdpClientSessionToConsole `
    -SshEntryPath 'unused.cmd' `
    -State $OccupiedState `
    -SessionId '3'
if ($Moved.Id -ne 3 -or
    $RouteCalls.Count -ne 1 -or
    $RouteCalls[0] -ne 'connect:3:console') {
    throw 'Explicit --tscon did not route the selected session to console.'
}

$RouteCalls.Clear()
$WaitConsoleSession = $ActiveConsole
$AlreadyConsoleState = [pscustomobject]@{
    ConsoleSessionId = [uint64]2
    Sessions = @($ActiveConsole)
}
$AlreadyConsole = Move-RdpClientSessionToConsole `
    -SshEntryPath 'unused.cmd' `
    -State $AlreadyConsoleState `
    -SessionId '2'
if ($AlreadyConsole.Id -ne 2 -or $RouteCalls.Count -ne 0) {
    throw 'An already-active console target must not invoke tscon again.'
}

$RouteMode = 'connected'
$WaitConsoleSession = $LandingAtConsole
$DisplaySession = Enable-RdpClientConsoleDisplay `
    -SshEntryPath 'unused.cmd' `
    -EntryFile 'unused.rdp.cmd' `
    -EntryUserName 'administrator' `
    -CommandName 'rdp' `
    -BeforeState $EmptyState
if ($DisplaySession.Id -ne 6 -or
    $RouteCalls[$RouteCalls.Count - 1] -ne
        'connect-console-if-empty:6:console' -or
    $StopCount -ne 1 -or
    $DisconnectCount -ne 0) {
    throw 'Console --display did not complete its guarded bootstrap path.'
}

$RouteMode = 'console-occupied'
$OccupiedResult = Enable-RdpClientConsoleDisplay `
    -SshEntryPath 'unused.cmd' `
    -EntryFile 'unused.rdp.cmd' `
    -EntryUserName 'administrator' `
    -CommandName 'rdp' `
    -BeforeState $EmptyState
if ($OccupiedResult.Id -ne 2 -or
    $DisconnectCount -ne 1 -or
    $StopCount -ne 2) {
    throw 'A newly occupied console must be preserved and the bootstrap disconnected.'
}

$RouteMode = 'failure'
Assert-ThrowsContaining -Action {
    Enable-RdpClientConsoleDisplay `
        -SshEntryPath 'unused.cmd' `
        -EntryFile 'unused.rdp.cmd' `
        -EntryUserName 'administrator' `
        -CommandName 'rdp' `
        -BeforeState $EmptyState
} -Expected 'simulated console route failure'
if ($DisconnectCount -ne 2 -or $StopCount -ne 3) {
    throw 'A failed guarded route must roll back its temporary RDP session and process.'
}

Write-Host 'rdp client Shadow console tests: PASS' -ForegroundColor Green
