[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$OutputEncoding = New-Object Text.UTF8Encoding($false)

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$RuntimeRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$ScratchRoot = Join-Path (Join-Path $RepoRoot 'data\rdp-client') (
    '.desktop-test-' + [Guid]::NewGuid().ToString('N')
)
$Runtime = Join-Path $ScratchRoot '_lib\rdp_client'
$Entry = Join-Path $ScratchRoot 'account.rdp.cmd'
$SshEntry = Join-Path $ScratchRoot 'peer.ssh.cmd'
$OutputPath = Join-Path $ScratchRoot 'capture.png'
$ExistingOutputPath = Join-Path $ScratchRoot 'existing.png'
$WorkflowPath = Join-Path $ScratchRoot 'workflow.ps1'
$WorkflowBeforePath = Join-Path $ScratchRoot 'workflow-before.png'
$WorkflowAfterPath = Join-Path $ScratchRoot 'workflow-after.png'
$CloseCapture = Join-Path $ScratchRoot 'display-closed.txt'
$TimeoutCapture = Join-Path $ScratchRoot 'timeout-budget.txt'
$BootstrapRuntime = Join-Path $ScratchRoot 'bootstrap-runtime'
$BootstrapPidPath = Join-Path $ScratchRoot 'bootstrap-child.pid'

function Assert-DesktopSourceParses {
    param([Parameter(Mandatory = $true)][string]$Path)

    $Tokens = $null
    $Errors = $null
    [void][Management.Automation.Language.Parser]::ParseFile(
        $Path,
        [ref]$Tokens,
        [ref]$Errors
    )
    if ($Errors.Count -gt 0) {
        throw "$Path does not parse: $($Errors[0].Message)"
    }
}

function Assert-DesktopScriptRejected {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Expected
    )

    try {
        Read-RdpClientDesktopScript -Path $Path | Out-Null
        throw "Expected desktop script failure containing '$Expected'."
    } catch {
        if (-not $_.Exception.Message.Contains($Expected)) {
            throw
        }
    }
}

function Invoke-DesktopTestCommand {
    param(
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][int]$ExpectedExitCode
    )

    $PreviousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Output = (& PowerShell.exe `
            -NoLogo `
            -NoProfile `
            -ExecutionPolicy Bypass `
            -File (Join-Path $Runtime 'desktop.ps1') `
            @Arguments 2>&1 | Out-String)
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousPreference
    }
    if ($ExitCode -ne $ExpectedExitCode) {
        throw "Unexpected desktop exit code $ExitCode.`n$Output"
    }
    return $Output
}

function New-FakeDesktopResultPayload {
    param(
        [Parameter(Mandatory = $true)][string]$Action,
        [switch]$Success
    )

    $Result = [ordered]@{
        Version = 1
        Success = $Success.IsPresent
        Action = $Action
    }
    if ($Success) {
        $Result.DesktopName = 'Default'
        $Result.OriginX = 0
        $Result.OriginY = 0
        $Result.Width = 1200
        $Result.Height = 800
        $Png = [Convert]::ToBase64String([byte[]]@(
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
            0x01, 0x02, 0x03, 0x04
        ))
        if ($Action -eq 'screenshot') {
            $Result.ImageBase64 = $Png
        } elseif ($Action -eq 'script') {
            $Result.Steps = @(
                [ordered]@{ Index = 1; Action = 'screenshot'; ImageBase64 = $Png },
                [ordered]@{ Index = 2; Action = 'pixel'; X = 640; Y = 360; Color = '#123456' },
                [ordered]@{ Index = 3; Action = 'click'; X = 380; Y = 155 },
                [ordered]@{ Index = 4; Action = 'wait'; Milliseconds = 5 },
                [ordered]@{ Index = 5; Action = 'screenshot'; ImageBase64 = $Png }
            )
        } else {
            $Result.X = 640
            $Result.Y = 360
            if ($Action -eq 'pixel') {
                $Result.Color = '#123456'
            }
        }
    } else {
        $Result.ErrorCode = 'DISPLAY_NOT_READY'
        $Result.Error = 'fake failure'
    }
    $Json = ConvertTo-Json -InputObject $Result -Compress -Depth 4
    return [Convert]::ToBase64String(
        [Text.Encoding]::UTF8.GetBytes($Json)
    )
}

