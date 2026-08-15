$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

. (Join-Path $PSScriptRoot '_lib\runtime.ps1')

$Context = New-ProjDevContextFromEnvironment
$Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
$BunDefinition = Get-ProjDevBunDefinition
if ($null -eq $BunDefinition) {
    throw (
        'Bun is disabled for this project. Run ' +
        "'$($Context.EntryCommand) .dev.bun.mode managed', " +
        "then '$Repair'."
    )
}
try {
    Import-ProjDevGeneratedEnvironment -Context $Context | Out-Null
    $BunDefinition = Get-ProjDevBunResolvedDefinition `
        -Context $Context `
        -Definition $BunDefinition
    Assert-ProjDevWindowsX64 -ToolName 'Bun'
    Assert-ProjDevBunEnvironmentCurrent `
        -Context $Context `
        -Definition $BunDefinition
} finally {
    Clear-ProjDevSetupExportMetadata
}

$BunRoot = Get-ProjDevInstallRoot `
    -Context $Context `
    -Definition $BunDefinition
$BunExecutable = Resolve-ProjDevChildPath `
    -Root $BunRoot `
    -RelativePath ([string]$BunDefinition.Executable) `
    -Description 'Bun executable'
[string[]]$BunArguments = @($args)
$ExitCode = Invoke-ProjDevConsoleProcess `
    -Executable $BunExecutable `
    -Arguments $BunArguments `
    -WorkingDirectory $Context.InvocationDirectory
exit ([int]$ExitCode)
