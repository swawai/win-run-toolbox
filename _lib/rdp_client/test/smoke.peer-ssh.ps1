[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$ScratchRoot = Join-Path (Join-Path $RepoRoot 'data\rdp-client') (
    '.peer-ssh-test-' + [Guid]::NewGuid().ToString('N')
)
$BlockingEntry = Join-Path $ScratchRoot 'blocking.ssh.cmd'

. (Join-Path $PSScriptRoot '..\peer-ssh.ps1')

try {
    [IO.Directory]::CreateDirectory($ScratchRoot) | Out-Null
    [IO.File]::WriteAllText(
        $BlockingEntry,
        "@echo off`r`nPowerShell.exe -NoLogo -NoProfile -Command `"Start-Sleep -Seconds 30`"`r`n",
        (New-Object Text.UTF8Encoding($false))
    )

    $LargeRemoteSource = 'x' * 131072
    $Stopwatch = [Diagnostics.Stopwatch]::StartNew()
    try {
        $null = Invoke-RdpClientPeerSshPowerShell `
            -SshEntryPath $BlockingEntry `
            -RemoteSource $LargeRemoteSource `
            -TimeoutSeconds 2
        throw 'A blocked SSH entry unexpectedly completed.'
    } catch {
        if (-not $_.Exception.Message.Contains('timed out after 2 seconds')) {
            throw
        }
    } finally {
        $Stopwatch.Stop()
    }

    if ($Stopwatch.Elapsed.TotalSeconds -gt 10) {
        throw "SSH timeout cleanup took too long: $($Stopwatch.Elapsed)"
    }

    Write-Host 'rdp client peer SSH timeout tests: PASS' -ForegroundColor Green
} finally {
    if ([IO.Directory]::Exists($ScratchRoot)) {
        [IO.Directory]::Delete($ScratchRoot, $true)
    }
}
