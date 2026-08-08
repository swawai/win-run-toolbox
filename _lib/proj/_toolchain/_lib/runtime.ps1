Set-StrictMode -Version 2.0

foreach ($File in @(
    'declaration.ps1',
    'generation.ps1',
    'foundation.ps1',
    'command-export.ps1',
    'state.ps1',
    'activation.ps1',
    'console-process.ps1'
)) {
    . (Join-Path $PSScriptRoot $File)
}
