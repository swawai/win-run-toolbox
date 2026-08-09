[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\setup.ps1')
. (Join-Path $PSScriptRoot '_lib\bun-fixture.ps1')

$EnvironmentNames = @(
    'SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL',
    'SWAWKIT_HOME',
    'SWAWKIT_PROJ_TARGET_PROJECT_ROOT',
    'SWAWKIT_PROJ_ACTION_ROOT',
    'SWAWKIT_PROJ_DATA_ROOT',
    'SWAWKIT_PROJ_ENTRY_COMMAND',
    'SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR',
    'SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION',
    'SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION',
    'SWAWKIT_PROJ_BUN_MODE',
    'SWAWKIT_PROJ_BUN_VERSION',
    'SWAWKIT_PROJ_BUN_SHA256'
)
$EnvironmentSnapshot = Enter-ProjBunIsolatedEnvironment `
    -ProjectVariableNames $EnvironmentNames
$TestTemporaryBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TestTemporaryBase)
$TemporaryRoot = Join-Path $TestTemporaryBase (
    "swawkit-proj-bun-status-$([Guid]::NewGuid().ToString('N'))"
)
$ControlHome = [IO.Path]::GetFullPath((Join-Path $ProjRoot '..\..'))
$EntryName = "test-bun-status-$([Guid]::NewGuid().ToString('N'))"
$PinnedEntryName = "$EntryName-pinned"
$DataRoot = Join-Path $ControlHome "data\proj.$EntryName"
$PinnedDataRoot = Join-Path $ControlHome "data\proj.$PinnedEntryName"
$SystemPowerShell = Join-Path $env:SystemRoot (
    'System32\WindowsPowerShell\v1.0\powershell.exe'
)

try {
    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    $ActionRoot = Join-Path $ProjectRoot '.swaw'
    [void][IO.Directory]::CreateDirectory($ActionRoot)
    [void][IO.Directory]::CreateDirectory($DataRoot)
    $ProfilePath = Join-Path $DataRoot '_profile.json'
    [IO.File]::WriteAllText($ProfilePath, '{}')
    $ProfileRevision = 'sha256-' + (
        Get-ProjDevFileSha256 -Path $ProfilePath
    )
    Set-ProjBunProcessEnvironment -Values @{
        SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL = '1'
        SWAWKIT_HOME = $ControlHome
        SWAWKIT_PROJ_TARGET_PROJECT_ROOT = $ProjectRoot
        SWAWKIT_PROJ_ACTION_ROOT = $ActionRoot
        SWAWKIT_PROJ_DATA_ROOT = $DataRoot
        SWAWKIT_PROJ_ENTRY_COMMAND = 'swawkit'
        SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR = $ProjectRoot
        SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION = ('sha256-' + ('a' * 64))
        SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION = $ProfileRevision
        SWAWKIT_PROJ_BUN_MODE = 'managed'
        SWAWKIT_PROJ_BUN_VERSION = '1.2.15'
        SWAWKIT_PROJ_BUN_SHA256 = ''
    }
    $Context = New-ProjDevContextFromEnvironment
    Assert-ProjBunTest `
        -Condition ($Context.CacheDataRoot.Equals(
            (Join-Path $ControlHome 'data\proj_cache'),
            [StringComparison]::OrdinalIgnoreCase
        )) `
        -Message 'the production context did not derive the shared cache from the entry root'
    $Definition = Get-ProjDevBunDefinition
    $Definition.Sha256 = 'f' * 64
    $Definition.Verification = 'github'
    $InstallRoot = Get-ProjDevInstallRoot `
        -Context $Context `
        -Definition $Definition
    [void][IO.Directory]::CreateDirectory($InstallRoot)
    New-ProjBunFixtureExecutable `
        -Path (Join-Path $InstallRoot 'bun.exe') `
        -Version '1.2.15'
    [IO.File]::WriteAllText(
        (Join-Path $InstallRoot 'bunx.cmd'),
        "@echo off`r`n`"%~dp0bun.exe`" x %*`r`n",
        [Text.UTF8Encoding]::new($false)
    )
    Write-ProjDevInstallMetadata `
        -Definition $Definition `
        -InstallRoot $InstallRoot

    $StatusResult = Invoke-ProjBunEntryFixture `
        -PowerShell $SystemPowerShell `
        -EntryPath (Join-Path $ProjRoot '.dev\status\run.ps1') `
        -Arguments @()
    Assert-ProjBunTest `
        -Condition (
            $StatusResult.ExitCode -eq 0 -and
            $StatusResult.Output -like '*[[]READY[]]*bun 1.2.15*upstream*' -and
            $StatusResult.Output -like '*GitHub Release digest*' -and
            $StatusResult.Output -like '*SWAWKIT_PROJ_BUN_SHA256*'
        ) `
        -Message ".dev.status did not report upstream trust: $($StatusResult.Output)"

    $SetupResult = Invoke-ProjBunEntryFixture `
        -PowerShell $SystemPowerShell `
        -EntryPath (Join-Path $ProjRoot '.dev\setup\run.ps1') `
        -Arguments @()
    Assert-ProjBunTest `
        -Condition (
            $SetupResult.ExitCode -eq 0 -and
            $SetupResult.Output -like '*Bun 1.2.15 is ready*' -and
            $SetupResult.Output -like '*GitHub Release digest*' -and
            [IO.File]::Exists($Context.EnvCmdPath) -and
            [IO.File]::Exists($Context.EnvPs1Path)
        ) `
        -Message ".dev.setup did not preserve non-blocking trust: $($SetupResult.Output)"
    $ReadyStatus = Invoke-ProjBunEntryFixture `
        -PowerShell $SystemPowerShell `
        -EntryPath (Join-Path $ProjRoot '.dev\status\run.ps1') `
        -Arguments @()
    Assert-ProjBunTest `
        -Condition ($ReadyStatus.Output -cmatch
            '\[READY\] \.dev\.setup publication [a-f0-9]{8}') `
        -Message (
            '.dev.status did not report the provider publication token: ' +
            $ReadyStatus.Output
        )

    $env:SWAWKIT_PROJ_DATA_ROOT = $PinnedDataRoot
    $env:SWAWKIT_PROJ_BUN_SHA256 = 'e' * 64
    $PinnedStatus = Invoke-ProjBunEntryFixture `
        -PowerShell $SystemPowerShell `
        -EntryPath (Join-Path $ProjRoot '.dev\status\run.ps1') `
        -Arguments @()
    Assert-ProjBunTest `
        -Condition (
            $PinnedStatus.ExitCode -eq 0 -and
            $PinnedStatus.Output -like '*[[]MISSING[]]*bun 1.2.15*pinned*' -and
            $PinnedStatus.Output -notlike '*WARNING*' -and
            -not [IO.Directory]::Exists(
                (Join-Path $PinnedDataRoot 'modules\kernel\.dev\setup\export')
            )
        ) `
        -Message ".dev.status was not read-only for pinned state: $($PinnedStatus.Output)"

    Write-Host '[PASS] Proj Bun development status test' `
        -ForegroundColor Green
} finally {
    Exit-ProjBunIsolatedEnvironment -Snapshot $EnvironmentSnapshot
    foreach ($OwnedDataRoot in @($DataRoot, $PinnedDataRoot)) {
        if ([IO.Directory]::Exists($OwnedDataRoot) -and
            [IO.Path]::GetDirectoryName($OwnedDataRoot).Equals(
                (Join-Path $ControlHome 'data'),
                [StringComparison]::OrdinalIgnoreCase
            )) {
            Remove-Item -LiteralPath $OwnedDataRoot -Recurse -Force
        }
    }
    $ResolvedTemporaryRoot = [IO.Path]::GetFullPath($TemporaryRoot)
    $SystemTemporaryRoot = [IO.Path]::GetFullPath(
        $TestTemporaryBase
    ).TrimEnd('\') + '\'
    if ($ResolvedTemporaryRoot.StartsWith(
        $SystemTemporaryRoot,
        [StringComparison]::OrdinalIgnoreCase
    ) -and
        [IO.Path]::GetFileName($ResolvedTemporaryRoot).StartsWith(
            'swawkit-proj-bun-status-',
            [StringComparison]::Ordinal
        ) -and
        [IO.Directory]::Exists($ResolvedTemporaryRoot)) {
        Remove-Item -LiteralPath $ResolvedTemporaryRoot -Recurse -Force
    }
}