try {
    foreach ($Name in @(
        'desktop.ps1',
        'desktop-script.ps1',
        'desktop-task.remote.ps1',
        'session-display.ps1',
        'psexec.remote.ps1',
        'psexec-lib.remote.ps1'
    )) {
        Assert-DesktopSourceParses -Path (Join-Path $RuntimeRoot $Name)
    }

    $TaskSource = [IO.File]::ReadAllText(
        (Join-Path $RuntimeRoot 'desktop-task.remote.ps1'),
        [Text.Encoding]::UTF8
    )
    foreach ($Expected in @(
        'OpenInputDesktop',
        'GetSystemMetrics',
        'CopyFromScreen',
        'SetCursorPos',
        'ProcessIdToSessionId',
        'WTSQuerySessionInformationW',
        '[IO.FileMode]::CreateNew',
        'ResultPath',
        'ProcessIdentityPath',
        'StartTimeUtcTicks',
        'RDP_CLIENT_DESKTOP_RESULT_V1:',
        'SESSION_CHANGED',
        'DESKTOP_NOT_INTERACTIVE',
        'COORDINATE_OUT_OF_RANGE',
        'WORKFLOW_STEP_FAILED',
        'Start-Sleep -Milliseconds'
    )) {
        if (-not $TaskSource.Contains($Expected)) {
            throw "The desktop worker is missing '$Expected'."
        }
    }
    if ($TaskSource -notmatch
        "(?s)\`$NativeSource = @'\r?\n(?<CSharp>.+?)\r?\n'@") {
        throw 'The desktop worker C# source was not found.'
    }
    Add-Type -TypeDefinition $Matches.CSharp -Language CSharp

    $PsExecSource = [IO.File]::ReadAllText(
        (Join-Path $RuntimeRoot 'psexec.remote.ps1'),
        [Text.Encoding]::UTF8
    )
    $PsExecSource += [IO.File]::ReadAllText(
        (Join-Path $RuntimeRoot 'psexec-lib.remote.ps1'),
        [Text.Encoding]::UTF8
    )
    foreach ($Expected in @(
        "`$Request.Action -eq 'desktop'",
        'DesktopRequestBase64',
        "Join-Path `$ManagedDirectory 'desktop-task.ps1'",
        "'-RequestBase64'",
        "'-ResultPath'",
        "'-d'",
        'DesktopTimeoutSeconds',
        'Wait-RdpClientDesktopResultFile',
        'Invoke-RdpClientUncapturedProcess',
        'Stop-RdpClientDesktopWorkerProcess',
        'StartTime.ToUniversalTime().Ticks',
        'Wait-RdpClientDesktopWorkerIdentityFile',
        'worker-created identity file is the start authority',
        '.desktop-identity-',
        '.desktop-result-',
        'ReadAllText',
        "'-i'",
        "'-WindowStyle'",
        "'-s'"
    )) {
        if (-not $PsExecSource.Contains($Expected)) {
            throw "The PsExec desktop path is missing '$Expected'."
        }
    }

    . (Join-Path $RuntimeRoot 'session-display.ps1')
    $StartedProcess = [pscustomobject]@{ Id = 42 }
    $FinalSession = [pscustomobject]@{
        Id = 2; State = 'Active'; Locked = $false
    }
    $Stopped = $false
    function Start-RdpClientDisplayBootstrap { return $StartedProcess }
    function Connect-RdpClientSessionById { return $FinalSession }
    function Get-RdpClientPeerSessionState {
        param([string]$SshEntryPath, [int]$TimeoutSeconds)
        return [pscustomobject]@{ Sessions = @($FinalSession) }
    }
    function Stop-RdpClientStartedMstsc { $script:Stopped = $true }

    $Lease = Open-RdpClientSessionDisplayLease `
        -SshEntryPath 'unused.cmd' `
        -EntryFile 'unused.cmd' `
        -EntryUserName 'administrator' `
        -CommandName 'rdp' `
        -BeforeState ([pscustomobject]@{ Sessions = @() }) `
        -SessionId ([uint32]2) `
        -StableMilliseconds 1
    if ($Lease.Session.Id -ne 2 -or $Lease.MstscProcess.Id -ne 42) {
        throw 'The desktop lease did not retain its session and mstsc process.'
    }
    Close-RdpClientSessionDisplayLease -Lease $Lease
    if (-not $Stopped) {
        throw 'Closing a desktop lease must stop its owned mstsc process.'
    }

    [IO.Directory]::CreateDirectory($Runtime) | Out-Null
    foreach ($Name in @(
        'desktop.ps1',
        'desktop-script.ps1',
        'helper.ps1',
        'process-job.ps1',
        'psexec-lib.remote.ps1'
    )) {
        [IO.File]::Copy(
            (Join-Path $RuntimeRoot $Name),
            (Join-Path $Runtime $Name)
        )
    }
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'desktop-task.remote.ps1'),
        "Write-Output 'unused'`r`n",
        (New-Object Text.UTF8Encoding($false))
    )
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'psexec.remote.ps1'),
        (
            "__RDP_CLIENT_PSEXEC_LIBRARY__`r`n" +
            "`$Payload = '__RDP_CLIENT_PSEXEC_PAYLOAD__'`r`n"
        ),
        (New-Object Text.UTF8Encoding($false))
    )
    [IO.File]::WriteAllText($Entry, "entry`r`n")
    [IO.File]::WriteAllText($SshEntry, "ssh`r`n")
    [IO.File]::WriteAllLines(
        $WorkflowPath,
        @(
            "Screenshot 'workflow-before.png'",
            'Pixel 640 360',
            'Click 380 155',
            'Wait-Desktop 5',
            "Screenshot 'workflow-after.png'"
        ),
        (New-Object Text.UTF8Encoding($false))
    )
    . (Join-Path $Runtime 'desktop-script.ps1')
    $ParsedWorkflow = Read-RdpClientDesktopScript -Path $WorkflowPath
    if ($ParsedWorkflow.Steps.Count -ne 5 -or
        $ParsedWorkflow.Steps[0].OutputPath -ne $WorkflowBeforePath -or
        $ParsedWorkflow.Steps[4].OutputPath -ne $WorkflowAfterPath) {
        throw 'The desktop script parser did not preserve ordered actions or paths.'
    }
    $InvalidWorkflowPath = Join-Path $ScratchRoot 'invalid-workflow.ps1'
    [IO.File]::WriteAllText(
        $InvalidWorkflowPath,
        'Get-Process',
        (New-Object Text.UTF8Encoding($false))
    )
    Assert-DesktopScriptRejected `
        -Path $InvalidWorkflowPath `
        -Expected "unsupported action 'Get-Process'"
    [IO.File]::WriteAllLines(
        $InvalidWorkflowPath,
        @('trap { continue }', "Screenshot 'hidden-trap.png'"),
        (New-Object Text.UTF8Encoding($false))
    )
    Assert-DesktopScriptRejected `
        -Path $InvalidWorkflowPath `
        -Expected 'only accepts direct desktop action statements'

    $FakeEntry = @'
