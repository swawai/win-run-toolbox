[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ToolchainPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Invoke-ProjStatusToolchainFixture {
    param([Parameter(Mandatory = $true)][string]$Executable)

    $Info = [Diagnostics.ProcessStartInfo]::new()
    $Info.FileName = $Executable
    $Info.Arguments = 'command-v1 dev.status'
    $Info.UseShellExecute = $false
    $Info.CreateNoWindow = $true
    $Info.RedirectStandardOutput = $true
    $Info.RedirectStandardError = $true
    $Process = [Diagnostics.Process]::Start($Info)
    try {
        $StandardOutput = $Process.StandardOutput.ReadToEnd()
        $StandardError = $Process.StandardError.ReadToEnd()
        $Process.WaitForExit()
        return [pscustomobject][ordered]@{
            ExitCode = [int]$Process.ExitCode
            Output = ($StandardOutput + $StandardError).TrimEnd()
        }
    } finally {
        $Process.Dispose()
    }
}

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\setup.ps1')
. (Join-Path $PSScriptRoot '_lib\bun-fixture.ps1')

$EnvironmentNames = @(
    'SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL',
    'SWAWKIT_PROJ_CORE_COMMAND_PHASE',
    'SWAWKIT_PROJ_CORE_COMMAND_ADDRESS',
    'SWAWKIT_HOME',
    'SWAWKIT_PROJ_TARGET_PROJECT_ROOT',
    'SWAWKIT_PROJ_ACTION_ROOT',
    'SWAWKIT_PROJ_DATA_ROOT',
    'SWAWKIT_PROJ_ENTRY_COMMAND',
    'SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR',
    'SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION',
    'SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION',
    'SWAWKIT_PROJ_CORE_TOOLCHAIN_EXECUTABLE',
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
$ReparseDataRoot = Join-Path $ControlHome "data\proj.$EntryName-reparse"
$ModulesJunction = ''
$ResolvedToolchainPath = [IO.Path]::GetFullPath($ToolchainPath)
if (-not [IO.File]::Exists($ResolvedToolchainPath)) {
    throw "Toolchain test candidate is missing: $ResolvedToolchainPath"
}

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
        SWAWKIT_PROJ_CORE_COMMAND_PHASE = 'run'
        SWAWKIT_PROJ_CORE_COMMAND_ADDRESS = '.dev.status'
        SWAWKIT_HOME = $ControlHome
        SWAWKIT_PROJ_TARGET_PROJECT_ROOT = $ProjectRoot
        SWAWKIT_PROJ_ACTION_ROOT = $ActionRoot
        SWAWKIT_PROJ_DATA_ROOT = $DataRoot
        SWAWKIT_PROJ_ENTRY_COMMAND = 'swawkit'
        SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR = $ProjectRoot
        SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION = ('sha256-' + ('a' * 64))
        SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION = $ProfileRevision
        SWAWKIT_PROJ_CORE_TOOLCHAIN_EXECUTABLE = $ResolvedToolchainPath
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

    $StatusResult = Invoke-ProjStatusToolchainFixture `
        -Executable $ResolvedToolchainPath
    Assert-ProjBunTest `
        -Condition (
            $StatusResult.ExitCode -eq 0 -and
            $StatusResult.Output -like '*[[]READY[]]*bun 1.2.15*upstream*' -and
            $StatusResult.Output -like '*GitHub Release digest*' -and
            $StatusResult.Output -like '*SWAWKIT_PROJ_BUN_SHA256*'
        ) `
        -Message ".dev.status did not report upstream trust: $($StatusResult.Output)"

    $MetadataPath = Get-ProjDevInstallMetadataPath -InstallRoot $InstallRoot
    [byte[]]$OriginalMetadata = [IO.File]::ReadAllBytes($MetadataPath)
    try {
        $InvalidMetadata = [Text.Encoding]::UTF8.GetString(
            $OriginalMetadata
        ) | ConvertFrom-Json
        $InvalidMetadata.sourceUrl = ''
        [IO.File]::WriteAllText(
            $MetadataPath,
            (ConvertTo-ProjDevJsonText -Value $InvalidMetadata),
            [Text.UTF8Encoding]::new($false)
        )
        $MissingSourceUrlStatus = Invoke-ProjStatusToolchainFixture `
            -Executable $ResolvedToolchainPath
        Assert-ProjBunTest `
            -Condition (
                $MissingSourceUrlStatus.ExitCode -eq 0 -and
                $MissingSourceUrlStatus.Output -cmatch
                    '(?m)^\[MISSING\] bun 1\.2\.15\s+unpinned\s+' -and
                $MissingSourceUrlStatus.Output -cnotmatch
                    '(?m)^\[READY\] bun 1\.2\.15 ' -and
                $MissingSourceUrlStatus.Output -cnotmatch
                    '(?m)^\[MISSING\] bun 1\.2\.15\s+upstream\s+'
            ) `
            -Message (
                '.dev.status trusted metadata without sourceUrl: ' +
                $MissingSourceUrlStatus.Output
            )
    } finally {
        [IO.File]::WriteAllBytes($MetadataPath, $OriginalMetadata)
    }

    $BunxPath = Join-Path $InstallRoot 'bunx.cmd'
    [byte[]]$OriginalBunx = [IO.File]::ReadAllBytes($BunxPath)
    [byte[]]$TamperedBunx = $OriginalBunx.Clone()
    $TamperedBunx[0] = $TamperedBunx[0] -bxor 1
    try {
        [IO.File]::WriteAllBytes($BunxPath, $TamperedBunx)
        $TamperedStatus = Invoke-ProjStatusToolchainFixture `
            -Executable $ResolvedToolchainPath
        Assert-ProjBunTest `
            -Condition (
                $TamperedStatus.ExitCode -eq 0 -and
                $TamperedStatus.Output -cmatch
                    '(?m)^\[MISSING\] bun 1\.2\.15\s+upstream\s+' -and
                $TamperedStatus.Output -cnotmatch
                    '(?m)^\[READY\] bun 1\.2\.15 '
            ) `
            -Message (
                '.dev.status accepted same-length Bun tampering or lost ' +
                'the validated source trust: ' + $TamperedStatus.Output
            )
    } finally {
        [IO.File]::WriteAllBytes($BunxPath, $OriginalBunx)
    }

    $env:SWAWKIT_PROJ_CORE_COMMAND_ADDRESS = '.dev.setup'
    $SetupResult = Invoke-ProjToolchainCommandFixture `
        -Executable $ResolvedToolchainPath `
        -Handler 'dev.setup'
    $env:SWAWKIT_PROJ_CORE_COMMAND_ADDRESS = '.dev.status'
    Assert-ProjBunTest `
        -Condition (
            $SetupResult.ExitCode -eq 0 -and
            $SetupResult.Output -like '*Bun 1.2.15 is ready*' -and
            $SetupResult.Output -like '*GitHub Release digest*' -and
            [IO.File]::Exists($Context.EnvCmdPath) -and
            [IO.File]::Exists($Context.EnvPs1Path)
        ) `
        -Message ".dev.setup did not preserve non-blocking trust: $($SetupResult.Output)"
    $ReadyStatus = Invoke-ProjStatusToolchainFixture `
        -Executable $ResolvedToolchainPath
    Assert-ProjBunTest `
        -Condition ($ReadyStatus.Output -cmatch
            '\[READY\] \.dev\.setup publication [a-f0-9]{8}') `
        -Message (
            '.dev.status did not report the provider publication token: ' +
            $ReadyStatus.Output
        )

    [IO.File]::WriteAllText(
        (Get-ProjDevBunSelectionPath -Context $Context),
        (ConvertTo-ProjDevJsonText -Value ([ordered]@{
            schema = 'swawkit.proj-dev.bun-selection.v0'
            selector = 'latest'
            version = '1.2.15'
            sourceSha256 = 'e' * 64
            sourceVerification = 'github'
        })),
        [Text.UTF8Encoding]::new($false)
    )
    $env:SWAWKIT_PROJ_BUN_VERSION = 'latest'
    $MismatchedSelection = Invoke-ProjStatusToolchainFixture `
        -Executable $ResolvedToolchainPath
    Assert-ProjBunTest `
        -Condition (
            $MismatchedSelection.ExitCode -eq 0 -and
            $MismatchedSelection.Output -cmatch
                '(?m)^\[MISSING\] bun latest -> 1\.2\.15 ' -and
            $MismatchedSelection.Output -cnotmatch
                '(?m)^\[READY\] bun latest -> 1\.2\.15 '
        ) `
        -Message (
            '.dev.status accepted an install whose digest disagreed with the ' +
            'latest selection: ' + $MismatchedSelection.Output
        )

    $ExternalModules = Join-Path $TemporaryRoot 'external-modules'
    $UnsafeSetupRoot = Join-Path $ExternalModules (
        'kernel\.dev\setup'
    )
    [void][IO.Directory]::CreateDirectory($UnsafeSetupRoot)
    [void][IO.Directory]::CreateDirectory((Join-Path $UnsafeSetupRoot 'export\bun'))
    [IO.File]::WriteAllText(
        (Join-Path $UnsafeSetupRoot 'export\bun\.swawkit-dev-selection.json'),
        (ConvertTo-ProjDevJsonText -Value ([ordered]@{
            schema = 'swawkit.proj-dev.bun-selection.v0'
            selector = 'latest'
            version = '9.9.9'
            sourceSha256 = 'f' * 64
            sourceVerification = 'unverified'
        })),
        [Text.UTF8Encoding]::new($false)
    )
    $ModulesJunction = Join-Path $ReparseDataRoot 'modules'
    [void][IO.Directory]::CreateDirectory($ReparseDataRoot)
    [void](New-Item `
        -ItemType Junction `
        -Path $ModulesJunction `
        -Target $ExternalModules)
    $env:SWAWKIT_PROJ_DATA_ROOT = $ReparseDataRoot
    $env:SWAWKIT_PROJ_BUN_VERSION = 'latest'
    $UnsafeStatus = Invoke-ProjStatusToolchainFixture `
        -Executable $ResolvedToolchainPath
    Assert-ProjBunTest `
        -Condition (
            $UnsafeStatus.ExitCode -ne 0 -and
            $UnsafeStatus.Output -like '*must be a regular directory*' -and
            $UnsafeStatus.Output -notlike '*latest -> 9.9.9*'
        ) `
        -Message (
            '.dev.status followed a reparse-point Export outside DataRoot: ' +
            $UnsafeStatus.Output
        )

    $env:SWAWKIT_PROJ_DATA_ROOT = $PinnedDataRoot
    $env:SWAWKIT_PROJ_BUN_VERSION = '1.2.15'
    $env:SWAWKIT_PROJ_BUN_SHA256 = 'e' * 64
    $PinnedStatus = Invoke-ProjStatusToolchainFixture `
        -Executable $ResolvedToolchainPath
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
    if (-not [string]::IsNullOrWhiteSpace($ModulesJunction) -and
        [IO.Directory]::Exists($ModulesJunction)) {
        [IO.Directory]::Delete($ModulesJunction)
    }
    foreach ($OwnedDataRoot in @(
        $DataRoot,
        $PinnedDataRoot,
        $ReparseDataRoot
    )) {
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
