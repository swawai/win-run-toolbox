param(
    [Parameter(Mandatory = $true)][string]$EntryPath,
    [Parameter(Mandatory = $true)][string]$ArgumentPayload
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

. (Join-Path $PSScriptRoot 'argument-payload.ps1')

try {
    [string[]]$EntryArguments = @(
        ConvertFrom-ProjArgumentPayload -Payload $ArgumentPayload
    )
    $global:LASTEXITCODE = 0
    & $EntryPath @EntryArguments
    $EntrySucceeded = $?
    $EntryExitCode = [int]$global:LASTEXITCODE
    if (-not $EntrySucceeded -and $EntryExitCode -eq 0) {
        $EntryExitCode = 1
    }
    exit $EntryExitCode
} catch {
    [Console]::Error.WriteLine("[ERROR] $($_.Exception.Message)")
    exit 1
}