function Read-RdpClientEntryDocument {
    return [pscustomobject]@{
        Username = 'administrator'
        FullAddress = [pscustomobject]@{ Host = '192.0.2.1' }
    }
}
function Resolve-RdpClientSessionId {
    param([string]$Value)
    return [uint32]$Value
}
'@
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'entry.ps1'),
        $FakeEntry,
        (New-Object Text.UTF8Encoding($false))
    )
    $FakePeer = @'
function Resolve-RdpClientPeerSshEntryPath { return $SshEntryFile }
function Assert-RdpClientPeerSshEntryIsSeparate {}
function Invoke-RdpClientPeerSshPowerShell {
    param(
        [string]$SshEntryPath,
        [string]$RemoteSource,
        [int]$TimeoutSeconds
    )
    if (-not [string]::IsNullOrWhiteSpace(
        $env:RDP_DESKTOP_FAKE_TIMEOUT_CAPTURE
    )) {
        Add-Content `
            -LiteralPath $env:RDP_DESKTOP_FAKE_TIMEOUT_CAPTURE `
            -Value "task:$TimeoutSeconds"
    }
    return [pscustomobject]@{
        ExitCode = 0
        Output = @('RDP_CLIENT_DESKTOP_RESULT_V1:' + $env:RDP_DESKTOP_FAKE_RESULT)
    }
}
'@
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'peer-ssh.ps1'),
        $FakePeer,
        (New-Object Text.UTF8Encoding($false))
    )
    $FakeSession = @'
