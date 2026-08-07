[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\bootstrap.ps1')

$Toolchain = Initialize-ProjBootstrapToolchain
$TargetRoot = Assert-ProjDevPathInsideDataRoot `
    -Path (Join-Path $Toolchain.Context.DataRoot 'build\app-test') `
    -DataRoot $Toolchain.Context.DataRoot `
    -Activity 'testing the Rust Proj Core'
$TestLock = Enter-ProjDevFileLock `
    -Path (Join-Path $Toolchain.Context.LockRoot 'app-test.lock') `
    -ControlledRoot $Toolchain.Context.DataRoot `
    -TimeoutSeconds 1800
try {
    & $Toolchain.CargoPath `
        test `
        --locked `
        --offline `
        --manifest-path (Join-Path $ProjRoot '_app\Cargo.toml') `
        --target-dir $TargetRoot
    if ($LASTEXITCODE -ne 0) {
        throw "Rust Core tests failed with exit code $LASTEXITCODE."
    }
} finally {
    $TestLock.Dispose()
}

Write-Host '[PASS] Proj Rust Core test suite' -ForegroundColor Green
$global:LASTEXITCODE = 0
