Set-StrictMode -Version 2.0

function Get-RdpClientActiveConsoleSession {
    param([Parameter(Mandatory = $true)][pscustomobject]$State)

    $ConsoleId = [uint64]$State.ConsoleSessionId
    if ($ConsoleId -eq [uint32]::MaxValue) {
        return $null
    }
    $Targets = @($State.Sessions | Where-Object {
        [uint64]$_.Id -eq $ConsoleId -and [bool]$_.IsConsole
    })
    if ($Targets.Count -ne 1) {
        throw "The peer did not return console session $ConsoleId."
    }
    $Target = $Targets[0]
    if ([string]::IsNullOrWhiteSpace([string]$Target.UserName) -or
        -not [string]::Equals(
            [string]$Target.State,
            'Active',
            [StringComparison]::OrdinalIgnoreCase
        )) {
        return $null
    }
    return $Target
}

function Resolve-RdpClientConsoleTsconSession {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$State,
        [Parameter(Mandatory = $true)][string]$SessionId
    )

    $ResolvedId = Resolve-RdpClientSessionId -Value $SessionId
    if ($null -eq $ResolvedId) {
        throw 'A tscon source session ID is required.'
    }
    $Targets = @($State.Sessions | Where-Object {
        [uint64]$_.Id -eq [uint64]$ResolvedId
    })
    if ($Targets.Count -ne 1 -or
        [string]::IsNullOrWhiteSpace([string]$Targets[0].UserName)) {
        throw "The peer has no logged-on user session for session $ResolvedId."
    }
    return $Targets[0]
}

function Wait-RdpClientSessionAtConsole {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][uint32]$SessionId,
        [ValidateRange(1, 60)][int]$TimeoutSeconds = 15
    )

    $Stopwatch = [Diagnostics.Stopwatch]::StartNew()
    while ($Stopwatch.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        $State = Get-RdpClientPeerSessionState -SshEntryPath $SshEntryPath
        $Targets = @($State.Sessions | Where-Object {
            [uint64]$_.Id -eq [uint64]$SessionId -and
            [bool]$_.IsConsole -and
            -not [string]::IsNullOrWhiteSpace([string]$_.UserName) -and
            [string]::Equals(
                [string]$_.State,
                'Active',
                [StringComparison]::OrdinalIgnoreCase
            )
        })
        if ($Targets.Count -eq 1) {
            return $Targets[0]
        }
        Start-Sleep -Milliseconds 500
    }
    throw "Session $SessionId did not become the active console session."
}

function Move-RdpClientSessionToConsole {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][pscustomobject]$State,
        [Parameter(Mandatory = $true)][string]$SessionId
    )

    $Target = Resolve-RdpClientConsoleTsconSession `
        -State $State `
        -SessionId $SessionId
    if ([bool]$Target.IsConsole -and
        [string]::Equals(
            [string]$Target.State,
            'Active',
            [StringComparison]::OrdinalIgnoreCase
        )) {
        return $Target
    }

    $CurrentConsole = Get-RdpClientActiveConsoleSession -State $State
    if ($null -ne $CurrentConsole) {
        Write-Host (
            '[RDP] Replacing: console session {0} with session {1}' -f `
                $CurrentConsole.Id,
                $Target.Id
        )
    } else {
        Write-Host "[RDP] Switching: session $($Target.Id) -> console"
    }
    $null = Invoke-RdpClientPeerSessionRoute `
        -SshEntryPath $SshEntryPath `
        -Action 'connect' `
        -SessionId ([uint32]$Target.Id) `
        -DestinationSessionName 'console'
    return Wait-RdpClientSessionAtConsole `
        -SshEntryPath $SshEntryPath `
        -SessionId ([uint32]$Target.Id)
}

function Enable-RdpClientConsoleDisplay {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][string]$EntryFile,
        [Parameter(Mandatory = $true)][string]$EntryUserName,
        [Parameter(Mandatory = $true)][string]$CommandName,
        [Parameter(Mandatory = $true)][pscustomobject]$BeforeState
    )

    $MstscProcess = $null
    $Landing = $null
    $MovedToConsole = $false
    try {
        Write-Host '[RDP] Display:   console has no active user desktop; starting ordinary RDP'
        $MstscProcess = Start-RdpClientDisplayBootstrap `
            -EntryFile $EntryFile `
            -SshEntryFile $SshEntryPath `
            -CommandName $CommandName
        $Landing = Wait-RdpClientLandingSession `
            -SshEntryPath $SshEntryPath `
            -BeforeState $BeforeState `
            -EntryUserName $EntryUserName `
            -MstscProcess $MstscProcess

        $Route = Invoke-RdpClientPeerSessionRoute `
            -SshEntryPath $SshEntryPath `
            -Action 'connect-console-if-empty' `
            -SessionId ([uint32]$Landing.Id) `
            -DestinationSessionName 'console'
        if ([string]::Equals(
            [string]$Route.Outcome,
            'console-occupied',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            $null = Disconnect-RdpClientSessionDestination `
                -SshEntryPath $SshEntryPath `
                -EntryUserName $EntryUserName `
                -LandingSession $Landing
            Stop-RdpClientStartedMstsc -MstscProcess $MstscProcess
            $MstscProcess = $null
            $CurrentState = Get-RdpClientPeerSessionState `
                -SshEntryPath $SshEntryPath
            $ConsoleSession = Resolve-RdpClientShadowConsoleSession `
                -State $CurrentState
            Write-Host (
                '[RDP] Display:   console became active as session {0}; kept it' -f `
                    $ConsoleSession.Id
            )
            return $ConsoleSession
        }
        if (-not [string]::Equals(
            [string]$Route.Outcome,
            'connected',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            throw "Unexpected guarded console route outcome: $($Route.Outcome)"
        }

        $MovedToConsole = $true
        $ConsoleSession = Wait-RdpClientSessionAtConsole `
            -SshEntryPath $SshEntryPath `
            -SessionId ([uint32]$Landing.Id)
        Stop-RdpClientStartedMstsc -MstscProcess $MstscProcess
        $MstscProcess = $null
        Write-Host "[RDP] Display:   session $($Landing.Id) attached to console"
        return $ConsoleSession
    } catch {
        $Failure = $_.Exception.Message
        if (-not $MovedToConsole -and $null -ne $Landing) {
            try {
                $null = Disconnect-RdpClientSessionDestination `
                    -SshEntryPath $SshEntryPath `
                    -EntryUserName $EntryUserName `
                    -LandingSession $Landing
            } catch {
            }
        }
        if ($null -ne $MstscProcess) {
            Stop-RdpClientStartedMstsc -MstscProcess $MstscProcess
        }
        throw "Could not establish a console display for Shadow: $Failure"
    }
}