function Get-RdpClientPeerSessionState {
    param([string]$SshEntryPath, [int]$TimeoutSeconds)
    if (-not [string]::IsNullOrWhiteSpace(
        $env:RDP_DESKTOP_FAKE_TIMEOUT_CAPTURE
    )) {
        Add-Content `
            -LiteralPath $env:RDP_DESKTOP_FAKE_TIMEOUT_CAPTURE `
            -Value "state:$TimeoutSeconds"
    }
    if (-not [string]::IsNullOrWhiteSpace(
        $env:RDP_DESKTOP_FAKE_STATE_DELAY_MS
    )) {
        Start-Sleep -Milliseconds ([int]$env:RDP_DESKTOP_FAKE_STATE_DELAY_MS)
    }
    return [pscustomobject]@{ Sessions = @() }
}
function Resolve-RdpClientSessionSelection {
    return [pscustomobject]@{
        Id = 2
        UserName = 'administrator'
        DomainName = 'TEST'
        State = $env:RDP_DESKTOP_FAKE_STATE
        Locked = [string]::Equals(
            $env:RDP_DESKTOP_FAKE_LOCKED,
            'true',
            [StringComparison]::OrdinalIgnoreCase
        )
        Terminal = 'rdp'
    }
}
function Get-RdpClientSessionDisplayUserName { return 'TEST\administrator' }
'@
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'session.ps1'),
        $FakeSession,
        (New-Object Text.UTF8Encoding($false))
    )
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'session-connect.ps1'),
        "Set-StrictMode -Version 2.0`r`n",
        (New-Object Text.UTF8Encoding($false))
    )
    $FakeDisplay = @'
