$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

[string[]]$Invocation = @($args)
if ($Invocation.Count -eq 0 -or [string]::IsNullOrWhiteSpace($Invocation[0])) {
    throw '.dev.exec requires an executable name followed by optional arguments.'
}

$KernelRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
. (Join-Path $KernelRoot '_toolchain\runtime.ps1')

$Context = New-ProjDevContextFromEnvironment
try {
    Import-ProjDevGeneratedEnvironment -Context $Context | Out-Null
    $Command = Get-Command $Invocation[0] `
        -CommandType Application `
        -ErrorAction SilentlyContinue |
        Select-Object -First 1
} finally {
    Clear-ProjDevSetupExportMetadata
}
if ($null -eq $Command) {
    throw "Executable is unavailable in the project development environment: $($Invocation[0])"
}
[string[]]$Arguments = if ($Invocation.Count -gt 1) {
    $Invocation[1..($Invocation.Count - 1)]
} else {
    @()
}
$ExitCode = Invoke-ProjDevConsoleProcess `
    -Executable ([string]$Command.Source) `
    -Arguments $Arguments `
    -WorkingDirectory $Context.InvocationDirectory
exit ([int]$ExitCode)
