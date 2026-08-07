[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$OutputEncoding = New-Object Text.UTF8Encoding($false)

$RuntimeRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $RuntimeRoot 'peer-ssh.ps1')
. (Join-Path $RuntimeRoot 'session.ps1')
. (Join-Path $RuntimeRoot 'session-connect.ps1')

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

$TargetConsole = [pscustomobject]@{
    Id = 2
    UserName = 'Administrator'
    DomainName = 'TEST-SERVER'
    SessionName = 'Console'
    State = 'Active'
    Terminal = 'console'
    ClientName = ''
    ConnectTime = 100
}
$DetachedLanding = [pscustomobject]@{
    Id = 4
    UserName = 'Administrator'
    DomainName = 'TEST-SERVER'
    SessionName = 'RDP-Tcp#4'
    State = 'Disconnected'
    Terminal = 'rdp'
    ClientName = 'CLIENT'
    ConnectTime = 80
}
$BeforeState = [pscustomobject]@{
    Sessions = @($TargetConsole, $DetachedLanding)
}
$LandingSession = [pscustomobject]@{
    Id = 4
    UserName = 'Administrator'
    DomainName = 'TEST-SERVER'
    SessionName = 'RDP-Tcp#4'
    State = 'Active'
    Terminal = 'rdp'
    ClientName = 'CLIENT'
    ConnectTime = 200
}
$LandingState = [pscustomobject]@{
    Sessions = @($TargetConsole, $LandingSession)
}

$ResolvedLanding = Resolve-RdpClientLandingSession `
    -BeforeState $BeforeState `
    -CurrentState $LandingState `
    -EntryUserName 'administrator'
if ($ResolvedLanding.Id -ne 4) {
    throw 'The changed active RDP session must be selected as the landing session.'
}

$ReconnectedTarget = [pscustomobject]@{
    Id = 2
    UserName = 'Administrator'
    DomainName = 'TEST-SERVER'
    SessionName = 'RDP-Tcp#2'
    State = 'Active'
    Terminal = 'rdp'
    ClientName = 'CLIENT'
    ConnectTime = 210
}
$RoutedTarget = [pscustomobject]@{
    Id = 2
    UserName = 'Administrator'
    DomainName = 'TEST-SERVER'
    SessionName = 'RDP-Tcp#4'
    State = 'Active'
    Terminal = 'rdp'
    ClientName = 'CLIENT'
    ConnectTime = 211
}
$TargetLanding = Resolve-RdpClientLandingSession `
    -BeforeState ([pscustomobject]@{ Sessions = @($TargetConsole) }) `
    -CurrentState ([pscustomobject]@{ Sessions = @($ReconnectedTarget) }) `
    -EntryUserName 'administrator'