function Test-RdpClientSessionDisplayReady {
    param($Session)
    return $Session.State -eq 'Active' -and -not [bool]$Session.Locked
}
function Open-RdpClientSessionDisplayLease {
    param(
        [string]$SshEntryPath,
        [string]$EntryFile,
        [string]$EntryUserName,
        [string]$CommandName,
        [pscustomobject]$BeforeState,
        [uint32]$SessionId,
        [pscustomobject]$TimeoutBudget
    )
    if (-not [string]::IsNullOrWhiteSpace(
        $env:RDP_DESKTOP_FAKE_TIMEOUT_CAPTURE
    )) {
        $Remaining = Get-RdpClientTimeoutBudgetRemainingSeconds `
            -Budget $TimeoutBudget
        Add-Content `
            -LiteralPath $env:RDP_DESKTOP_FAKE_TIMEOUT_CAPTURE `
            -Value "display:$Remaining"
    }
    if (-not [string]::IsNullOrWhiteSpace(
        $env:RDP_DESKTOP_FAKE_DISPLAY_DELAY_MS
    )) {
        Start-Sleep -Milliseconds ([int]$env:RDP_DESKTOP_FAKE_DISPLAY_DELAY_MS)
    }
    return [pscustomobject]@{
        Session = [pscustomobject]@{
            Id = 2
            UserName = 'administrator'
            DomainName = 'TEST'
            State = 'Active'
            Locked = $false
            Terminal = 'rdp'
        }
        MstscProcess = [pscustomobject]@{ Id = 99 }
    }
}
function Close-RdpClientSessionDisplayLease {
    param($Lease)
    if ($null -ne $Lease) {
        [IO.File]::WriteAllText($env:RDP_DESKTOP_FAKE_CLOSE, 'closed')
    }
}
'@
    [IO.File]::WriteAllText(
        (Join-Path $Runtime 'session-display.ps1'),
        $FakeDisplay,
        (New-Object Text.UTF8Encoding($false))
    )

    $env:RDP_DESKTOP_FAKE_STATE = 'Active'
    $env:RDP_DESKTOP_FAKE_LOCKED = 'false'
    $env:RDP_DESKTOP_FAKE_CLOSE = $CloseCapture
    $env:RDP_DESKTOP_FAKE_RESULT = New-FakeDesktopResultPayload `
        -Action pixel `
        -Success
    $Output = Invoke-DesktopTestCommand `
        -Arguments @(
            '-Action', 'pixel',
            '-EntryFile', $Entry,
            '-SshEntryFile', $SshEntry,
            '-SessionId', '2',
            '-X', '640',
            '-Y', '360',
            '-CommandName', 'rdp-test'
        ) `
        -ExpectedExitCode 0
    if (-not $Output.Contains('Pixel:     (640, 360) #123456')) {
        throw "The desktop pixel path failed.`n$Output"
    }

    $env:RDP_DESKTOP_FAKE_RESULT = New-FakeDesktopResultPayload `
        -Action click `
        -Success
    $Output = Invoke-DesktopTestCommand `
        -Arguments @(
            '-Action', 'click',
            '-EntryFile', $Entry,
            '-SshEntryFile', $SshEntry,
            '-SessionId', '2',
            '-X', '640',
            '-Y', '360',
            '-CommandName', 'rdp-test'
        ) `
        -ExpectedExitCode 0
    if (-not $Output.Contains('Clicked:   (640, 360)')) {
        throw "The desktop click path failed.`n$Output"
    }

    $env:RDP_DESKTOP_FAKE_RESULT = New-FakeDesktopResultPayload `
        -Action screenshot `
        -Success
    $Output = Invoke-DesktopTestCommand `
        -Arguments @(
            '-Action', 'screenshot',
            '-EntryFile', $Entry,
            '-SshEntryFile', $SshEntry,
            '-SessionId', '2',
            '-OutputPath', $OutputPath,
            '-CommandName', 'rdp-test'
        ) `
        -ExpectedExitCode 0
    if (-not [IO.File]::Exists($OutputPath) -or
        -not $Output.Contains('RDP_CLIENT_DESKTOP_OUTPUT_V1:') -or
        [IO.File]::Exists($CloseCapture)) {
        throw "The existing-display screenshot path failed.`n$Output"
    }

    $env:RDP_DESKTOP_FAKE_RESULT = New-FakeDesktopResultPayload `
        -Action script `
        -Success
    $Output = Invoke-DesktopTestCommand `
        -Arguments @(
            '-Action', 'script',
            '-EntryFile', $Entry,
            '-SshEntryFile', $SshEntry,
            '-SessionId', '2',
            '-ScriptPath', $WorkflowPath,
            '-CommandName', 'rdp-test'
        ) `
        -ExpectedExitCode 0
    if (-not [IO.File]::Exists($WorkflowBeforePath) -or
        -not [IO.File]::Exists($WorkflowAfterPath) -or
        -not $Output.Contains('Step 2: pixel (640, 360) #123456') -or
        -not $Output.Contains('Step 3: click (380, 155)') -or
        -not $Output.Contains('Step 4: wait 5ms') -or
        $Output -notmatch
            'RDP_CLIENT_DESKTOP_OUTPUT_V1:(?<Payload>[A-Za-z0-9+/=]+)') {
        throw "The ordered desktop script path failed.`n$Output"
    }
    $PublicWorkflowJson = [Text.Encoding]::UTF8.GetString(
        [Convert]::FromBase64String($Matches.Payload)
    )
    $PublicWorkflow = $PublicWorkflowJson | ConvertFrom-Json
    if ($PublicWorkflow.Action -ne 'script' -or
        @($PublicWorkflow.Steps).Count -ne 5 -or
        $PublicWorkflowJson.Contains('ImageBase64')) {
        throw 'The public workflow result is invalid or leaked screenshot payloads.'
    }
    $env:RDP_DESKTOP_FAKE_RESULT = New-FakeDesktopResultPayload `
        -Action screenshot `
        -Success

    [IO.File]::WriteAllBytes($ExistingOutputPath, [byte[]]@(1, 2, 3, 4))
    $ExistingHash = (
        Get-FileHash -LiteralPath $ExistingOutputPath -Algorithm SHA256
    ).Hash
    $Output = Invoke-DesktopTestCommand `
        -Arguments @(
            '-Action', 'screenshot',
            '-EntryFile', $Entry,
            '-SshEntryFile', $SshEntry,
            '-SessionId', '2',
            '-OutputPath', $ExistingOutputPath,
            '-CommandName', 'rdp-test'
        ) `
        -ExpectedExitCode 1
    if (-not $Output.Contains('Screenshot output already exists') -or
        (Get-FileHash -LiteralPath $ExistingOutputPath -Algorithm SHA256).Hash -ne
            $ExistingHash) {
        throw "An existing screenshot output must not be overwritten.`n$Output"
    }

    [IO.File]::Delete($OutputPath)
    $env:RDP_DESKTOP_FAKE_LOCKED = 'true'
    $Output = Invoke-DesktopTestCommand `
        -Arguments @(
            '-Action', 'screenshot',
            '-EntryFile', $Entry,
            '-SshEntryFile', $SshEntry,
            '-SessionId', '2',
            '-OutputPath', $OutputPath,
            '-Display',
            '-CommandName', 'rdp-test'
        ) `
        -ExpectedExitCode 1
    if (-not $Output.Contains('active but locked') -or
        [IO.File]::Exists($CloseCapture)) {
        throw "A locked active session must not be rerouted by --display.`n$Output"
    }

    $env:RDP_DESKTOP_FAKE_LOCKED = 'false'
    $env:RDP_DESKTOP_FAKE_STATE = 'Disconnected'
    $Output = Invoke-DesktopTestCommand `
        -Arguments @(
            '-Action', 'screenshot',
            '-EntryFile', $Entry,
            '-SshEntryFile', $SshEntry,
            '-SessionId', '2',
            '-OutputPath', $OutputPath,
            '-Display',
            '-CommandName', 'rdp-test'
        ) `
        -ExpectedExitCode 0
    if (-not [IO.File]::Exists($CloseCapture) -or
        -not $Output.Contains('temporary-rdp')) {
        throw "The temporary-display screenshot path failed.`n$Output"
    }

    [IO.File]::Delete($OutputPath)
    [IO.File]::Delete($CloseCapture)
    $env:RDP_DESKTOP_FAKE_TIMEOUT_CAPTURE = $TimeoutCapture
    $env:RDP_DESKTOP_FAKE_STATE_DELAY_MS = '1100'
    $env:RDP_DESKTOP_FAKE_DISPLAY_DELAY_MS = '1100'
    $env:RDP_DESKTOP_FAKE_RESULT = New-FakeDesktopResultPayload `
        -Action pixel `
        -Success
    $Output = Invoke-DesktopTestCommand `
        -Arguments @(
            '-Action', 'pixel',
            '-EntryFile', $Entry,
            '-SshEntryFile', $SshEntry,
            '-SessionId', '2',
            '-X', '640',
            '-Y', '360',
            '-Display',
            '-Timeout', '10s',
            '-CommandName', 'rdp-test'
        ) `
        -ExpectedExitCode 0
    $ObservedTimeouts = @{}
    foreach ($Line in [IO.File]::ReadAllLines($TimeoutCapture)) {
        $Parts = $Line.Split(':')
        $ObservedTimeouts[$Parts[0]] = [int]$Parts[1]
    }
    if ($ObservedTimeouts.state -ne 10 -or
        $ObservedTimeouts.display -ge $ObservedTimeouts.state -or
        $ObservedTimeouts.task -ge $ObservedTimeouts.display) {
        throw (
            'Desktop phases reset the timeout instead of consuming one ' +
            "budget: $($ObservedTimeouts | ConvertTo-Json -Compress)`n$Output"
        )
    }

    [IO.File]::Delete($TimeoutCapture)
    [IO.File]::Delete($CloseCapture)
    $Stopwatch = [Diagnostics.Stopwatch]::StartNew()
    try {
        $Output = Invoke-DesktopTestCommand `
            -Arguments @(
                '-Action', 'pixel',
                '-EntryFile', $Entry,
                '-SshEntryFile', $SshEntry,
                '-SessionId', '2',
                '-X', '640',
                '-Y', '360',
                '-Display',
                '-Timeout', '2s',
                '-CommandName', 'rdp-test'
            ) `
            -ExpectedExitCode 1
    } finally {
        $Stopwatch.Stop()
    }
    $ExpiredStages = @([IO.File]::ReadAllLines($TimeoutCapture))
    if (-not $Output.Contains('Desktop command timed out after 2 seconds') -or
        $Stopwatch.Elapsed.TotalSeconds -gt 4 -or
        @($ExpiredStages | Where-Object { $_ -like 'task:*' }).Count -ne 0 -or
        -not [IO.File]::Exists($CloseCapture)) {
        throw (
            'An exhausted desktop deadline must stop before the task and ' +
            "close its display lease. elapsed=$($Stopwatch.Elapsed)`n" +
            ($ExpiredStages -join ',') + "`n$Output"
        )
    }

    [IO.Directory]::CreateDirectory($BootstrapRuntime) | Out-Null
    foreach ($Name in @('process-job.ps1', 'session-display.ps1')) {
        [IO.File]::Copy(
            (Join-Path $RuntimeRoot $Name),
            (Join-Path $BootstrapRuntime $Name)
        )
    }
    [IO.File]::WriteAllText(
        (Join-Path $BootstrapRuntime 'connect.ps1'),
        (
            '[IO.File]::WriteAllText(' +
            '$env:RDP_DESKTOP_BOOTSTRAP_PID,[string]$PID);' +
            'Start-Sleep -Seconds 30'
        ),
        (New-Object Text.UTF8Encoding($false))
    )
    . (Join-Path $BootstrapRuntime 'session-display.ps1')
    $env:RDP_DESKTOP_BOOTSTRAP_PID = $BootstrapPidPath
    $BootstrapStopwatch = [Diagnostics.Stopwatch]::StartNew()
    try {
        try {
            $null = Start-RdpClientDisplayBootstrap `
                -EntryFile 'unused.rdp.cmd' `
                -SshEntryFile 'unused.ssh.cmd' `
                -CommandName 'rdp-test' `
                -TimeoutSeconds 2
            throw 'A blocked temporary RDP bootstrap unexpectedly completed.'
        } catch {
            if (-not $_.Exception.Message.Contains(
                'timed out after 2 seconds while starting temporary RDP'
            )) {
                throw
            }
        }
    } finally {
        $BootstrapStopwatch.Stop()
    }
    if ($BootstrapStopwatch.Elapsed.TotalSeconds -gt 5 -or
        -not [IO.File]::Exists($BootstrapPidPath)) {
        throw (
            'The temporary RDP bootstrap did not stop within its deadline. ' +
            "elapsed=$($BootstrapStopwatch.Elapsed)"
        )
    }
    $BootstrapPid = [int][IO.File]::ReadAllText($BootstrapPidPath)
    try {
        $BootstrapProcess = [Diagnostics.Process]::GetProcessById($BootstrapPid)
        try {
            if (-not $BootstrapProcess.HasExited) {
                throw "The timed-out RDP bootstrap left PID $BootstrapPid running."
            }
        } finally {
            $BootstrapProcess.Dispose()
        }
    } catch [ArgumentException] {
    }

    Write-Host 'rdp client desktop tests: PASS' -ForegroundColor Green
} finally {
    Remove-Item Env:RDP_DESKTOP_FAKE_RESULT -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_DESKTOP_FAKE_STATE -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_DESKTOP_FAKE_LOCKED -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_DESKTOP_FAKE_CLOSE -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_DESKTOP_FAKE_TIMEOUT_CAPTURE -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_DESKTOP_FAKE_STATE_DELAY_MS -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_DESKTOP_FAKE_DISPLAY_DELAY_MS -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_DESKTOP_BOOTSTRAP_PID -ErrorAction SilentlyContinue
    if ([IO.Directory]::Exists($ScratchRoot)) {
        [IO.Directory]::Delete($ScratchRoot, $true)
    }
}
