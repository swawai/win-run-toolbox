[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjCommandExportTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "Command export test failed: $Message"
    }
}

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\_lib\runtime.ps1')
. (Join-Path $ProjRoot '_toolchain\_lib\environment.ps1')

$TemporaryBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TemporaryBase)
$TemporaryRoot = Join-Path $TemporaryBase (
    "swawkit-command-export-$([Guid]::NewGuid().ToString('N'))"
)
$ModulesJunction = ''

try {
    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    $DataRoot = Join-Path $TemporaryRoot 'data'
    $CacheRoot = Join-Path $TemporaryRoot 'cache'
    [void][IO.Directory]::CreateDirectory($ProjectRoot)
    [void][IO.Directory]::CreateDirectory($DataRoot)

    $Context = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $DataRoot `
        -CacheDataRoot $CacheRoot `
        -EntryCommand 'fixture'
    $ExpectedExport = Join-Path $DataRoot (
        'modules\kernel\.dev\setup\export'
    )
    Assert-ProjCommandExportTest `
        -Condition (
            (Get-ProjDevCanonicalPath -Path $Context.EnvironmentRoot) -ceq
            (Get-ProjDevCanonicalPath -Path $ExpectedExport)
        ) `
        -Message 'the .dev.setup export path is not canonical'
    Assert-ProjCommandExportTest `
        -Condition (-not [IO.Directory]::Exists($ExpectedExport)) `
        -Message 'resolving an export created its directory'

    $MissingMessage = ''
    try {
        [void](Get-ProjRequiredCommandExport `
            -DataRoot $DataRoot `
            -ProviderAddress '.dev.setup' `
            -EntryCommand 'fixture')
    } catch {
        $MissingMessage = $_.Exception.Message
    }
    Assert-ProjCommandExportTest `
        -Condition (
            $MissingMessage -like "*'.dev.setup'*" -and
            $MissingMessage -like "*'fixture .dev.setup'*"
        ) `
        -Message 'a missing export did not identify its provider command'

    foreach ($Address in @(
        '',
        '.Dev.setup',
        '.dev..setup',
        '.dev/setup',
        '.dev\setup',
        '..entry',
        'build'
    )) {
        $Rejected = $false
        try {
            [void](Resolve-ProjCommandExportPath `
                -DataRoot $DataRoot `
                -ProviderAddress $Address)
        } catch {
            $Rejected = $true
        }
        Assert-ProjCommandExportTest `
            -Condition $Rejected `
            -Message "unsafe or unsupported provider address was accepted: '$Address'"
    }

    $ExternalRoot = Join-Path $TemporaryRoot 'external'
    $ReparseDataRoot = Join-Path $TemporaryRoot 'reparse-data'
    [void][IO.Directory]::CreateDirectory($ExternalRoot)
    [void][IO.Directory]::CreateDirectory($ReparseDataRoot)
    $ExternalSentinel = Join-Path $ExternalRoot 'sentinel.txt'
    [IO.File]::WriteAllText($ExternalSentinel, 'outside')
    $ModulesJunction = Join-Path $ReparseDataRoot 'modules'
    [void](New-Item `
        -ItemType Junction `
        -Path $ModulesJunction `
        -Target $ExternalRoot)
    try {
        $ReparseRejected = $false
        try {
            [void](Resolve-ProjCommandExportPath `
                -DataRoot $ReparseDataRoot `
                -ProviderAddress '.dev.setup')
        } catch {
            $ReparseRejected = $_.Exception.Message -like '*reparse point*'
        }
        Assert-ProjCommandExportTest `
            -Condition $ReparseRejected `
            -Message 'an intermediate command-data junction was accepted'
        Assert-ProjCommandExportTest `
            -Condition (
                [IO.File]::ReadAllText($ExternalSentinel) -ceq 'outside'
            ) `
            -Message 'an external junction target was modified'
    } finally {
        if ([IO.Directory]::Exists($ModulesJunction)) {
            [IO.Directory]::Delete($ModulesJunction)
        }
        $ModulesJunction = ''
    }

    [void][IO.Directory]::CreateDirectory($ExpectedExport)
    Assert-ProjCommandExportTest `
        -Condition (
            (Get-ProjDevCanonicalPath -Path (
                Get-ProjRequiredCommandExport `
                    -DataRoot $DataRoot `
                    -ProviderAddress '.dev.setup' `
                    -EntryCommand 'fixture'
            )) -ceq (Get-ProjDevCanonicalPath -Path $ExpectedExport)
        ) `
        -Message 'an existing export was not resolved'
    $IncompletePath = Join-Path $ExpectedExport 'env.cmd'
    [IO.File]::WriteAllText($IncompletePath, '@rem incomplete')
    $IncompleteMessage = ''
    try {
        [void](Get-ProjDevelopmentEnvironmentGeneration -Context $Context)
    } catch {
        $IncompleteMessage = $_.Exception.Message
    }
    Assert-ProjCommandExportTest `
        -Condition (
            $IncompleteMessage -like '*incomplete*' -and
            $IncompleteMessage -like "*'fixture .dev.setup'*"
        ) `
        -Message 'an incomplete export was treated as ready'
    [IO.File]::Delete($IncompletePath)

    $Plan = New-ProjDevEnvironmentPlan -Context $Context
    $Scripts = ConvertTo-ProjDevEnvironmentScripts -Plan $Plan
    [void](Publish-ProjDevEnvironmentScripts `
        -Context $Context `
        -Scripts $Scripts)
    [void](Publish-ProjDevEnvironmentState `
        -Context $Context `
        -GenerationId ([string]$Scripts.GenerationId))
    Assert-ProjCommandExportTest `
        -Condition (
            (Get-ProjDevelopmentEnvironmentGeneration -Context $Context) -ceq
            [string]$Scripts.GenerationId
        ) `
        -Message 'a complete export was not recognized'

    $SentinelPath = Join-Path $ExpectedExport 'tool-sentinel.bin'
    [IO.File]::WriteAllBytes($SentinelPath, [byte[]](1, 2, 3, 4))
    $StatePath = Join-Path $ExpectedExport '_state.json'
    $LegacyState = [IO.File]::ReadAllText(
        $StatePath,
        [Text.Encoding]::UTF8
    ) | ConvertFrom-Json
    $LegacyState.schema = 'swawkit.proj-dev.environment-state.v0'
    [IO.File]::WriteAllText(
        $StatePath,
        ($LegacyState | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    $LegacyMessage = ''
    try {
        [void](Get-ProjDevelopmentEnvironmentGeneration -Context $Context)
    } catch {
        $LegacyMessage = $_.Exception.Message
    }
    Assert-ProjCommandExportTest `
        -Condition (
            $LegacyMessage -like '*state is invalid*' -and
            $LegacyMessage -like "*'fixture .dev.setup'*" -and
            [IO.File]::ReadAllBytes($SentinelPath).Length -eq 4
        ) `
        -Message 'a v0 state did not fail closed without deleting module data'
    [void](Publish-ProjDevEnvironmentState `
        -Context $Context `
        -GenerationId ([string]$Scripts.GenerationId))
    Assert-ProjCommandExportTest `
        -Condition (
            (Get-ProjDevelopmentEnvironmentGeneration -Context $Context) -ceq
            [string]$Scripts.GenerationId -and
            [IO.File]::ReadAllBytes($SentinelPath).Length -eq 4
        ) `
        -Message 'v1 republishing did not safely upgrade the export state'

    $MovedDataRoot = Join-Path $TemporaryRoot 'moved-data'
    [IO.Directory]::Move($DataRoot, $MovedDataRoot)
    $MovedContext = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $MovedDataRoot `
        -CacheDataRoot $CacheRoot `
        -EntryCommand 'fixture'
    $MovedMessage = ''
    try {
        [void](Get-ProjDevelopmentEnvironmentGeneration `
            -Context $MovedContext)
    } catch {
        $MovedMessage = $_.Exception.Message
    }
    Assert-ProjCommandExportTest `
        -Condition (
            $MovedMessage -like '*another project or data root*' -and
            $MovedMessage -like "*'fixture .dev.setup'*"
        ) `
        -Message 'a moved export was not rejected before activation'
    Assert-ProjCommandExportTest `
        -Condition (
            [IO.File]::ReadAllBytes(
                (Join-Path $MovedContext.EnvironmentRoot 'tool-sentinel.bin')
            ).Length -eq 4
        ) `
        -Message 'stale detection modified opaque module data'

    $MovedPlan = New-ProjDevEnvironmentPlan -Context $MovedContext
    $MovedScripts = ConvertTo-ProjDevEnvironmentScripts -Plan $MovedPlan
    [void](Publish-ProjDevEnvironmentScripts `
        -Context $MovedContext `
        -Scripts $MovedScripts)
    [void](Publish-ProjDevEnvironmentState `
        -Context $MovedContext `
        -GenerationId ([string]$MovedScripts.GenerationId))
    Assert-ProjCommandExportTest `
        -Condition (
            (Get-ProjDevelopmentEnvironmentGeneration `
                -Context $MovedContext) -ceq [string]$MovedScripts.GenerationId
        ) `
        -Message 'republishing did not recover a moved export'

    $OtherProjectRoot = Join-Path $TemporaryRoot 'other-project'
    [void][IO.Directory]::CreateDirectory($OtherProjectRoot)
    $OtherProjectContext = New-ProjDevContext `
        -ProjectRoot $OtherProjectRoot `
        -DataRoot $MovedDataRoot `
        -CacheDataRoot $CacheRoot `
        -EntryCommand 'fixture'
    $OtherProjectMessage = ''
    try {
        [void](Get-ProjDevelopmentEnvironmentGeneration `
            -Context $OtherProjectContext)
    } catch {
        $OtherProjectMessage = $_.Exception.Message
    }
    Assert-ProjCommandExportTest `
        -Condition (
            $OtherProjectMessage -like '*another project or data root*' -and
            $OtherProjectMessage -like "*'fixture .dev.setup'*"
        ) `
        -Message 'a project-root change did not invalidate the export'

    Write-Host '[PASS] Proj command export test' -ForegroundColor Green
} finally {
    if (-not [string]::IsNullOrWhiteSpace($ModulesJunction) -and
        [IO.Directory]::Exists($ModulesJunction)) {
        [IO.Directory]::Delete($ModulesJunction)
    }
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        Remove-Item -LiteralPath $TemporaryRoot -Recurse -Force
    }
}

$global:LASTEXITCODE = 0
