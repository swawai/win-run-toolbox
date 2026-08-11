[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjCommandEventTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "Assertion failed: $Message"
    }
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$EventScript = Join-Path $RepoRoot '_lib\proj\_toolchain\_lib\event.ps1'
$Command = @"
. '$($EventScript.Replace("'", "''"))'
Write-ProjDevProgressEvent -Id 'download:fixture.zip' -State 'running' -Unit 'bytes' -Message 'Downloading fixture.zip'
Write-ProjDevProgressEvent -Id 'download:fixture.zip' -State 'completed' -Unit 'bytes' -Message 'Downloaded fixture.zip' -Current 42 -Total 42
"@
$Encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Command))
$StartInfo = [Diagnostics.ProcessStartInfo]::new()
$StartInfo.FileName = Join-Path $env:SystemRoot (
    'System32\WindowsPowerShell\v1.0\powershell.exe'
)
$StartInfo.Arguments = "-NoLogo -NoProfile -NonInteractive -EncodedCommand $Encoded"
$StartInfo.UseShellExecute = $false
$StartInfo.CreateNoWindow = $true
$StartInfo.RedirectStandardOutput = $true
$StartInfo.RedirectStandardError = $true
$StartInfo.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
$StartInfo.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
[void]$StartInfo.EnvironmentVariables
$StartInfo.EnvironmentVariables[
    'SWAWKIT_PROJ_CORE_COMMAND_EVENT_PROTOCOL'
] = 'swawkit.command-event-frame/v1'
$Process = [Diagnostics.Process]::new()
$Process.StartInfo = $StartInfo
try {
    Assert-ProjCommandEventTest `
        -Condition $Process.Start() `
        -Message 'the PowerShell event producer did not start'
    $StandardOutput = $Process.StandardOutput.ReadToEnd()
    $StandardError = $Process.StandardError.ReadToEnd()
    $Process.WaitForExit()
    Assert-ProjCommandEventTest `
        -Condition ($Process.ExitCode -eq 0) `
        -Message "the event producer failed: $StandardError"
} finally {
    $Process.Dispose()
}

$Prefix = ([char]0x1e) + 'swawkit-event-v1 '
$Frames = @($StandardError -split "`r?`n" | Where-Object {
    $_.StartsWith($Prefix, [StringComparison]::Ordinal)
})
Assert-ProjCommandEventTest `
    -Condition ([string]::IsNullOrEmpty($StandardOutput) -and $Frames.Count -eq 2) `
    -Message 'the event producer did not emit exactly two stderr frames'
$Running = $Frames[0].Substring($Prefix.Length) | ConvertFrom-Json
$Completed = $Frames[1].Substring($Prefix.Length) | ConvertFrom-Json
Assert-ProjCommandEventTest `
    -Condition (
        $Running.kind -ceq 'progress' -and
        $Running.state -ceq 'running' -and
        $null -eq $Running.current -and
        $null -eq $Running.total
    ) `
    -Message 'an indeterminate progress frame was not serialized correctly'
Assert-ProjCommandEventTest `
    -Condition (
        $Completed.state -ceq 'completed' -and
        [long]$Completed.current -eq 42 -and
        [long]$Completed.total -eq 42
    ) `
    -Message 'a completed progress frame was not serialized correctly'

Write-Host '[PASS] Proj command event producer' -ForegroundColor Green
$global:LASTEXITCODE = 0
