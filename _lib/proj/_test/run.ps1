[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

& (Join-Path $PSScriptRoot 'launcher-build.ps1')
& (Join-Path $PSScriptRoot 'smoke-entry.ps1')
& (Join-Path $PSScriptRoot 'claim-entry.ps1')
& (Join-Path $PSScriptRoot 'development-declaration.ps1')
& (Join-Path $PSScriptRoot 'app-build.ps1')
& (Join-Path $PSScriptRoot 'app-core.ps1')
& (Join-Path $PSScriptRoot 'web.ps1')
& (Join-Path $PSScriptRoot 'bootstrap-contract.ps1')
& (Join-Path $PSScriptRoot 'shell.ps1')
& (Join-Path $PSScriptRoot 'install-recovery.ps1')
& (Join-Path $PSScriptRoot 'bun.ps1')
& (Join-Path $PSScriptRoot 'pwsh.ps1')
& (Join-Path $PSScriptRoot 'msvc.ps1')
& (Join-Path $PSScriptRoot 'msvc.command.ps1')
& (Join-Path $PSScriptRoot 'msvc.cache.ps1')
& (Join-Path $PSScriptRoot 'rust.ps1')
& (Join-Path $PSScriptRoot 'rust.strict.ps1')

Write-Host '[PASS] Proj test suite' -ForegroundColor Green
$global:LASTEXITCODE = 0
