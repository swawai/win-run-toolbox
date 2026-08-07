$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if ($args.Count -ne 0) {
    throw 'proj.build.launcher does not accept dynamic arguments.'
}

$KernelRoot = [IO.Path]::GetFullPath(
    (Join-Path ([string]$env:SWAWKIT_HOME) '_lib\proj')
)
. (Join-Path $KernelRoot '_toolchain\bootstrap.ps1')

Invoke-ProjBootstrapToolchain -Action {
    param($Toolchain, $Layout)

    $BuildRoot = Assert-ProjDevPathInsideDataRoot `
        -Path $Layout.LauncherBuildRoot `
        -DataRoot $Toolchain.Context.DataRoot `
        -Activity 'building the Proj Launcher'
    $CandidatePath = Assert-ProjDevPathInsideDataRoot `
        -Path $Layout.LauncherCandidatePath `
        -DataRoot $Toolchain.Context.DataRoot `
        -Activity 'publishing the Proj Launcher build candidate'
    $BuildLock = Enter-ProjDevFileLock `
        -Path (Join-Path $Layout.LockRoot 'launcher-build.lock') `
        -ControlledRoot $Toolchain.Context.DataRoot `
        -TimeoutSeconds 1800
    try {
        & $Layout.LauncherBuildPath `
            -CompilerPath ([string]$Toolchain.CompilerPath) `
            -LinkerPath ([string]$Toolchain.LinkerPath) `
            -BuildRoot $BuildRoot `
            -CandidatePath $CandidatePath | Out-Host
    } finally {
        $BuildLock.Dispose()
    }
}
