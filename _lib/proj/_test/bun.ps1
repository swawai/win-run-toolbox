[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ToolchainPath
)

$ErrorActionPreference = 'Stop'

& (Join-Path $PSScriptRoot 'bun.release.ps1')
& (Join-Path $PSScriptRoot 'bun.latest.ps1')
& (Join-Path $PSScriptRoot 'bun.install.ps1') `
    -ToolchainPath $ToolchainPath
& (Join-Path $PSScriptRoot 'bun.status.ps1') `
    -ToolchainPath $ToolchainPath
& (Join-Path $PSScriptRoot 'bun.command.ps1') `
    -ToolchainPath $ToolchainPath

Write-Host '[PASS] Proj Bun test suite' -ForegroundColor Green
$global:LASTEXITCODE = 0