if ($TargetLanding.Id -ne 2 -or
    -not (Test-RdpClientSessionOwnsDestination `
        -Session $TargetLanding `
        -TargetSessionId ([uint32]2) `
        -DestinationSessionName 'RDP-Tcp#2')) {
    throw 'A direct reconnect to the requested ID must satisfy the postcondition.'
}

$UnchangedLanding = Resolve-RdpClientLandingSession `
    -BeforeState $LandingState `
    -CurrentState $LandingState `
    -EntryUserName 'administrator'
if ($null -ne $UnchangedLanding) {
    throw 'An unchanged pre-existing RDP session must not be claimed as this connection.'
}

$SecondLanding = [pscustomobject]@{
    Id = 5
    UserName = 'Administrator'
    DomainName = 'TEST-SERVER'
    SessionName = 'RDP-Tcp#5'
    State = 'Active'
    Terminal = 'rdp'
    ClientName = 'CLIENT'
    ConnectTime = 201
}
Assert-ThrowsContaining -Action {
    Resolve-RdpClientLandingSession `
        -BeforeState $BeforeState `
        -CurrentState ([pscustomobject]@{
            Sessions = @($TargetConsole, $LandingSession, $SecondLanding)
        }) `
        -EntryUserName 'administrator'
} -Expected 'Several RDP sessions changed'

$CapturedRouteRequest = $null
$RouteExitCode = 0
function Invoke-RdpClientPeerSshPowerShell {
    param(
        [string]$SshEntryPath,
        [string]$RemoteSource,
        [int]$TimeoutSeconds
    )

    if ($RemoteSource -notmatch
        "RdpClientSessionRequestBase64 = '(?<Request>[A-Za-z0-9+/=]+)'") {
        throw 'The transported route request was not framed safely.'
    }
    $script:CapturedRouteRequest = (
        [Text.Encoding]::UTF8.GetString(
            [Convert]::FromBase64String($Matches.Request)
        ) | ConvertFrom-Json
    )
    $Result = [ordered]@{
        Version = 1
        Action = [string]$script:CapturedRouteRequest.Action
        ExitCode = $script:RouteExitCode
        Output = $(if ($script:RouteExitCode -eq 0) { @() } else { @('denied') })
    }
    $Json = ConvertTo-Json -InputObject $Result -Compress
    $Marker = 'RDP_CLIENT_SESSION_ROUTE_V1:' + (
        [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Json))
    )
    return [pscustomobject]@{ ExitCode = 0; Output = @($Marker) }
}

$null = Invoke-RdpClientPeerSessionRoute `
    -SshEntryPath 'unused.cmd' `
    -Action 'connect' `
    -SessionId ([uint32]2) `
    -DestinationSessionName 'RDP-Tcp#4'
if ($CapturedRouteRequest.Action -ne 'connect' -or
    [int]$CapturedRouteRequest.SessionId -ne 2 -or
    $CapturedRouteRequest.DestinationSessionName -ne 'RDP-Tcp#4') {
    throw 'The exact-session route request was not transported correctly.'
}

$RouteExitCode = 5
Assert-ThrowsContaining -Action {
    Invoke-RdpClientPeerSessionRoute `
        -SshEntryPath 'unused.cmd' `
        -Action 'disconnect' `
        -SessionId ([uint32]4)
} -Expected 'exit code 5'
$RouteExitCode = 0

$RemoteRoute = [IO.File]::ReadAllText(
    (Join-Path $RuntimeRoot 'session-route.remote.ps1'),
    [Text.Encoding]::UTF8
)
if (-not $RemoteRoute.Contains('tscon.exe') -or
    -not $RemoteRoute.Contains('tsdiscon.exe')) {
    throw 'The peer route script must support exact connect and rollback disconnect.'
}

$RouteCalls = New-Object 'Collections.Generic.List[string]'
$RollbackDisconnected = $false
$MstscStopped = $false
function Wait-RdpClientLandingSession { return $LandingSession }
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
}
function Wait-RdpClientTargetSessionDestination { return $RoutedTarget }
function Disconnect-RdpClientSessionDestination {
    $script:RollbackDisconnected = $true
    return [uint32]4
}
function Stop-RdpClientStartedMstsc { $script:MstscStopped = $true }

$null = Connect-RdpClientSessionById `
    -SshEntryPath 'unused.cmd' `
    -BeforeState $BeforeState `
    -EntryUserName 'administrator' `
    -TargetSessionId ([uint32]2) `
    -MstscProcess ([pscustomobject]@{})
if ($RouteCalls.Count -ne 1 -or
    $RouteCalls[0] -ne 'connect:2:RDP-Tcp#4' -or
    $RollbackDisconnected -or
    $MstscStopped) {
    throw 'Successful exact-session routing did not follow the expected state machine.'
}

function Invoke-RdpClientPeerSessionRoute { throw 'simulated tscon failure' }
Assert-ThrowsContaining -Action {
    Connect-RdpClientSessionById `
        -SshEntryPath 'unused.cmd' `
        -BeforeState $BeforeState `
        -EntryUserName 'administrator' `
        -TargetSessionId ([uint32]2) `
        -MstscProcess ([pscustomobject]@{})
} -Expected 'simulated tscon failure'
if (-not $RollbackDisconnected -or -not $MstscStopped) {
    throw 'A failed tscon operation must disconnect the landing session and stop mstsc.'
}

Write-Host 'rdp client exact-session tests: PASS' -ForegroundColor Green
