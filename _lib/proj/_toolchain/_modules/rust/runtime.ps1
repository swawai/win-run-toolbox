Set-StrictMode -Version 2.0

$SetupRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
foreach ($RelativePath in @(
    '_lib\runtime.ps1',
    '_modules\msvc\module.ps1',
    '_modules\msvc\environment.ps1',
    '_modules\rust\module.ps1',
    '_modules\rust\state.ps1',
    '_modules\rust\environment.ps1',
    '_modules\rust\command.ps1'
)) {
    . (Join-Path $SetupRoot $RelativePath)
}
