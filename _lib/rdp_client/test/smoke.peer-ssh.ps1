[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$ScratchRoot = Join-Path (Join-Path $RepoRoot 'data\rdp-client') (
    '.peer-ssh-test-' + [Guid]::NewGuid().ToString('N')
)
$BlockingCopyEntry = Join-Path $ScratchRoot 'blocking-copy.ssh.cmd'
$BlockingCommandEntry = Join-Path $ScratchRoot 'blocking-command.ssh.cmd'
$PidPath = Join-Path $ScratchRoot 'blocked-child.pid'

. (Join-Path $PSScriptRoot '..\peer-ssh.ps1')

try {
    [IO.Directory]::CreateDirectory($ScratchRoot) | Out-Null
    [IO.File]::WriteAllText(
        $BlockingCopyEntry,
        (
            "@echo off`r`n" +
            "PowerShell.exe -NoLogo -NoProfile -Command `"" +
            "[IO.File]::WriteAllText(" +
            "`$env:RDP_PEER_SSH_TEST_PID_PATH,[string]`$PID);" +
            "Start-Sleep -Seconds 30`"`r`n"
        ),
        (New-Object Text.UTF8Encoding($false))
    )
    [IO.File]::WriteAllText(
        $BlockingCommandEntry,
        (
            "@echo off`r`n" +
            "if /i `"%~1`"==`"copy`" exit /b 0`r`n" +
            "PowerShell.exe -NoLogo -NoProfile -Command `"" +
            "[IO.File]::WriteAllText(" +
            "`$env:RDP_PEER_SSH_TEST_PID_PATH,[string]`$PID);" +
            "Start-Sleep -Seconds 30`"`r`n"
        ),
        (New-Object Text.UTF8Encoding($false))
    )

    $LargeRemoteSource = 'x' * 131072
    $env:RDP_PEER_SSH_TEST_PID_PATH = $PidPath
    foreach ($Case in @(
        [pscustomobject]@{ Name = 'copy'; Entry = $BlockingCopyEntry },
        [pscustomobject]@{ Name = 'command'; Entry = $BlockingCommandEntry }
    )) {
        if ([IO.File]::Exists($PidPath)) {
            [IO.File]::Delete($PidPath)
        }
        $Stopwatch = [Diagnostics.Stopwatch]::StartNew()
        try {
            $null = Invoke-RdpClientPeerSshPowerShell `
                -SshEntryPath $Case.Entry `
                -RemoteSource $LargeRemoteSource `
                -TimeoutSeconds 2
            throw "A blocked SSH $($Case.Name) unexpectedly completed."
        } catch {
            if (-not $_.Exception.Message.Contains('timed out after 2 seconds')) {
                throw
            }
        } finally {
            $Stopwatch.Stop()
        }

        if ($Stopwatch.Elapsed.TotalSeconds -gt 10) {
            throw (
                "SSH $($Case.Name) timeout cleanup took too long: " +
                $Stopwatch.Elapsed
            )
        }
        if (-not [IO.File]::Exists($PidPath)) {
            throw "The blocked SSH $($Case.Name) child did not report its PID."
        }
        $BlockedPid = [int][IO.File]::ReadAllText($PidPath)
        try {
            $BlockedProcess = [Diagnostics.Process]::GetProcessById($BlockedPid)
            try {
                if (-not $BlockedProcess.HasExited) {
                    throw (
                        "SSH $($Case.Name) timeout left child PID " +
                        "$BlockedPid running."
                    )
                }
            } finally {
                $BlockedProcess.Dispose()
            }
        } catch [ArgumentException] {
        }
    }

    Write-Host 'rdp client peer SSH timeout tests: PASS' -ForegroundColor Green
} finally {
    Remove-Item Env:RDP_PEER_SSH_TEST_PID_PATH -ErrorAction SilentlyContinue
    if ([IO.Directory]::Exists($ScratchRoot)) {
        [IO.Directory]::Delete($ScratchRoot, $true)
    }
}
