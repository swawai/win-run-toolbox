[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\setup.ps1')

$Rejected = $false
try {
    [void](Invoke-ProjDevRustCommand `
        -ExecutableName 'cargo.exe' `
        -Arguments @('+nightly', '--version'))
} catch {
    $Rejected = $_.Exception.Message -like (
        '*+toolchain overrides are not allowed*'
    )
}
if (-not $Rejected) {
    throw 'Proj Rust strict test failed: +toolchain override was accepted.'
}

Write-Host '[PASS] Proj Rust strict selector test' -ForegroundColor Green
$global:LASTEXITCODE = 0
