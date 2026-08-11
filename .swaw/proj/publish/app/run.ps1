$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if ($args.Count -ne 0) {
    throw 'proj.publish.app does not accept dynamic arguments.'
}

$ProjHome = [string]$env:SWAWKIT_HOME
$DataRoot = [string]$env:SWAWKIT_PROJ_DATA_ROOT
$EntryCommand = [string]$env:SWAWKIT_PROJ_ENTRY_COMMAND
if ([string]::IsNullOrWhiteSpace($ProjHome) -or
    [string]::IsNullOrWhiteSpace($DataRoot) -or
    [string]::IsNullOrWhiteSpace($EntryCommand)) {
    throw 'The project runtime context is incomplete.'
}
$KernelRoot = Join-Path $ProjHome '_lib\proj'
. (Join-Path $KernelRoot '_toolchain\runtime.ps1')
. (Join-Path $PSScriptRoot '..\..\build\_lib\export.ps1')
. (Join-Path $PSScriptRoot '..\..\build\_lib\release-set.ps1')
. (Join-Path $KernelRoot '_toolchain\_lib\runtime-release.ps1')

$ProviderAddress = 'proj.build.app'
$ProviderContract = 'swawkit.proj-build-app/v3'
$ProviderRoot = Get-ProjActionCommandDataRoot `
    -DataRoot $DataRoot `
    -Address $ProviderAddress
$ProviderLock = Enter-ProjDevFileLock `
    -Path (Join-Path $ProviderRoot 'locks\build.lock') `
    -ControlledRoot $DataRoot `
    -TimeoutSeconds 120
try {
    $ReleaseSet = Get-ProjRequiredBuildReleaseSet `
        -DataRoot $DataRoot `
        -ProviderAddress $ProviderAddress `
        -EntryCommand $EntryCommand `
        -ProducerContract $ProviderContract `
        -ArtifactNames @(
            'swawkit-proj.exe',
            'swawkit-proj-host.exe',
            'swawkit-proj-toolchain.exe'
        )
    Publish-ProjRuntimeReleaseSet `
        -ReleaseSet $ReleaseSet `
        -ProjHome $ProjHome `
        -CacheDataRoot (Join-Path $ProjHome 'data\proj_cache') | Out-Null
} finally {
    $ProviderLock.Dispose()
}
