Set-StrictMode -Version 2.0

$ToolchainRoot = [IO.Path]::GetFullPath(
    (Join-Path $PSScriptRoot '..\..\..\_toolchain')
)
foreach ($File in @(
    '_lib\runtime.ps1',
    '_modules\bun\module.ps1',
    '_modules\bun\release.ps1',
    '_modules\bun\selection.ps1'
)) {
    . (Join-Path $ToolchainRoot $File)
}
