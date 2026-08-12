[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$OutputEncoding = New-Object Text.UTF8Encoding($false)

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$TemplateEntry = Join-Path $RepoRoot 'Favorites\template.rdp1.cmd'
$ScratchRoot = Join-Path (Join-Path $RepoRoot 'data\rdp-client') (
    '.psexec-test-' + [Guid]::NewGuid().ToString('N')
)
$Entry = Join-Path $ScratchRoot 'account.rdp.cmd'
$FakeSshEntry = Join-Path $ScratchRoot 'windows-admin.ssh.cmd'
$CapturePath = Join-Path $ScratchRoot 'ssh-arguments.txt'
$SourcePath = Join-Path $ScratchRoot 'remote-source.ps1'
$CaptureScript = Join-Path $ScratchRoot 'capture-stdin.ps1'
$Runtime = Join-Path $ScratchRoot '_lib\rdp_client'

function Invoke-PsExecTestEntry {
    param(
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][int]$ExpectedExitCode
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
        throw "Unexpected exit code $ExitCode.`n$Output"
    }
    return $Output
}

function Get-CapturedRemoteSource {
    $Captured = [IO.File]::ReadAllText($CapturePath).Trim()
    if (-not $Captured.StartsWith('-- powershell.exe ') -or
        -not $Captured.Contains('-EncodedCommand ')) {
        throw "Unexpected SSH entry arguments: $Captured"
    }
    $BootstrapBase64 = ($Captured -split ' ')[-1]
    if ($BootstrapBase64.EndsWith('=') -or
        $BootstrapBase64.Length -gt 2048) {
        throw 'The peer loader is unsafe for the SSH .cmd command chain.'
    }
    $Bootstrap = [Text.Encoding]::Unicode.GetString(
        [Convert]::FromBase64String($BootstrapBase64)
    )
    foreach ($ExpectedLoaderSource in @(
        'Start-Process',
        'WaitForExit(',
        'taskkill.exe /PID $x.Id /T /F',
        'Remove-Item -LiteralPath $p -Force'
    )) {
        if (-not $Bootstrap.Contains($ExpectedLoaderSource)) {
            throw "The bounded peer loader is missing: $ExpectedLoaderSource"
        }
    }
    return [IO.File]::ReadAllText(
        $SourcePath,
        [Text.Encoding]::UTF8
    )
}

function Get-CapturedRequest {
    $RemoteSource = Get-CapturedRemoteSource
    $Tokens = $null
    $ParseErrors = $null
    [void][Management.Automation.Language.Parser]::ParseInput(
        $RemoteSource,
        [ref]$Tokens,
        [ref]$ParseErrors
    )
    if ($ParseErrors.Count -gt 0) {
        throw "Transported PsExec source does not parse: $($ParseErrors[0].Message)"
    }
    if ($RemoteSource -notmatch
        "(?m)^\s*\`$PayloadBase64 = '(?<Payload>[^']+)'\r?\n") {
        throw (
            'The transported PsExec source has no request payload. ' +
            "Captured length=$($RemoteSource.Length)."
        )
    }
    $Json = [Text.Encoding]::UTF8.GetString(
        [Convert]::FromBase64String($Matches.Payload)
    )
    return $Json | ConvertFrom-Json
}

function Assert-Request {
    param(
        [Parameter(Mandatory = $true)][object]$Request,
        [Parameter(Mandatory = $true)][string]$Action,
        [Parameter(Mandatory = $true)][bool]$DryRun,
        [int]$SessionId = 0
    )

    if ($Request.Action -ne $Action -or [bool]$Request.DryRun -ne $DryRun) {
        throw "Unexpected PsExec request: $($Request | ConvertTo-Json -Compress)"
    }
    if (@($Request.ExpectedAddresses) -notcontains '192.168.1.115') {
        throw 'The PsExec request did not bind SSH to the RDP peer address.'
    }
    if ([int]$Request.SessionId -ne $SessionId) {
        throw "Unexpected PsExec session ID: $($Request.SessionId)"
    }
    if ([string]$Request.HelperSha256 -notmatch '^[A-F0-9]{64}$') {
        throw 'The PsExec request has no valid session-helper hash.'
    }
    if ([string]$Request.DesktopWorkerSha256 -notmatch '^[A-F0-9]{64}$') {
        throw 'The PsExec request has no valid desktop-worker hash.'
    }
    if ($Action -eq 'add' -and -not $DryRun -and
        [string]$Request.HelperUploadName -notmatch
        '^\.swaw-kit-psexec-helper-[A-Fa-f0-9]{32}\.ps1$') {
        throw 'PsExec add did not reference the uploaded session helper.'
    }
    if ($Action -eq 'add' -and -not $DryRun -and
        [string]$Request.DesktopWorkerUploadName -notmatch
        '^\.swaw-kit-psexec-desktop-[A-Fa-f0-9]{32}\.ps1$') {
        throw 'PsExec add did not reference the uploaded desktop worker.'
    }
}

