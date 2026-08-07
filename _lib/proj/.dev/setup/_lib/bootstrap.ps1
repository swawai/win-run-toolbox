Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot 'runtime.ps1')

foreach ($File in @(
    'artifact.ps1',
    'recovery.ps1',
    'install.ps1',
    'environment.ps1'
)) {
    . (Join-Path $PSScriptRoot $File)
}

$ModuleRoot = [IO.Path]::GetFullPath(
    (Join-Path $PSScriptRoot '..\_modules')
)
foreach ($File in @(
    'bun\module.ps1',
    'bun\release.ps1',
    'bun\selection.ps1',
    'bun\install.ps1',
    'pwsh\module.ps1',
    'pwsh\release.ps1',
    'pwsh\selection.ps1',
    'pwsh\install.ps1',
    'msvc\module.ps1',
    'msvc\payload.ps1',
    'msvc\manifest.ps1',
    'msvc\install.ps1',
    'msvc\environment.ps1',
    'rust\module.ps1',
    'rust\metadata.ps1',
    'rust\state.ps1',
    'rust\release.ps1',
    'rust\process.ps1',
    'rust\install.ps1',
    'rust\environment.ps1',
    'rust\command.ps1'
)) {
    . (Join-Path $ModuleRoot $File)
}
