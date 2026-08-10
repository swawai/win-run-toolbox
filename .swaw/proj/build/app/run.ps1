$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if ($args.Count -ne 0) {
    throw 'proj.build.app does not accept dynamic arguments.'
}

$ProjHome = [string]$env:SWAWKIT_HOME
$CommandDataRoot = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT
if ([string]::IsNullOrWhiteSpace($ProjHome) -or
    [string]::IsNullOrWhiteSpace($CommandDataRoot)) {
    throw 'The project runtime context is incomplete.'
}
$KernelRoot = Join-Path $ProjHome '_lib\proj'
. (Join-Path $KernelRoot '_toolchain\_modules\rust\runtime.ps1')
. (Join-Path $PSScriptRoot '..\_lib\export.ps1')

$Cargo = Resolve-ProjDevRustCommand -ExecutableName 'cargo.exe'
$BuildPath = Join-Path $KernelRoot '_app\build.ps1'
$WorkRoot = Assert-ProjDevPathInsideDataRoot `
    -Path (Join-Path $CommandDataRoot 'work\cargo') `
    -DataRoot $CommandDataRoot `
    -Activity 'building the Swaw Kit Proj application'
$ExportPath = Assert-ProjDevPathInsideDataRoot `
    -Path (Join-Path $CommandDataRoot 'export\swawkit-proj.exe') `
    -DataRoot $CommandDataRoot `
    -Activity 'publishing the Swaw Kit Proj application export'
$BuildLock = Enter-ProjDevFileLock `
    -Path (Join-Path $CommandDataRoot 'locks\build.lock') `
    -ControlledRoot $CommandDataRoot `
    -TimeoutSeconds 1800
try {
    & $BuildPath `
        -CargoPath ([string]$Cargo.Executable) `
        -TargetDirectory $WorkRoot | Out-Host
    Publish-ProjBuildArtifact `
        -SourcePath (Join-Path $WorkRoot 'release\swawkit-proj.exe') `
        -ExportPath $ExportPath `
        -CommandDataRoot $CommandDataRoot `
        -ProducerAddress 'proj.build.app' `
        -ProducerContract 'swawkit.proj-build-app/v1'
} finally {
    $BuildLock.Dispose()
}
