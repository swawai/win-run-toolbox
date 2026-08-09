Set-StrictMode -Version 2.0

function Get-RdpClientSessionPropertyText {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Session,
        [Parameter(Mandatory = $true)][string]$Name
    )

    $Property = $Session.PSObject.Properties[$Name]
    if ($null -eq $Property -or $null -eq $Property.Value) {
        return ''
    }
    return [string]$Property.Value
}

function Test-RdpClientSessionChangedSinceSnapshot {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Session,
        [Parameter(Mandatory = $true)][pscustomobject]$BeforeState
    )

    $Before = @($BeforeState.Sessions | Where-Object {
        [uint64]$_.Id -eq [uint64]$Session.Id
    })
    if ($Before.Count -eq 0) {
        return $true
    }
    if ($Before.Count -ne 1) {
        throw "The initial session snapshot contains duplicate ID $($Session.Id)."
    }

    foreach ($Name in @('State', 'SessionName', 'ClientName', 'ConnectTime')) {
        if ((Get-RdpClientSessionPropertyText -Session $Session -Name $Name) -ne
            (Get-RdpClientSessionPropertyText -Session $Before[0] -Name $Name)) {
            return $true
        }
    }
    return $false
}

function Resolve-RdpClientLandingSession {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$BeforeState,
        [Parameter(Mandatory = $true)][pscustomobject]$CurrentState,
        [Parameter(Mandatory = $true)][string]$EntryUserName
    )

    $Candidates = @($CurrentState.Sessions | Where-Object {
        [string]::Equals(
            [string]$_.State,
            'Active',
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [string]::Equals(
            [string]$_.Terminal,
            'rdp',
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        (Test-RdpClientSessionMatchesEntryUser `
            -EntryUserName $EntryUserName `
            -Session $_) -and
        (Test-RdpClientSessionChangedSinceSnapshot `
            -Session $_ `
            -BeforeState $BeforeState)
    })

    if ($Candidates.Count -eq 0) {
        return $null
    }
    if ($Candidates.Count -ne 1) {
        $Ids = @($Candidates | ForEach-Object { [string]$_.Id }) -join ', '
        throw (
            'Several RDP sessions changed while mstsc was connecting ' +
            "($Ids), so this connection cannot be identified safely."
        )
    }
    return $Candidates[0]
}

function Test-RdpClientSessionOwnsDestination {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Session,
        [Parameter(Mandatory = $true)][uint32]$TargetSessionId,
        [Parameter(Mandatory = $true)][string]$DestinationSessionName
    )

    return (
        [uint64]$Session.Id -eq [uint64]$TargetSessionId -and
        [string]::Equals(
            [string]$Session.State,
            'Active',
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [string]::Equals(
            [string]$Session.Terminal,
            'rdp',
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [string]::Equals(
            [string]$Session.SessionName,
            $DestinationSessionName,
            [StringComparison]::OrdinalIgnoreCase
        )
    )
}

function Invoke-RdpClientPeerSessionRoute {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)]
        [ValidateSet('connect', 'connect-console-if-empty', 'disconnect')]
        [string]$Action,
        [Parameter(Mandatory = $true)][uint32]$SessionId,
        [AllowEmptyString()][string]$DestinationSessionName = ''
    )

    $RemoteScriptPath = Join-Path $PSScriptRoot 'session-route.remote.ps1'
    if (-not [IO.File]::Exists($RemoteScriptPath)) {
        throw "RDP session route script was not found: $RemoteScriptPath"
    }
    if ($Action -in @('connect', 'connect-console-if-empty') -and
        [string]::IsNullOrWhiteSpace($DestinationSessionName)) {
        throw 'An RDP destination session name is required.'
    }

    $Request = [ordered]@{
        Action                 = $Action
        SessionId              = [uint64]$SessionId
        DestinationSessionName = $DestinationSessionName
    }
    $RequestJson = ConvertTo-Json -InputObject $Request -Compress
    $RequestBase64 = [Convert]::ToBase64String(
        [Text.Encoding]::UTF8.GetBytes($RequestJson)
    )
    $RemoteParts = New-Object 'Collections.Generic.List[string]'
    $RemoteParts.Add(
        '$RdpClientSessionRequestBase64 = ''' + $RequestBase64 + "'"
    )
    if ($Action -eq 'connect-console-if-empty') {
        $QueryScriptPath = Join-Path $PSScriptRoot 'session-query.remote.ps1'
        if (-not [IO.File]::Exists($QueryScriptPath)) {
            throw "RDP session query script was not found: $QueryScriptPath"
        }
        $RemoteParts.Add(
            [IO.File]::ReadAllText($QueryScriptPath, [Text.Encoding]::UTF8)
        )
    }
    $RemoteParts.Add(
        [IO.File]::ReadAllText($RemoteScriptPath, [Text.Encoding]::UTF8)
    )
    $RemoteSource = $RemoteParts -join "`r`n"
    $Invocation = Invoke-RdpClientPeerSshPowerShell `
        -SshEntryPath $SshEntryPath `
        -RemoteSource $RemoteSource `
        -TimeoutSeconds 20
    if ($Invocation.ExitCode -ne 0) {
        throw "SSH session route failed with exit code $($Invocation.ExitCode)."
    }

    $MarkerPattern = '^RDP_CLIENT_SESSION_ROUTE_V1:(?<Payload>[A-Za-z0-9+/=]+)$'
    $Markers = @($Invocation.Output | Where-Object { $_ -match $MarkerPattern })
    if ($Markers.Count -ne 1 -or $Markers[0] -notmatch $MarkerPattern) {
        throw 'The peer did not return exactly one valid RDP session route result.'
    }
    try {
        $Json = [Text.Encoding]::UTF8.GetString(
            [Convert]::FromBase64String($Matches.Payload)
        )
        $Result = $Json | ConvertFrom-Json
    } catch {
        throw "The peer returned an invalid RDP session route result: $($_.Exception.Message)"
    }
    if ($null -eq $Result -or
        $Result -is [Array] -or
        $null -eq $Result.PSObject.Properties['Version'] -or
        [int]$Result.Version -ne 1 -or
        $null -eq $Result.PSObject.Properties['ExitCode'] -or
        $null -eq $Result.PSObject.Properties['Output']) {
        throw 'The peer returned an unsupported RDP session route result.'
    }
    if ([int]$Result.ExitCode -ne 0) {
        $Detail = @($Result.Output) -join [Environment]::NewLine
        $Suffix = if ([string]::IsNullOrWhiteSpace($Detail)) {
            ''
        } else {
            [Environment]::NewLine + $Detail.Trim()
        }
        throw "Peer $Action command failed with exit code $($Result.ExitCode).$Suffix"
    }
    return $Result
}

function Wait-RdpClientLandingSession {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][pscustomobject]$BeforeState,
        [Parameter(Mandatory = $true)][string]$EntryUserName,
        [Parameter(Mandatory = $true)]$MstscProcess,
        [ValidateRange(1, 600)][int]$TimeoutSeconds = 120
    )

    $Stopwatch = [Diagnostics.Stopwatch]::StartNew()
    while ($Stopwatch.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        $CurrentState = Get-RdpClientPeerSessionState `
            -SshEntryPath $SshEntryPath
        $Landing = Resolve-RdpClientLandingSession `
            -BeforeState $BeforeState `
            -CurrentState $CurrentState `
            -EntryUserName $EntryUserName
        if ($null -ne $Landing) {
            return $Landing
        }
        try {
            $MstscProcess.Refresh()
            if ($MstscProcess.HasExited) {
                throw 'mstsc exited before an RDP session became active.'
            }
        } catch [InvalidOperationException] {
            throw 'mstsc exited before an RDP session became active.'
        }
        Start-Sleep -Milliseconds 1000
    }
    throw "Timed out after $TimeoutSeconds seconds waiting for mstsc to connect."
}

function Wait-RdpClientTargetSessionDestination {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][uint32]$TargetSessionId,
        [Parameter(Mandatory = $true)][string]$DestinationSessionName,
        [ValidateRange(1, 60)][int]$TimeoutSeconds = 15
    )

    $Stopwatch = [Diagnostics.Stopwatch]::StartNew()
    while ($Stopwatch.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        $State = Get-RdpClientPeerSessionState -SshEntryPath $SshEntryPath
        $Targets = @($State.Sessions | Where-Object {
            Test-RdpClientSessionOwnsDestination `
                -Session $_ `
                -TargetSessionId $TargetSessionId `
                -DestinationSessionName $DestinationSessionName
        })
        if ($Targets.Count -eq 1) {
            return $Targets[0]
        }
        Start-Sleep -Milliseconds 500
    }
    throw (
        "Session $TargetSessionId did not take over destination " +
        "$DestinationSessionName within $TimeoutSeconds seconds."
    )
}

