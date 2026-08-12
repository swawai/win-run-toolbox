Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot 'process-job.ps1')

function Start-RdpClientDisplayBootstrap {
    param(
        [Parameter(Mandatory = $true)][string]$EntryFile,
        [Parameter(Mandatory = $true)][string]$SshEntryFile,
        [Parameter(Mandatory = $true)][string]$CommandName,
        [ValidateRange(1, 600)][int]$TimeoutSeconds = 120,
        [AllowNull()][pscustomobject]$TimeoutBudget
    )

    $ConnectScript = Join-Path $PSScriptRoot 'connect.ps1'
    if (-not [IO.File]::Exists($ConnectScript)) {
        throw "RDP connection script was not found: $ConnectScript"
    }
    if ($null -eq $TimeoutBudget) {
        $TimeoutBudget = New-RdpClientTimeoutBudget `
            -TimeoutSeconds $TimeoutSeconds
    }

    $Bootstrap = (
        '$ProgressPreference=''SilentlyContinue'';' +
        '& $env:RDP_CLIENT_DISPLAY_CONNECT_SCRIPT ' +
        '-EntryFile $env:RDP_CLIENT_DISPLAY_ENTRY ' +
        '-SshEntryFile $env:RDP_CLIENT_DISPLAY_SSH_ENTRY ' +
        '-CommandName $env:RDP_CLIENT_DISPLAY_COMMAND ' +
        '-Launch -ReportMstscProcessId 6>&1;' +
        'exit $LASTEXITCODE'
    )
    $EncodedBootstrap = [Convert]::ToBase64String(
        [Text.Encoding]::Unicode.GetBytes($Bootstrap)
    )
    $Utf8 = New-Object Text.UTF8Encoding($false)
    $Process = New-Object Diagnostics.Process
    $Started = $false
    try {
        $Process.StartInfo.FileName = Join-Path $PSHOME 'powershell.exe'
        $Process.StartInfo.Arguments = (
            '-NoLogo -NoProfile -NonInteractive -OutputFormat Text ' +
            '-ExecutionPolicy Bypass -EncodedCommand ' + $EncodedBootstrap
        )
        $Process.StartInfo.UseShellExecute = $false
        $Process.StartInfo.CreateNoWindow = $true
        $Process.StartInfo.RedirectStandardOutput = $true
        $Process.StartInfo.RedirectStandardError = $true
        $Process.StartInfo.StandardOutputEncoding = $Utf8
        $Process.StartInfo.StandardErrorEncoding = $Utf8
        [void]$Process.StartInfo.EnvironmentVariables
        $ChildEnvironment = $Process.StartInfo.EnvironmentVariables
        $ChildEnvironment['RDP_CLIENT_DISPLAY_CONNECT_SCRIPT'] = $ConnectScript
        $ChildEnvironment['RDP_CLIENT_DISPLAY_ENTRY'] = $EntryFile
        $ChildEnvironment['RDP_CLIENT_DISPLAY_SSH_ENTRY'] = $SshEntryFile
        $ChildEnvironment['RDP_CLIENT_DISPLAY_COMMAND'] = $CommandName

        $TimeoutSeconds = Get-RdpClientTimeoutBudgetRemainingSeconds `
            -Budget $TimeoutBudget `
            -Operation 'Desktop command'
        $Process.Start() | Out-Null
        $Started = $true
        $StdOutTask = $Process.StandardOutput.ReadToEndAsync()
        $StdErrTask = $Process.StandardError.ReadToEndAsync()
        if (-not $Process.WaitForExit($TimeoutSeconds * 1000)) {
            Stop-RdpClientProcessTree -Process $Process -Job $null
            throw (
                'Desktop command timed out after ' +
                "$($TimeoutBudget.TimeoutSeconds) seconds while starting " +
                'temporary RDP.'
            )
        }
        foreach ($ReadTask in @($StdOutTask, $StdErrTask)) {
            if (-not $ReadTask.Wait(5000)) {
                throw 'Temporary RDP bootstrap output did not close.'
            }
        }
        $Output = @(
            foreach ($Text in @($StdOutTask.Result, $StdErrTask.Result)) {
                @($Text -split '\r?\n' | Where-Object { $_.Length -gt 0 })
            }
        )
        $ExitCode = $Process.ExitCode
    } finally {
        if ($Started) {
            try {
                if (-not $Process.HasExited) {
                    Stop-RdpClientProcessTree -Process $Process -Job $null
                }
            } catch {
            }
        }
        $Process.Dispose()
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
        [AllowNull()][pscustomobject]$TimeoutBudget,
        [ValidateRange(0, 10000)][int]$StableMilliseconds = 3000
    )

    if ($null -eq $TimeoutBudget) {
        $TimeoutBudget = New-RdpClientTimeoutBudget `
            -TimeoutSeconds $TimeoutSeconds
    }
    $StableSince = $null
    $LastDetail = 'session was not observed'
    while ((Get-RdpClientTimeoutBudgetRemainingMilliseconds `
        -Budget $TimeoutBudget) -gt 0) {
        $RemainingSeconds = Get-RdpClientTimeoutBudgetRemainingSeconds `
            -Budget $TimeoutBudget `
            -Operation 'Desktop command'
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
                    $StableSince = $TimeoutBudget.Stopwatch.Elapsed.TotalMilliseconds
                }
                if (($TimeoutBudget.Stopwatch.Elapsed.TotalMilliseconds -
                    $StableSince) -ge
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
        $RemainingMilliseconds = Get-RdpClientTimeoutBudgetRemainingMilliseconds `
            -Budget $TimeoutBudget
        if ($RemainingMilliseconds -gt 0) {
            Start-Sleep -Milliseconds ([Math]::Min(500, $RemainingMilliseconds))
        }
    }
    throw (
        "Session $SessionId did not expose a stable interactive desktop " +
        "within $($TimeoutBudget.TimeoutSeconds) seconds. " +
        "Last observation: $LastDetail"
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
        [AllowNull()][pscustomobject]$TimeoutBudget,
        [ValidateRange(0, 10000)][int]$StableMilliseconds = 3000
    )

    if ($null -eq $TimeoutBudget) {
        $TimeoutBudget = New-RdpClientTimeoutBudget `
            -TimeoutSeconds $TimeoutSeconds
    }
    $MstscProcess = $null
    try {
        Write-Host "[RDP] Display:   starting temporary RDP for session $SessionId"
        $MstscProcess = Start-RdpClientDisplayBootstrap `
            -EntryFile $EntryFile `
            -SshEntryFile $SshEntryPath `
            -CommandName $CommandName `
            -TimeoutBudget $TimeoutBudget
        $Session = Connect-RdpClientSessionById `
            -SshEntryPath $SshEntryPath `
            -BeforeState $BeforeState `
            -EntryUserName $EntryUserName `
            -TargetSessionId $SessionId `
            -MstscProcess $MstscProcess `
            -TimeoutBudget $TimeoutBudget
        $Session = Wait-RdpClientSessionDisplayStable `
            -SshEntryPath $SshEntryPath `
            -SessionId $SessionId `
            -TimeoutBudget $TimeoutBudget `
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
