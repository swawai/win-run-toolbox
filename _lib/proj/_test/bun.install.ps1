[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\setup.ps1')
. (Join-Path $PSScriptRoot '_lib\bun-fixture.ps1')

$EnvironmentNames = @(
    'SWAWKIT_PROJ_PROTOCOL',
    'SWAWKIT_HOME',
    'SWAWKIT_PROJ_TARGET_PROJECT_ROOT',
    'SWAWKIT_PROJ_ACTION_ROOT',
    'SWAWKIT_PROJ_DATA_ROOT',
    'SWAWKIT_PROJ_ENTRY_COMMAND',
    'SWAWKIT_PROJ_ENTRY_FILE',
    'SWAWKIT_PROJ_INVOCATION_DIR',
    'SWAWKIT_PROJ_BUN_MODE',
    'SWAWKIT_PROJ_BUN_VERSION',
    'SWAWKIT_PROJ_BUN_SHA256',
    'SWAWKIT_PROJ_UV_MODE',
    'SWAWKIT_PROJ_UV_VERSION',
    'SWAWKIT_PROJ_PYTHON_MODE',
    'SWAWKIT_PROJ_PYTHON_VERSION',
    'SWAWKIT_PROJ_PWSH_MODE',
    'SWAWKIT_PROJ_GO_MODE',
    'SWAWKIT_PROJ_GO_VERSION',
    'SWAWKIT_PROJ_TEST_BUN_CAPTURE'
)
$EnvironmentSnapshot = Enter-ProjBunIsolatedEnvironment `
    -ProjectVariableNames $EnvironmentNames
$UserPathBefore = [Environment]::GetEnvironmentVariable('PATH', 'User')
$MachinePathBefore = [Environment]::GetEnvironmentVariable('PATH', 'Machine')
$TestTemporaryBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TestTemporaryBase)
$TemporaryRoot = Join-Path $TestTemporaryBase (
    "swawkit-proj-bun-$([Guid]::NewGuid().ToString('N'))"
)
$ControlHome = [IO.Path]::GetFullPath((Join-Path $ProjRoot '..\..'))
$SetupEntryName = "test-bun-setup-$([Guid]::NewGuid().ToString('N'))"
$SystemPowerShell = Join-Path $env:SystemRoot (
    'System32\WindowsPowerShell\v1.0\powershell.exe'
)

try {
    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    $DataRoot = Join-Path $TemporaryRoot 'data root'
    $CacheDataRoot = Join-Path $TemporaryRoot 'shared cache'
    $FixtureRoot = Join-Path $TemporaryRoot 'fixture'
    $ArchiveRoot = Join-Path $FixtureRoot 'archive'
    $BunArchiveRoot = Join-Path $ArchiveRoot 'bun-windows-x64'
    $InvocationRoot = Join-Path $ProjectRoot 'work area'
    foreach ($Directory in @(
        $ProjectRoot,
        $DataRoot,
        $BunArchiveRoot,
        $InvocationRoot
    )) {
        [void][IO.Directory]::CreateDirectory($Directory)
    }

    $FixtureExecutable = Join-Path $BunArchiveRoot 'bun.exe'
    New-ProjBunFixtureExecutable `
        -Path $FixtureExecutable `
        -Version '1.2.15'
    $ArchivePath = Join-Path $FixtureRoot 'bun-windows-x64.zip'
    [IO.Compression.ZipFile]::CreateFromDirectory($ArchiveRoot, $ArchivePath)
    $Definition = New-ProjBunTestDefinition `
        -ArchivePath $ArchivePath `
        -Sha256 (Get-ProjDevFileSha256 -Path $ArchivePath)
    $Context = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $DataRoot `
        -CacheDataRoot $CacheDataRoot `
        -EntryCommand 'swawkit' `
        -InvocationDirectory $InvocationRoot

    $Changed = Install-ProjDevBun `
        -Context $Context `
        -Definition $Definition
    Assert-ProjBunTest -Condition $Changed -Message 'first install was skipped'
    Assert-ProjBunTest `
        -Condition (Test-ProjDevInstalled `
            -Context $Context `
            -Definition $Definition) `
        -Message 'trusted fixture installation was not recognized'
    $InstallRoot = Get-ProjDevInstallRoot `
        -Context $Context `
        -Definition $Definition
    $ExpectedBunx = "@echo off`r`n`"%~dp0bun.exe`" x %*`r`n"
    Assert-ProjBunTest `
        -Condition ([IO.File]::ReadAllText(
            (Join-Path $InstallRoot 'bunx.cmd')
        ) -ceq $ExpectedBunx) `
        -Message 'bunx.cmd shim is not byte-compatible with the baseline'

    $Plan = New-ProjDevEnvironmentPlan -Context $Context
    Add-ProjDevBunEnvironment `
        -Context $Context `
        -Definition $Definition `
        -Plan $Plan
    $Scripts = ConvertTo-ProjDevEnvironmentScripts -Plan $Plan
    Assert-ProjBunTest `
        -Condition (Publish-ProjDevEnvironmentScripts `
            -Context $Context `
            -Scripts $Scripts) `
        -Message 'first environment publication was skipped'
    $env:SWAWKIT_PROJ_BUN_MODE = 'managed'
    $env:SWAWKIT_PROJ_BUN_VERSION = '1.2.15'
    $env:SWAWKIT_PROJ_BUN_SHA256 = [string]$Definition.ProjectSha256
    Assert-ProjBunTest `
        -Condition (Publish-ProjDevEnvironmentState `
            -Context $Context `
            -GenerationId ([string]$Scripts.GenerationId)) `
        -Message 'first environment state publication was skipped'
    $PublishedState = Read-ProjDevelopmentEnvironmentState `
        -EnvironmentRoot $Context.EnvironmentRoot
    Assert-ProjBunTest `
        -Condition (
            [string]$PublishedState.GenerationId -ceq
                [string]$Scripts.GenerationId -and
            [string]$PublishedState.Declarations.SWAWKIT_PROJ_BUN_MODE -ceq
                'managed' -and
            [string]$PublishedState.Declarations.SWAWKIT_PROJ_BUN_VERSION -ceq
                '1.2.15'
        ) `
        -Message 'published environment state lost its declaration contract'
    Assert-ProjBunEnvironmentScriptsUsable `
        -Context $Context `
        -ExpectedExecutable (Join-Path $InstallRoot 'bun.exe') `
        -PowerShell $SystemPowerShell
    $EnvCmdHash = Get-ProjDevFileSha256 -Path $Context.EnvCmdPath
    $EnvPs1Hash = Get-ProjDevFileSha256 -Path $Context.EnvPs1Path

    Assert-ProjBunTest `
        -Condition (-not (Install-ProjDevBun `
            -Context $Context `
            -Definition $Definition)) `
        -Message 'valid installation was needlessly replaced'
    Assert-ProjBunTest `
        -Condition (-not (Publish-ProjDevEnvironmentScripts `
            -Context $Context `
            -Scripts (ConvertTo-ProjDevEnvironmentScripts -Plan $Plan))) `
        -Message 'byte-stable environment was needlessly rewritten'
    Assert-ProjBunTest `
        -Condition (-not (Publish-ProjDevEnvironmentState `
            -Context $Context `
            -GenerationId ([string]$Scripts.GenerationId))) `
        -Message 'byte-stable environment state was needlessly rewritten'
    Assert-ProjBunTest `
        -Condition (
            (Get-ProjDevFileSha256 -Path $Context.EnvCmdPath) -ceq $EnvCmdHash -and
            (Get-ProjDevFileSha256 -Path $Context.EnvPs1Path) -ceq $EnvPs1Hash
        ) `
        -Message 'repeated setup changed generated environment bytes'

    $UnpinnedDefinition = New-ProjBunTestDefinition `
        -ArchivePath $ArchivePath `
        -Sha256 (Get-ProjDevFileSha256 -Path $ArchivePath)
    $UnpinnedDefinition.ProjectSha256 = ''
    $UnpinnedDefinition.Sha256 = ''
    $UnpinnedDefinition.Verification = 'unverified'
    $UnpinnedContext = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot (Join-Path $TemporaryRoot 'unpinned-data') `
        -CacheDataRoot $CacheDataRoot
    Assert-ProjBunTest `
        -Condition (Install-ProjDevBun `
            -Context $UnpinnedContext `
            -Definition $UnpinnedDefinition) `
        -Message 'Bun without an upstream or project hash was blocked'
    $UnpinnedMetadata = Get-ProjDevValidInstallMetadata `
        -Context $UnpinnedContext `
        -Definition $UnpinnedDefinition
    Assert-ProjBunTest `
        -Condition (
            $null -ne $UnpinnedMetadata -and
            [string]$UnpinnedMetadata.sourceSha256 -ceq (
                Get-ProjDevFileSha256 -Path $ArchivePath
            ) -and
            [string]$UnpinnedMetadata.sourceVerification -ceq 'unverified'
        ) `
        -Message 'unpinned installation did not record its downloaded SHA-256'

    $UnpinnedInstallRoot = Get-ProjDevInstallRoot `
        -Context $UnpinnedContext `
        -Definition $UnpinnedDefinition
    [IO.File]::AppendAllText(
        (Join-Path $UnpinnedInstallRoot 'bun.exe'),
        'damage'
    )
    $WrongArchiveRoot = Join-Path $FixtureRoot 'wrong-archive'
    $WrongBunRoot = Join-Path $WrongArchiveRoot 'bun-windows-x64'
    [void][IO.Directory]::CreateDirectory($WrongBunRoot)
    New-ProjBunFixtureExecutable `
        -Path (Join-Path $WrongBunRoot 'bun.exe') `
        -Version '9.9.9'
    $WrongArchive = Join-Path $FixtureRoot 'wrong-bun.zip'
    [IO.Compression.ZipFile]::CreateFromDirectory(
        $WrongArchiveRoot,
        $WrongArchive
    )
    $UnpinnedCacheRoot = Get-ProjDevArtifactCacheRoot `
        -Context $UnpinnedContext `
        -Definition $UnpinnedDefinition
    [IO.File]::Copy(
        $WrongArchive,
        (Join-Path $UnpinnedCacheRoot 'bun-windows-x64.zip'),
        $true
    )
    $RetryDefinition = New-ProjBunTestDefinition `
        -ArchivePath $ArchivePath `
        -Sha256 (Get-ProjDevFileSha256 -Path $ArchivePath)
    $RetryDefinition.ProjectSha256 = ''
    $RetryDefinition.Sha256 = ''
    $RetryDefinition.Verification = 'unverified'
    Assert-ProjBunTest `
        -Condition (Install-ProjDevBun `
            -Context $UnpinnedContext `
            -Definition $RetryDefinition) `
        -Message 'staged payload failure did not reset and retry the cache'
    Assert-ProjBunTest `
        -Condition (Test-ProjDevInstalled `
            -Context $UnpinnedContext `
            -Definition $RetryDefinition) `
        -Message 'clean artifact retry did not produce a valid installation'

    $BadDataRoot = Join-Path $TemporaryRoot 'bad-data'
    $BadContext = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $BadDataRoot `
        -CacheDataRoot $CacheDataRoot
    $BadDefinition = New-ProjBunTestDefinition `
        -ArchivePath $ArchivePath `
        -Sha256 ('0' * 64)
    Assert-ProjBunThrows `
        -Action {
            Install-ProjDevBun `
                -Context $BadContext `
                -Definition $BadDefinition
        } `
        -Pattern '*SHA-256 verification failed*'
    Assert-ProjBunTest `
        -Condition (-not [IO.Directory]::Exists(
            (Get-ProjDevInstallRoot `
                -Context $BadContext `
                -Definition $BadDefinition)
        )) `
        -Message 'wrong-checksum artifact created an installation'

    [IO.File]::AppendAllText((Join-Path $InstallRoot 'bun.exe'), 'damage')
    Assert-ProjBunTest `
        -Condition (-not (Test-ProjDevInstalled `
            -Context $Context `
            -Definition $Definition)) `
        -Message 'installed-file corruption was not detected'
    [IO.File]::Delete($ArchivePath)
    $PeerContext = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot (Join-Path $TemporaryRoot 'peer project data') `
        -CacheDataRoot $CacheDataRoot
    Assert-ProjBunTest `
        -Condition (Install-ProjDevBun `
            -Context $PeerContext `
            -Definition $Definition) `
        -Message 'a second project did not reuse the shared verified cache'
    Assert-ProjBunTest `
        -Condition ($PeerContext.CacheRoot.Equals(
            $Context.CacheRoot,
            [StringComparison]::OrdinalIgnoreCase
        ) -and
            -not $PeerContext.DataRoot.Equals(
                $Context.DataRoot,
                [StringComparison]::OrdinalIgnoreCase
            )) `
        -Message 'the cache remained scoped to a project DataRoot'
    Assert-ProjBunTest `
        -Condition (Install-ProjDevBun `
            -Context $Context `
            -Definition $Definition) `
        -Message 'damaged install was not repaired from verified cache'
    Assert-ProjBunTest `
        -Condition (Test-ProjDevInstalled `
            -Context $Context `
            -Definition $Definition) `
        -Message 'cache repair did not restore a trusted installation'

    Assert-ProjBunZipTraversalRejected `
        -TemporaryRoot $TemporaryRoot `
        -FixtureRoot $FixtureRoot

    $ActionRoot = Join-Path $ProjectRoot '.swaw'
    [void][IO.Directory]::CreateDirectory($ActionRoot)
    $EntryFile = Join-Path $ProjectRoot "$SetupEntryName.cmd"
    [IO.File]::WriteAllText($EntryFile, '@echo off')
    Set-ProjBunProcessEnvironment -Values @{
        SWAWKIT_PROJ_PROTOCOL = '1'
        SWAWKIT_HOME = $ControlHome
        SWAWKIT_PROJ_TARGET_PROJECT_ROOT = $ProjectRoot
        SWAWKIT_PROJ_ACTION_ROOT = $ActionRoot
        SWAWKIT_PROJ_DATA_ROOT = $null
        SWAWKIT_PROJ_ENTRY_COMMAND = 'swawkit'
        SWAWKIT_PROJ_ENTRY_FILE = $EntryFile
        SWAWKIT_PROJ_INVOCATION_DIR = $InvocationRoot
        SWAWKIT_PROJ_BUN_MODE = 'disabled'
        SWAWKIT_PROJ_BUN_VERSION = '1.2.15'
    }
    $SetupDataRoot = Join-Path $TemporaryRoot 'setup entry data'
    $env:SWAWKIT_PROJ_DATA_ROOT = $SetupDataRoot
    $SetupEntry = Join-Path $ProjRoot '.dev\setup\run.ps1'
    $SetupResult = Invoke-ProjBunEntryFixture `
        -PowerShell $SystemPowerShell `
        -EntryPath $SetupEntry `
        -Arguments @()
    Assert-ProjBunTest `
        -Condition ($SetupResult.ExitCode -eq 0 -and
            [IO.File]::Exists((Join-Path $SetupDataRoot 'modules\kernel\.dev\setup\export\env.cmd')) -and
            [IO.File]::Exists((Join-Path $SetupDataRoot 'modules\kernel\.dev\setup\export\env.ps1')) -and
            [IO.File]::Exists((Join-Path $SetupDataRoot 'modules\kernel\.dev\setup\export\_state.json')) -and
            -not [IO.Directory]::Exists(
                (Join-Path $SetupDataRoot 'modules\kernel\.dev\setup\export\bun')
            )) `
        -Message "real disabled .dev.setup entry failed: $($SetupResult.Output)"
    $SetupEnvHash = Get-ProjDevFileSha256 `
        -Path (Join-Path $SetupDataRoot 'modules\kernel\.dev\setup\export\env.ps1')
    $RejectedSetup = Invoke-ProjBunEntryFixture `
        -PowerShell $SystemPowerShell `
        -EntryPath $SetupEntry `
        -Arguments @('unexpected')
    Assert-ProjBunTest `
        -Condition ($RejectedSetup.ExitCode -eq 1 -and
            (Get-ProjDevFileSha256 `
                -Path (Join-Path $SetupDataRoot 'modules\kernel\.dev\setup\export\env.ps1')
            ) -ceq $SetupEnvHash) `
        -Message '.dev.setup accepted arguments or changed state after rejection'

    $PendingDataRoot = Join-Path $TemporaryRoot 'pending setup data'
    $env:SWAWKIT_PROJ_DATA_ROOT = $PendingDataRoot
    $env:SWAWKIT_PROJ_GO_MODE = 'managed'
    $env:SWAWKIT_PROJ_GO_VERSION = '1.22.4'
    $env:SWAWKIT_PROJ_PYTHON_MODE = 'uv'
    $env:SWAWKIT_PROJ_PYTHON_VERSION = '3.13'
    $env:SWAWKIT_PROJ_UV_MODE = 'managed'
    $env:SWAWKIT_PROJ_UV_VERSION = '0.10.2'
    $PendingSetup = Invoke-ProjBunEntryFixture `
        -PowerShell $SystemPowerShell `
        -EntryPath $SetupEntry `
        -Arguments @()
    Assert-ProjBunTest `
        -Condition ($PendingSetup.ExitCode -eq 1 -and
            $PendingSetup.Output.Contains(
                '.dev.setup does not yet handle these enabled declarations: go, python, uv.'
            ) -and
            -not [IO.Directory]::Exists($PendingDataRoot) -and
            -not [IO.Directory]::Exists(
                (Join-Path $ProjectRoot 'data\proj_cache')
            )) `
        -Message 'an unsupported enabled module did not fail before side effects'
    foreach ($Name in @(
        'SWAWKIT_PROJ_GO_MODE',
        'SWAWKIT_PROJ_GO_VERSION',
        'SWAWKIT_PROJ_PYTHON_MODE',
        'SWAWKIT_PROJ_PYTHON_VERSION',
        'SWAWKIT_PROJ_UV_MODE',
        'SWAWKIT_PROJ_UV_VERSION'
    )) {
        [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
    }

    Assert-ProjBunTest `
        -Condition (
            [Environment]::GetEnvironmentVariable('PATH', 'User') -ceq
                $UserPathBefore -and
            [Environment]::GetEnvironmentVariable('PATH', 'Machine') -ceq
                $MachinePathBefore
        ) `
        -Message 'setup changed persistent User or Machine PATH'

    Write-Host '[PASS] Proj Bun installation test' -ForegroundColor Green
} finally {
    Exit-ProjBunIsolatedEnvironment -Snapshot $EnvironmentSnapshot
    $ResolvedTemporaryRoot = [IO.Path]::GetFullPath($TemporaryRoot)
    $SystemTemporaryRoot = [IO.Path]::GetFullPath(
        $TestTemporaryBase
    ).TrimEnd('\') + '\'
    if ($ResolvedTemporaryRoot.StartsWith(
        $SystemTemporaryRoot,
        [StringComparison]::OrdinalIgnoreCase
    ) -and
        [IO.Path]::GetFileName($ResolvedTemporaryRoot).StartsWith(
            'swawkit-proj-bun-',
            [StringComparison]::Ordinal
        ) -and
        [IO.Directory]::Exists($ResolvedTemporaryRoot)) {
        Remove-Item -LiteralPath $ResolvedTemporaryRoot -Recurse -Force
    }
}
