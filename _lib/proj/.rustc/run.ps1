$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

. (Join-Path $PSScriptRoot (
    '..\.dev\setup\_modules\rust\runtime.ps1'
))

[string[]]$RustcArguments = @($args)
$ExitCode = Invoke-ProjDevRustCommand `
    -ExecutableName 'rustc.exe' `
    -Arguments $RustcArguments
exit ([int]$ExitCode)
