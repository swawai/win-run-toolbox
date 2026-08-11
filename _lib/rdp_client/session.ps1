Set-StrictMode -Version 2.0

function Get-RdpClientPeerSessionState {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [ValidateRange(1, 600)][int]$TimeoutSeconds = 60
    )

    $RemoteScriptPath = Join-Path $PSScriptRoot 'session-query.remote.ps1'
    if (-not [IO.File]::Exists($RemoteScriptPath)) {
        throw "RDP session query script was not found: $RemoteScriptPath"
    }

    $RemoteSource = [IO.File]::ReadAllText(
        $RemoteScriptPath,
        [Text.Encoding]::UTF8
    )
    $Invocation = Invoke-RdpClientPeerSshPowerShell `
        -SshEntryPath $SshEntryPath `
        -RemoteSource $RemoteSource `
        -TimeoutSeconds $TimeoutSeconds
    if ($Invocation.ExitCode -ne 0) {
        throw "SSH session query failed with exit code $($Invocation.ExitCode)."
    }

    $MarkerPattern = '^RDP_CLIENT_SESSION_STATE_V1:(?<Payload>[A-Za-z0-9+/=]+)$'
    $Markers = @($Invocation.Output | Where-Object { $_ -match $MarkerPattern })
    if ($Markers.Count -ne 1 -or $Markers[0] -notmatch $MarkerPattern) {
        throw 'The peer did not return exactly one valid RDP session state.'
    }

    try {
        $Json = [Text.Encoding]::UTF8.GetString(
            [Convert]::FromBase64String($Matches.Payload)
        )
        $State = $Json | ConvertFrom-Json
    } catch {
        throw "The peer returned invalid RDP session state: $($_.Exception.Message)"
    }

    if ($null -eq $State -or
        $State -is [Array] -or
        $null -eq $State.PSObject.Properties['Version'] -or
        [int]$State.Version -ne 1 -or
        $null -eq $State.PSObject.Properties['ConsoleSessionId'] -or
        $null -eq $State.PSObject.Properties['Sessions']) {
        throw 'The peer returned an unsupported RDP session state document.'
    }
    return $State
}

function Get-RdpClientSessionDisplayUserName {
    param([Parameter(Mandatory = $true)][pscustomobject]$Session)

    if ([string]::IsNullOrWhiteSpace([string]$Session.UserName)) {
        return '-'
    }
    if ([string]::IsNullOrWhiteSpace([string]$Session.DomainName)) {
        return [string]$Session.UserName
    }
    return '{0}\{1}' -f $Session.DomainName, $Session.UserName
}

function Test-RdpClientSessionMatchesEntryUser {
    param(
        [Parameter(Mandatory = $true)][string]$EntryUserName,
        [Parameter(Mandatory = $true)][pscustomobject]$Session
    )

    $Configured = $EntryUserName.Trim()
    if ($Configured -match '^(?<Domain>[^\\]+)\\(?<User>[^\\]+)$') {
        if (-not [string]::Equals(
            $Matches.User,
            [string]$Session.UserName,
            [StringComparison]::OrdinalIgnoreCase
        )) {
            return $false
        }
        if ($Matches.Domain -eq '.') {
            return $true
        }
        return [string]::Equals(
            $Matches.Domain,
            [string]$Session.DomainName,
            [StringComparison]::OrdinalIgnoreCase
        )
    }

    $LeafUser = if ($Configured -match '^(?<User>[^@]+)@[^@]+$') {
        $Matches.User
    } else {
        $Configured
    }
    return [string]::Equals(
        $LeafUser,
        [string]$Session.UserName,
        [StringComparison]::OrdinalIgnoreCase
    )
}

function Resolve-RdpClientSessionSelection {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$State,
        [Parameter(Mandatory = $true)][string]$EntryUserName,
        [Parameter(Mandatory = $true)][uint32]$SessionId
    )

    $TargetId = [uint64]$SessionId

    $Sessions = @($State.Sessions)
    $Targets = @($Sessions | Where-Object { [uint64]$_.Id -eq $TargetId })
    if ($Targets.Count -ne 1) {
        throw "The peer has no logged-on user session for session $TargetId."
    }
    $Target = $Targets[0]

    if (-not (Test-RdpClientSessionMatchesEntryUser `
        -EntryUserName $EntryUserName `
        -Session $Target)) {
        $ActualUser = Get-RdpClientSessionDisplayUserName -Session $Target
        throw (
            "Selected session $TargetId belongs to $ActualUser, but this RDP " +
            "entry authenticates as $EntryUserName. Use that user's entry or " +
            ".shadow $TargetId."
        )
    }

    return $Target
}

function Resolve-RdpClientShadowConsoleSession {
    param([Parameter(Mandatory = $true)][pscustomobject]$State)

    $ConsoleId = [uint64]$State.ConsoleSessionId
    if ($ConsoleId -eq [uint32]::MaxValue) {
        throw 'The peer has no session attached to its physical console.'
    }

    $Targets = @($State.Sessions | Where-Object {
        [uint64]$_.Id -eq $ConsoleId -and [bool]$_.IsConsole
    })
    if ($Targets.Count -ne 1) {
        throw "The peer did not return console session $ConsoleId."
    }

    $Target = $Targets[0]
    if ([string]::IsNullOrWhiteSpace([string]$Target.UserName)) {
        throw (
            "Console session $ConsoleId has no logged-on user desktop, so it " +
            'cannot be shadowed.'
        )
    }
    if (-not [string]::Equals(
        [string]$Target.State,
        'Active',
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw (
            "Console session $ConsoleId is $($Target.State), not Active, so it " +
            'cannot be shadowed.'
        )
    }
    return $Target
}