try {
    [IO.Directory]::CreateDirectory($Runtime) | Out-Null
    [IO.File]::Copy($TemplateEntry, $Entry)
    foreach ($RuntimeFile in @(
        'client.cmd',
        'entry.ps1',
        'peer-ssh.ps1',
        'process-job.ps1',
        'psexec.ps1',
        'psexec.remote.ps1',
        'psexec-lib.remote.ps1',
        'helper.ps1',
        'desktop-task.remote.ps1'
    )) {
        [IO.File]::Copy(
            (Join-Path (Join-Path $PSScriptRoot '..') $RuntimeFile),
            (Join-Path $Runtime $RuntimeFile)
        )
    }

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

$FakeSshSource = @'
@echo off
PowerShell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%~dp0capture-stdin.ps1" %*
exit /b %ERRORLEVEL%
'@
    $FakeSshSource = [regex]::Replace($FakeSshSource, "`r?`n", "`r`n")
    [IO.File]::WriteAllText(
        $FakeSshEntry,
        $FakeSshSource,
        (New-Object Text.UTF8Encoding($false))
    )
$CaptureSource = @'
if ($args[0] -eq 'copy') {
    $remoteName = ([string]$args[2]).TrimStart(':')
    if ($remoteName -match '^\.swaw-kit-rdp-peer-[A-Fa-f0-9]{32}\.ps1$') {
        [IO.File]::Copy(
            [IO.Path]::GetFullPath([string]$args[1]),
            $env:RDP_PSEXEC_TEST_SOURCE,
            $true
        )
    }
    exit 0
}
[IO.File]::WriteAllText(
    $env:RDP_PSEXEC_TEST_CAPTURE,
    ($args -join ' '),
    [Text.Encoding]::ASCII
)
if ([string]::IsNullOrWhiteSpace($env:RDP_PSEXEC_FAKE_EXIT)) {
    exit 0
}
exit [int]$env:RDP_PSEXEC_FAKE_EXIT
'@
    [IO.File]::WriteAllText(
        $CaptureScript,
        $CaptureSource,
        (New-Object Text.UTF8Encoding($false))
    )
    $env:RDP_PSEXEC_TEST_CAPTURE = $CapturePath
    $env:RDP_PSEXEC_TEST_SOURCE = $SourcePath

    Invoke-PsExecTestEntry `
        -Arguments @('.peer', 'psexec', 'status') `
        -ExpectedExitCode 0 |
        Out-Null
    Assert-Request -Request (Get-CapturedRequest) -Action status -DryRun $false

    Invoke-PsExecTestEntry `
        -Arguments @('.peer', 'psexec', 'add') `
        -ExpectedExitCode 0 |
        Out-Null
    Assert-Request -Request (Get-CapturedRequest) -Action add -DryRun $false

    $env:RDP_PSEXEC_FAKE_EXIT = '19'
    $FailedAddOutput = Invoke-PsExecTestEntry `
        -Arguments @('.peer', 'psexec', 'add') `
        -ExpectedExitCode 19
    Remove-Item Env:RDP_PSEXEC_FAKE_EXIT
    $CleanupSource = Get-CapturedRemoteSource
    foreach ($ExpectedCleanupSource in @(
        'Invalid PsExec upload cleanup name.',
        'Remove-Item -LiteralPath $p -Force',
        '^\.swaw-kit-psexec-(?:helper|desktop)-'
    )) {
        if (-not $CleanupSource.Contains($ExpectedCleanupSource)) {
            throw "Failed PsExec add did not run safe upload cleanup."
        }
    }
    if (-not $FailedAddOutput.Contains(
        '[WARN] Could not clean temporary peer PsExec uploads'
    )) {
        throw 'Failed upload cleanup should preserve the command failure and warn.'
    }

    Invoke-PsExecTestEntry `
        -Arguments @('.peer', 'psexec', 'add', '--dry-run') `
        -ExpectedExitCode 0 |
        Out-Null
    Assert-Request -Request (Get-CapturedRequest) -Action add -DryRun $true

    Invoke-PsExecTestEntry `
        -Arguments @('.peer', 'psexec', 'remove', '--dry-run') `
        -ExpectedExitCode 0 |
        Out-Null
    Assert-Request -Request (Get-CapturedRequest) -Action remove -DryRun $true

    Invoke-PsExecTestEntry `
        -Arguments @('.peer', 'psexec', 'remove') `
        -ExpectedExitCode 0 |
        Out-Null
    Assert-Request -Request (Get-CapturedRequest) -Action remove -DryRun $false

    $SessionArguments = @(
        'C:\Program Files\Test App\app.exe',
        '--flag',
        'hello world',
        'bang!'
    )
    $env:RDP_PSEXEC_FAKE_EXIT = '19'
    Invoke-PsExecTestEntry `
        -Arguments (@('.peer', 'psexec', '2') + $SessionArguments) `
        -ExpectedExitCode 19 |
        Out-Null
    $LaunchRequest = Get-CapturedRequest
    Assert-Request `
        -Request $LaunchRequest `
        -Action launch `
        -DryRun $false `
        -SessionId 2
    if ((@($LaunchRequest.Arguments) -join "`n") -ne
        ($SessionArguments -join "`n")) {
        throw (
            'PsExec session arguments changed in transit: ' +
            (@($LaunchRequest.Arguments) -join ' | ')
        )
    }
    Remove-Item Env:RDP_PSEXEC_FAKE_EXIT

    $NativeArguments = @(
        '-nobanner',
        '-i',
        '2',
        '-d',
        'C:\Program Files\Test App\app.exe',
        '--flag',
        'hello world',
        'bang!'
    )
    $env:RDP_PSEXEC_FAKE_EXIT = '23'
    Invoke-PsExecTestEntry `
        -Arguments (@('.peer', 'psexec', '--') + $NativeArguments) `
        -ExpectedExitCode 23 |
        Out-Null
    $RunRequest = Get-CapturedRequest
    Assert-Request -Request $RunRequest -Action run -DryRun $false
    if ((@($RunRequest.Arguments) -join "`n") -ne ($NativeArguments -join "`n")) {
        throw (
            'PsExec native arguments changed in transit: ' +
            (@($RunRequest.Arguments) -join ' | ')
        )
    }
    Remove-Item Env:RDP_PSEXEC_FAKE_EXIT

    $RemoteTemplate = [IO.File]::ReadAllText(
        (Join-Path $PSScriptRoot '..\psexec.remote.ps1'),
        [Text.Encoding]::UTF8
    )
    $RemoteTemplate += [IO.File]::ReadAllText(
        (Join-Path $PSScriptRoot '..\psexec-lib.remote.ps1'),
        [Text.Encoding]::UTF8
    )
    foreach ($ExpectedSource in @(
        'PROCESSOR_ARCHITEW6432',
        'https://live.sysinternals.com/PsExec64.exe',
        'https://live.sysinternals.com/ARM64/PsExec64a.exe',
        'https://live.sysinternals.com/PsExec.exe',
        'LocalApplicationData',
        "Join-Path `$LocalAppData 'swaw-kit\rdp-client'",
        "Join-Path `$ManagedDirectory 'psexec.exe'",
        'Get-AuthenticodeSignature',
        'O=Microsoft Corporation',
        'Invoke-WebRequest',
        'Move-Item',
        'Invoke-RdpClientCapturedProcess',
        'RedirectStandardError = $true',
        'started on .+ with process ID',
        "Join-Path `$ManagedDirectory 'helper.ps1'",
        "Join-Path `$ManagedDirectory 'desktop-task.ps1'",
        'HelperUploadName',
        'HelperSha256',
        'DesktopWorkerUploadName',
        'DesktopWorkerSha256',
        'Write-RdpClientPsExecField',
        'Write-RdpClientPsExecFile',
        "'PsExec SOURCE'",
        "'Helper VERIFY'",
        "Write-Output '  ---'",
        "`$Request.Action -eq 'launch'",
        "`$Request.Action -eq 'desktop'",
        'DesktopRequestBase64',
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
        'interactive desktop worker result',
        "'-i'",
        "'-WindowStyle'",
        "'-s'"
    )) {
        if (-not $RemoteTemplate.Contains($ExpectedSource)) {
            throw "The remote PsExec implementation is missing '$ExpectedSource'."
        }
    }
    if (-not $RemoteTemplate.Contains(
        "`$PsExecArguments = @('-accepteula') + `$PsExecArguments"
    )) {
        throw 'The PsExec wrapper must inject -accepteula for unattended runs.'
    }
    if ($RemoteTemplate.Contains('@PsExecArguments 2>&1')) {
        throw 'PsExec stderr must remain a raw native stream.'
    }
    if ($RemoteTemplate.Contains('psexec-session-launch.ps1')) {
        throw 'The unpublished PsExec helper filename must not remain supported.'
    }

    . (Join-Path $PSScriptRoot '..\psexec-lib.remote.ps1')
    $IdentityPath = Join-Path $ScratchRoot 'owned-process-identity.json'
    $OwnedProcess = Start-Process `
        -FilePath 'powershell.exe' `
        -ArgumentList @(
            '-NoLogo',
            '-NoProfile',
            '-NonInteractive',
            '-Command',
            'Start-Sleep -Seconds 60'
        ) `
        -WindowStyle Hidden `
        -PassThru
    try {
        $OwnedStartTime = $OwnedProcess.StartTime.ToUniversalTime().Ticks
        [IO.File]::WriteAllText(
            $IdentityPath,
            (ConvertTo-Json -Compress -InputObject ([ordered]@{
                Version           = 1
                ProcessId         = $OwnedProcess.Id
                StartTimeUtcTicks = $OwnedStartTime + 1
            })),
            (New-Object Text.UTF8Encoding($false))
        )
        Stop-RdpClientDesktopWorkerProcess -IdentityPath $IdentityPath
        $OwnedProcess.Refresh()
        if ($OwnedProcess.HasExited) {
            throw 'Worker cleanup must not kill a PID with a different start time.'
        }
        [IO.File]::WriteAllText(
            $IdentityPath,
            (ConvertTo-Json -Compress -InputObject ([ordered]@{
                Version           = 1
                ProcessId         = $OwnedProcess.Id
                StartTimeUtcTicks = $OwnedStartTime
            })),
            (New-Object Text.UTF8Encoding($false))
        )
        Stop-RdpClientDesktopWorkerProcess -IdentityPath $IdentityPath
        if (-not $OwnedProcess.WaitForExit(5000)) {
            throw 'Worker cleanup did not stop its exact owned process.'
        }
    } finally {
        if (-not $OwnedProcess.HasExited) {
            Stop-Process -InputObject $OwnedProcess -Force
        }
        $OwnedProcess.Dispose()
    }

    $HelperTemplate = [IO.File]::ReadAllText(
        (Join-Path $PSScriptRoot '..\helper.ps1'),
        [Text.Encoding]::UTF8
    )
    if ([IO.File]::Exists(
        (Join-Path $PSScriptRoot '..\psexec-session-launch.ps1')
    )) {
        throw 'The PsExec helper must have one canonical source filename.'
    }
    foreach ($ExpectedHelperSource in @(
        'WTSQueryUserToken',
        'CreateEnvironmentBlock',
        'CreateProcessAsUserW',
        'AdjustTokenPrivileges',
        'SeTcbPrivilege',
        'winsta0\default',
        'RdpSessionLaunchResult'
    )) {
        if (-not $HelperTemplate.Contains($ExpectedHelperSource)) {
            throw "The session helper is missing '$ExpectedHelperSource'."
        }
    }
    if ($HelperTemplate -notmatch
        "(?s)\`$NativeSource = @'\r?\n(?<CSharp>.+?)\r?\n'@") {
        throw 'The PsExec session helper C# source was not found.'
    }
    Add-Type -TypeDefinition $Matches.CSharp -Language CSharp

    foreach ($InvalidArguments in @(
        [string[]]@('.peer', 'psexec'),
        [string[]]@('.peer', 'psexec', 'status', 'extra'),
        [string[]]@('.peer', 'psexec', 'add', '--unexpected'),
        [string[]]@('.peer', 'psexec', 'add', '--dry-run', 'extra'),
        [string[]]@('.peer', 'psexec', 'remove', '--unexpected'),
        [string[]]@('.peer', 'psexec', '--'),
        [string[]]@('.peer', 'psexec', '2'),
        [string[]]@('.peer', 'psexec', '2x', 'notepad.exe')
    )) {
        $InvalidOutput = Invoke-PsExecTestEntry `
            -Arguments $InvalidArguments `
            -ExpectedExitCode 1
        if (-not $InvalidOutput.Contains('.peer psexec -- <native-arguments>')) {
            throw "Invalid PsExec arguments should show Peer usage.`n$InvalidOutput"
        }
    }

    Write-Host 'rdp client PsExec tests: PASS' -ForegroundColor Green
} finally {
    Remove-Item Env:RDP_PSEXEC_TEST_CAPTURE -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_PSEXEC_TEST_SOURCE -ErrorAction SilentlyContinue
    Remove-Item Env:RDP_PSEXEC_FAKE_EXIT -ErrorAction SilentlyContinue
    if ([IO.Directory]::Exists($ScratchRoot)) {
        [IO.Directory]::Delete($ScratchRoot, $true)
    }
}
