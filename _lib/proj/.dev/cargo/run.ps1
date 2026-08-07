$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

. (Join-Path $PSScriptRoot (
    '..\..\_toolchain\_modules\rust\runtime.ps1'
))

[string[]]$CargoArguments = @($args)
$ExitCode = Invoke-ProjDevRustCommand `
    -ExecutableName 'cargo.exe' `
    -Arguments $CargoArguments
exit ([int]$ExitCode)
