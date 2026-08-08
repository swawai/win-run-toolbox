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
        "'$($Context.EntryCommand) ..entry.env.bun.SWAWKIT_PROJ_BUN_MODE managed', " +
        "then '$Repair'."
    )
}
[void](Get-ProjRequiredCommandExport `
    -DataRoot ([string]$Context.DataRoot) `
    -ProviderAddress ([string]$Context.EnvironmentProviderAddress) `
    -EntryCommand ([string]$Context.EntryCommand))
$BunDefinition = Get-ProjDevBunResolvedDefinition `
    -Context $Context `
    -Definition $BunDefinition

Assert-ProjDevWindowsX64 -ToolName 'Bun'
$AlreadyActive = Assert-ProjDevActiveEnvironmentCompatible -Context $Context
Assert-ProjDevBunReady `
    -Context $Context `
    -Definition $BunDefinition
Import-ProjDevGeneratedEnvironment `
    -Context $Context `
    -AlreadyActive $AlreadyActive | Out-Null
Assert-ProjDevBunEnvironmentCurrent `
    -Context $Context `
    -Definition $BunDefinition

$BunRoot = Get-ProjDevInstallRoot `
    -Context $Context `
    -Definition $BunDefinition
$BunExecutable = Resolve-ProjDevChildPath `
    -Root $BunRoot `
    -RelativePath ([string]$BunDefinition.Executable) `
    -Description 'Bun executable'
[string[]]$BunArguments = @($args)
$BunWorkingDirectory = $Context.InvocationDirectory
$RuntimeWorkingDirectory = [Environment]::GetEnvironmentVariable(
    'SWAWKIT_PROJ_INTERNAL_RUNTIME_WORKING_DIR',
    [EnvironmentVariableTarget]::Process
)
[Environment]::SetEnvironmentVariable(
    'SWAWKIT_PROJ_INTERNAL_RUNTIME_WORKING_DIR',
    $null,
    [EnvironmentVariableTarget]::Process
)
if (-not [string]::IsNullOrWhiteSpace($RuntimeWorkingDirectory)) {
    $RuntimeWorkingDirectory = Get-ProjDevFullPath `
        -Path $RuntimeWorkingDirectory
    if (-not $RuntimeWorkingDirectory.Equals(
        $Context.ProjectRoot,
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw (
            'The internal Bun runtime working directory must be the project root.'
        )
    }
    $BunWorkingDirectory = $RuntimeWorkingDirectory
}
$ExitCode = Invoke-ProjDevConsoleProcess `
    -Executable $BunExecutable `
    -Arguments $BunArguments `
    -WorkingDirectory $BunWorkingDirectory
exit ([int]$ExitCode)
