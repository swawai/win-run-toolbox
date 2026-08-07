Set-StrictMode -Version 2.0

$SetupRoot = [IO.Path]::GetFullPath(
    (Join-Path $PSScriptRoot '..\..\.dev\setup')
)
foreach ($File in @(
    '_lib\runtime.ps1',
    '_modules\bun\module.ps1',
    '_modules\bun\release.ps1',
    '_modules\bun\selection.ps1'
)) {
    . (Join-Path $SetupRoot $File)
}
