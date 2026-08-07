[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

. (Join-Path $PSScriptRoot '_toolchain\bootstrap.ps1')

Invoke-ProjBootstrapToolchain -Action {
    param($Toolchain, $Layout)

    $AppTarget = Assert-ProjDevPathInsideDataRoot `
        -Path $Layout.BuildRoot `
        -DataRoot $Toolchain.Context.DataRoot `
        -Activity 'building the Bootstrap application'
    $AppLock = Enter-ProjDevFileLock `
        -Path (Join-Path $Layout.LockRoot 'app-build.lock') `
        -ControlledRoot $Toolchain.Context.DataRoot `
        -TimeoutSeconds 1800
    try {
        & $Layout.AppBuildPath `
            -CargoPath ([string]$Toolchain.CargoPath) `
            -TargetDirectory $AppTarget | Out-Host
    } finally {
        $AppLock.Dispose()
    }

    $LauncherRoot = Assert-ProjDevPathInsideDataRoot `
        -Path $Layout.LauncherBuildRoot `
        -DataRoot $Toolchain.Context.DataRoot `
        -Activity 'building the Proj Launcher'
    $LauncherCandidate = Assert-ProjDevPathInsideDataRoot `
        -Path $Layout.LauncherCandidatePath `
        -DataRoot $Toolchain.Context.DataRoot `
        -Activity 'publishing the Proj Launcher build candidate'
    $LauncherLock = Enter-ProjDevFileLock `
        -Path (Join-Path $Layout.LockRoot 'launcher-build.lock') `
        -ControlledRoot $Toolchain.Context.DataRoot `
        -TimeoutSeconds 1800
    try {
        & $Layout.LauncherBuildPath `
            -CompilerPath ([string]$Toolchain.CompilerPath) `
            -LinkerPath ([string]$Toolchain.LinkerPath) `
            -BuildRoot $LauncherRoot `
            -CandidatePath $LauncherCandidate | Out-Host
    } finally {
        $LauncherLock.Dispose()
    }
}

$global:LASTEXITCODE = 0