function Disconnect-RdpClientSessionDestination {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][string]$EntryUserName,
        [Parameter(Mandatory = $true)][pscustomobject]$LandingSession
    )

    $State = Get-RdpClientPeerSessionState -SshEntryPath $SshEntryPath
    $Owners = @($State.Sessions | Where-Object {
        [string]::Equals(
            [string]$_.SessionName,
            [string]$LandingSession.SessionName,
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [string]::Equals(
            [string]$_.Terminal,
            'rdp',
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        (Test-RdpClientSessionMatchesEntryUser `
            -EntryUserName $EntryUserName `
            -Session $_)
    })
    if ($Owners.Count -eq 0) {
        return $null
    }
    if ($Owners.Count -ne 1) {
        throw "Destination $($LandingSession.SessionName) has several owners."
    }
    if (-not [string]::IsNullOrWhiteSpace([string]$LandingSession.ClientName) -and
        -not [string]::Equals(
            [string]$Owners[0].ClientName,
            [string]$LandingSession.ClientName,
            [StringComparison]::OrdinalIgnoreCase
        )) {
        throw "Destination $($LandingSession.SessionName) changed clients; rollback was skipped."
    }

    $null = Invoke-RdpClientPeerSessionRoute `
        -SshEntryPath $SshEntryPath `
        -Action 'disconnect' `
        -SessionId ([uint32]$Owners[0].Id)
    return [uint32]$Owners[0].Id
}

function Stop-RdpClientStartedMstsc {
    param([Parameter(Mandatory = $true)]$MstscProcess)

    try {
        $MstscProcess.Refresh()
        if ($MstscProcess.HasExited) {
            return
        }
        if ($MstscProcess.CloseMainWindow()) {
            if ($MstscProcess.WaitForExit(1500)) {
                return
            }
        }
        $MstscProcess.Kill()
    } catch {
    }
}

function Connect-RdpClientSessionById {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][pscustomobject]$BeforeState,
        [Parameter(Mandatory = $true)][string]$EntryUserName,
        [Parameter(Mandatory = $true)][uint32]$TargetSessionId,
        [Parameter(Mandatory = $true)]$MstscProcess
    )

    $Landing = $null
    try {
        Write-Host "[RDP] Waiting:   session $TargetSessionId"
        $Landing = Wait-RdpClientLandingSession `
            -SshEntryPath $SshEntryPath `
            -BeforeState $BeforeState `
            -EntryUserName $EntryUserName `
            -MstscProcess $MstscProcess

        if ([uint64]$Landing.Id -ne [uint64]$TargetSessionId) {
            Write-Host (
                '[RDP] Switching: landing session {0} -> requested session {1}' -f `
                    $Landing.Id,
                    $TargetSessionId
            )
            $null = Invoke-RdpClientPeerSessionRoute `
                -SshEntryPath $SshEntryPath `
                -Action 'connect' `
                -SessionId $TargetSessionId `
                -DestinationSessionName ([string]$Landing.SessionName)
            $FinalSession = Wait-RdpClientTargetSessionDestination `
                -SshEntryPath $SshEntryPath `
                -TargetSessionId $TargetSessionId `
                -DestinationSessionName ([string]$Landing.SessionName)
        } else {
            $FinalSession = $Landing
        }

        Write-Host (
            '[RDP] Connected: session {0} ({1})' -f `
                $FinalSession.Id,
                $FinalSession.SessionName
        )
        return $FinalSession
    } catch {
        $Failure = $_.Exception.Message
        $Rollback = New-Object 'Collections.Generic.List[string]'
        if ($null -ne $Landing) {
            try {
                $DisconnectedId = Disconnect-RdpClientSessionDestination `
                    -SshEntryPath $SshEntryPath `
                    -EntryUserName $EntryUserName `
                    -LandingSession $Landing
                if ($null -ne $DisconnectedId) {
                    $Rollback.Add("disconnected peer session $DisconnectedId")
                }
            } catch {
                $Rollback.Add("peer disconnect failed: $($_.Exception.Message)")
            }
        }
        Stop-RdpClientStartedMstsc -MstscProcess $MstscProcess
        $Rollback.Add('closed the mstsc process started by this command')
        throw (
            "Exact session connection failed: $Failure" +
            [Environment]::NewLine +
            'Rollback: ' + ($Rollback -join '; ')
        )
    }
}
