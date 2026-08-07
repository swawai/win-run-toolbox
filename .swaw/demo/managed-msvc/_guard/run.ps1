$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if (@($args).Count -ne 0) {
    throw 'The managed MSVC execution guard does not accept arguments.'
}
$KernelRoot = [IO.Path]::GetFullPath(
    (Join-Path ([string]$env:SWAWKIT_HOME) '_lib\proj')
)
. (Join-Path $KernelRoot '.dev\setup\_modules\msvc\runtime.ps1')
[void](Assert-ProjDevMsvcCommandReady)

$global:LASTEXITCODE = 0
