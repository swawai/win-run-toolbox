Set-StrictMode -Version 2.0

function Start-RdpClientDisplayBootstrap {
    param(
        [Parameter(Mandatory = $true)][string]$EntryFile,
        [Parameter(Mandatory = $true)][string]$SshEntryFile,
        [Parameter(Mandatory = $true)][string]$CommandName
    )

    $ConnectScript = Join-Path $PSScriptRoot 'connect.ps1'
    if (-not [IO.File]::Exists($ConnectScript)) {
        throw "RDP connection script was not found: $ConnectScript"
    }

    $PreviousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Output = @(& PowerShell.exe `
            -NoLogo `
            -NoProfile `
            -ExecutionPolicy Bypass `
            -File $ConnectScript `
            -EntryFile $EntryFile `
            -SshEntryFile $SshEntryFile `
            -CommandName $CommandName `
            -Launch `
            -ReportMstscProcessId 2>&1 | ForEach-Object { [string]$_ })
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousPreference
    }

    $MarkerPattern = '^RDP_CLIENT_MSTSC_PROCESS_V1:(?<ProcessId>[0-9]+)$'
    $Markers = @($Output | Where-Object { $_ -match $MarkerPattern })
    foreach ($Line in @($Output | Where-Object { $_ -notmatch $MarkerPattern })) {
        Write-Host $Line
    }
    if ($ExitCode -ne 0) {
        throw "The temporary ordinary RDP connection failed with exit code $ExitCode."
    }
    if ($Markers.Count -ne 1 -or $Markers[0] -notmatch $MarkerPattern) {
        throw 'The temporary ordinary RDP connection did not report its mstsc process.'
    }

    $ProcessId = [int]0
    if (-not [int]::TryParse($Matches.ProcessId, [ref]$ProcessId) -or
        $ProcessId -le 0) {
        throw 'The temporary ordinary RDP connection reported an invalid process ID.'
    }
    try {
        return [Diagnostics.Process]::GetProcessById($ProcessId)
    } catch {
        throw 'The temporary mstsc process exited before its session could be identified.'
    }
}

function Test-RdpClientSessionDisplayReady {
    param([Parameter(Mandatory = $true)][pscustomobject]$Session)

    if (-not [string]::Equals(
        [string]$Session.State,
        'Active',
        [StringComparison]::OrdinalIgnoreCase
    )) {
        return $false
    }

    $LockedProperty = $Session.PSObject.Properties['Locked']
    return $null -eq $LockedProperty -or $null -eq $LockedProperty.Value -or
        -not [bool]$LockedProperty.Value
}

function Wait-RdpClientSessionDisplayStable {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][uint32]$SessionId,
        [ValidateRange(1, 600)][int]$TimeoutSeconds = 120,
        [ValidateRange(0, 10000)][int]$StableMilliseconds = 3000
    )

    $Stopwatch = [Diagnostics.Stopwatch]::StartNew()
    $StableSince = $null
    $LastDetail = 'session was not observed'
    while ($Stopwatch.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        $RemainingSeconds = [Math]::Max(
            1,
            [Math]::Ceiling($TimeoutSeconds - $Stopwatch.Elapsed.TotalSeconds)
        )
        try {
            $State = Get-RdpClientPeerSessionState `
                -SshEntryPath $SshEntryPath `
                -TimeoutSeconds ([Math]::Min(20, $RemainingSeconds))
            $Targets = @($State.Sessions | Where-Object {
                [uint64]$_.Id -eq [uint64]$SessionId
            })
            if ($Targets.Count -eq 1 -and
                (Test-RdpClientSessionDisplayReady -Session $Targets[0])) {
                if ($null -eq $StableSince) {
                    $StableSince = $Stopwatch.Elapsed.TotalMilliseconds
                }
                if (($Stopwatch.Elapsed.TotalMilliseconds - $StableSince) -ge
                    $StableMilliseconds) {
                    return $Targets[0]
                }
                $LastDetail = 'desktop was not stable for the required window'
            } else {
                $StableSince = $null
                $LastDetail = if ($Targets.Count -eq 1) {
                    "state=$($Targets[0].State) locked=$($Targets[0].Locked)"
                } else {
                    "matching sessions=$($Targets.Count)"
                }
            }
        } catch {
            $StableSince = $null
            $LastDetail = $_.Exception.Message
        }
        Start-Sleep -Milliseconds 500
    }
    throw (
        "Session $SessionId did not expose a stable interactive desktop " +
        "within $TimeoutSeconds seconds. Last observation: $LastDetail"
    )
}

function Open-RdpClientSessionDisplayLease {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][string]$EntryFile,
        [Parameter(Mandatory = $true)][string]$EntryUserName,
        [Parameter(Mandatory = $true)][string]$CommandName,
        [Parameter(Mandatory = $true)][pscustomobject]$BeforeState,
        [Parameter(Mandatory = $true)][uint32]$SessionId,
        [ValidateRange(1, 600)][int]$TimeoutSeconds = 120,
        [ValidateRange(0, 10000)][int]$StableMilliseconds = 3000
    )

    $MstscProcess = $null
    try {
        Write-Host "[RDP] Display:   starting temporary RDP for session $SessionId"
        $MstscProcess = Start-RdpClientDisplayBootstrap `
            -EntryFile $EntryFile `
            -SshEntryFile $SshEntryPath `
            -CommandName $CommandName
        $Session = Connect-RdpClientSessionById `
            -SshEntryPath $SshEntryPath `
            -BeforeState $BeforeState `
            -EntryUserName $EntryUserName `
            -TargetSessionId $SessionId `
            -MstscProcess $MstscProcess `
            -TimeoutSeconds $TimeoutSeconds
        $Session = Wait-RdpClientSessionDisplayStable `
            -SshEntryPath $SshEntryPath `
            -SessionId $SessionId `
            -TimeoutSeconds $TimeoutSeconds `
            -StableMilliseconds $StableMilliseconds
        Write-Host (
            '[RDP] Display:   session {0} stable for {1:N1}s' -f `
                $SessionId,
                ($StableMilliseconds / 1000)
        )
        return [pscustomobject]@{
            Session      = $Session
            MstscProcess = $MstscProcess
        }
    } catch {
        if ($null -ne $MstscProcess) {
            Stop-RdpClientStartedMstsc -MstscProcess $MstscProcess
        }
        throw
    }
}

function Close-RdpClientSessionDisplayLease {
    param([AllowNull()]$Lease)

    if ($null -eq $Lease -or $null -eq $Lease.MstscProcess) {
        return
    }
    Stop-RdpClientStartedMstsc -MstscProcess $Lease.MstscProcess
    Write-Host '[RDP] Display:   temporary RDP closed'
}
