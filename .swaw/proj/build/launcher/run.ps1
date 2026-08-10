$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if ($args.Count -ne 0) {
    throw 'proj.build.launcher does not accept dynamic arguments.'
}

$CommandDataRoot = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT
if ([string]::IsNullOrWhiteSpace($CommandDataRoot)) {
    throw 'The project command data root is unavailable.'
}
$KernelRoot = [IO.Path]::GetFullPath(
    (Join-Path ([string]$env:SWAWKIT_HOME) '_lib\proj')
)
. (Join-Path $KernelRoot '_toolchain\bootstrap.ps1')
. (Join-Path $PSScriptRoot '..\_lib\export.ps1')

$WorkRoot = Assert-ProjDevPathInsideDataRoot `
    -Path (Join-Path $CommandDataRoot 'work\launcher') `
    -DataRoot $CommandDataRoot `
    -Activity 'building the Proj Launcher'
$CandidatePath = Assert-ProjDevPathInsideDataRoot `
    -Path (Join-Path $WorkRoot 'release\template.proj1.exe') `
    -DataRoot $CommandDataRoot `
    -Activity 'staging the Proj Launcher candidate'
$ExportPath = Assert-ProjDevPathInsideDataRoot `
    -Path (Join-Path $CommandDataRoot 'export\template.proj1.exe') `
    -DataRoot $CommandDataRoot `
    -Activity 'publishing the Proj Launcher export'
$BuildLock = Enter-ProjDevFileLock `
    -Path (Join-Path $CommandDataRoot 'locks\build.lock') `
    -ControlledRoot $CommandDataRoot `
    -TimeoutSeconds 1800

try {
    Invoke-ProjBootstrapToolchain -Action {
        param($Toolchain, $Layout)

        & $Layout.LauncherBuildPath `
            -CompilerPath ([string]$Toolchain.CompilerPath) `
            -LinkerPath ([string]$Toolchain.LinkerPath) `
            -BuildRoot $WorkRoot `
            -CandidatePath $CandidatePath | Out-Host
    }
    Publish-ProjBuildCandidate `
        -SourcePath $CandidatePath `
        -ExportPath $ExportPath `
        -CommandDataRoot $CommandDataRoot
} finally {
    $BuildLock.Dispose()
}
