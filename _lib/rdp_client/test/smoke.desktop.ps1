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
$CloseCapture = Join-Path $ScratchRoot 'display-closed.txt'

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
        if ($Action -eq 'screenshot') {
            $Result.ImageBase64 = [Convert]::ToBase64String([byte[]]@(
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
                0x01, 0x02, 0x03, 0x04
            ))
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
    $Json = ConvertTo-Json -InputObject $Result -Compress
    return [Convert]::ToBase64String(
        [Text.Encoding]::UTF8.GetBytes($Json)
    )
}

try {
    foreach ($Name in @(
        'desktop.ps1',
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
        'COORDINATE_OUT_OF_RANGE'
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
    foreach ($Name in @('desktop.ps1', 'helper.ps1', 'psexec-lib.remote.ps1')) {
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

    Write-Host 'rdp client desktop tests: PASS' -ForegroundColor Green
} finally {
    Remove-Item Env:RDP_DESKTOP_FAKE_RESULT -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_DESKTOP_FAKE_STATE -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_DESKTOP_FAKE_LOCKED -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_DESKTOP_FAKE_CLOSE -ErrorAction SilentlyContinue
    if ([IO.Directory]::Exists($ScratchRoot)) {
        [IO.Directory]::Delete($ScratchRoot, $true)
    }
}
