$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if ($args.Count -ne 0) {
    throw 'proj.build.app does not accept dynamic arguments.'
}

$ProjHome = [string]$env:SWAWKIT_HOME
$DataRoot = [string]$env:SWAWKIT_PROJ_DATA_ROOT
if ([string]::IsNullOrWhiteSpace($ProjHome) -or
    [string]::IsNullOrWhiteSpace($DataRoot)) {
    throw 'The project runtime context is incomplete.'
}
$KernelRoot = Join-Path $ProjHome '_lib\proj'
. (Join-Path $KernelRoot '.dev\setup\_modules\rust\runtime.ps1')

$Cargo = Resolve-ProjDevRustCommand -ExecutableName 'cargo.exe'
$BuildPath = Join-Path $KernelRoot '_app\build.ps1'
$TargetDirectory = Assert-ProjDevPathInsideDataRoot `
    -Path (Join-Path $DataRoot '_build\app') `
    -DataRoot $DataRoot `
    -Activity 'building the Swaw Kit Proj application'
& $BuildPath `
    -CargoPath ([string]$Cargo.Executable) `
    -TargetDirectory $TargetDirectory
