$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

. (Join-Path $PSScriptRoot (
    '..\..\..\_toolchain\_modules\msvc\runtime.ps1'
))

[string[]]$CompilerArguments = @($args)
$ExitCode = Invoke-ProjDevMsvcCommand `
    -ExecutableName 'cl.exe' `
    -Arguments $CompilerArguments
exit ([int]$ExitCode)
