[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

. (Join-Path $PSScriptRoot '_toolchain\bootstrap.ps1')
. (Join-Path $PSScriptRoot '_toolchain\_lib\runtime-release.ps1')
$Layout = Get-ProjBootstrapLayout
function Test-ProjBootstrapRuntime {
    param([Parameter(Mandatory = $true)][object]$BuildLayout)

    $SelectorItem = Get-Item `
        -LiteralPath $BuildLayout.RuntimeCurrentPath `
        -Force `
        -ErrorAction SilentlyContinue
    if ($null -eq $SelectorItem) {
        return $false
    }
    if ($SelectorItem.PSIsContainer -or
        ($SelectorItem.Attributes -band
            [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw (
            'The Bootstrap runtime selector is unsafe: ' +
            $BuildLayout.RuntimeCurrentPath
        )
    }
    try {
        [void](Read-ProjSelectedRuntimeReleaseSet `
            -RuntimeRoot $BuildLayout.RuntimeRoot)
        return $true
    } catch {
        return $false
    }
}

if (Test-ProjBootstrapRuntime -BuildLayout $Layout) {
    $global:LASTEXITCODE = 0
    return
}

$Context = New-ProjBootstrapToolchainContext
$BootstrapLock = Enter-ProjDevFileLock `
    -Path (Join-Path $Layout.LockRoot 'core-bootstrap.lock') `
    -ControlledRoot $Context.DataRoot `
    -TimeoutSeconds 1800
try {
    if (-not (Test-ProjBootstrapRuntime -BuildLayout $Layout)) {
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
                -CandidateCorePath (Join-Path $TargetDirectory (
                    'release\swawkit-proj.exe'
                )) `
                -CandidateHostPath (Join-Path $TargetDirectory (
                    'release\swawkit-proj-host.exe'
                )) `
                -CandidateToolchainPath (Join-Path $TargetDirectory (
                    'release\swawkit-proj-toolchain.exe'
                )) `
                -ProjHome $BuildLayout.ProjHome `
                -CandidateRoot $Toolchain.Context.DataRoot
        }
    }
} finally {
    $BootstrapLock.Dispose()
}

$global:LASTEXITCODE = 0
