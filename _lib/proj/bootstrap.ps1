[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

. (Join-Path $PSScriptRoot '_toolchain\bootstrap.ps1')
$Layout = Get-ProjBootstrapLayout
if ([IO.File]::Exists($Layout.RuntimePath)) {
    $global:LASTEXITCODE = 0
    return
}

$Context = New-ProjBootstrapToolchainContext
$BootstrapLock = Enter-ProjDevFileLock `
    -Path (Join-Path $Layout.LockRoot 'core-bootstrap.lock') `
    -ControlledRoot $Context.DataRoot `
    -TimeoutSeconds 1800
try {
    if (-not [IO.File]::Exists($Layout.RuntimePath)) {
        Invoke-ProjBootstrapToolchain -Action {
            param($Toolchain, $BuildLayout)

            $TargetDirectory = Assert-ProjDevPathInsideDataRoot `
                -Path $BuildLayout.BuildRoot `
                -DataRoot $Toolchain.Context.DataRoot `
                -Activity 'building the Bootstrap application'
            $BuildLock = Enter-ProjDevFileLock `
                -Path (Join-Path $BuildLayout.LockRoot 'app-build.lock') `
                -ControlledRoot $Toolchain.Context.DataRoot `
                -TimeoutSeconds 1800
            try {
                & $BuildLayout.AppBuildPath `
                    -CargoPath ([string]$Toolchain.CargoPath) `
                    -TargetDirectory $TargetDirectory | Out-Host
            } finally {
                $BuildLock.Dispose()
            }
            & $BuildLayout.AppPublishPath `
                -CandidatePath (Join-Path $TargetDirectory (
                    'release\swawkit-proj.exe'
                )) `
                -RuntimePath $BuildLayout.RuntimePath `
                -CandidateRoot $Toolchain.Context.DataRoot
        }
    }
} finally {
    $BootstrapLock.Dispose()
}

$global:LASTEXITCODE = 0
