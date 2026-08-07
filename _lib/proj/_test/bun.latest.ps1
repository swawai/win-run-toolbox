[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\setup.ps1')
. (Join-Path $PSScriptRoot '_lib\bun-fixture.ps1')

$EnvironmentNames = @(
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
    "swawkit-proj-bun-latest-$([Guid]::NewGuid().ToString('N'))"
)

function New-ProjBunLatestReleaseFixture {
    param(
        [Parameter(Mandatory = $true)][string]$Version,
        [Parameter(Mandatory = $true)][string]$Sha256
    )

    $Tag = "bun-v$Version"
    return [pscustomobject][ordered]@{
        tag_name = $Tag
        assets = @(
            [pscustomobject][ordered]@{
                name = 'bun-windows-x64.zip'
                browser_download_url = (
                    'https://github.com/oven-sh/bun/releases/download/' +
                    "$Tag/bun-windows-x64.zip"
                )
                digest = "sha256:$Sha256"
            }
        )
    }
}

try {
    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    $DataRoot = Join-Path $TemporaryRoot 'data'
    $CacheDataRoot = Join-Path $TemporaryRoot 'cache'
    [void][IO.Directory]::CreateDirectory($ProjectRoot)
    $Context = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $DataRoot `
        -CacheDataRoot $CacheDataRoot `
        -EntryCommand 'swawkit'

    Set-ProjBunProcessEnvironment -Values @{
        SWAWKIT_PROJ_BUN_MODE = 'managed'
        SWAWKIT_PROJ_BUN_VERSION = 'latest'
        SWAWKIT_PROJ_BUN_SHA256 = ''
    }
    $Definition = Get-ProjDevBunDefinition
    Assert-ProjBunTest `
        -Condition (
            $Definition.RequestedVersion -ceq 'latest' -and
            $null -eq (Find-ProjDevBunResolvedDefinition `
                -Context $Context `
                -Definition $Definition)
        ) `
        -Message 'a missing latest selection was treated as resolved'

    $FirstDigest = 'a' * 64
    $Resolved = Resolve-ProjDevBunDefinitionForSetup `
        -Context $Context `
        -Definition $Definition `
        -LatestRelease (New-ProjBunLatestReleaseFixture `
            -Version '1.3.14' `
            -Sha256 $FirstDigest)
    Assert-ProjBunTest `
        -Condition (
            $Resolved.Version -ceq '1.3.14' -and
            $Resolved.Sha256 -ceq $FirstDigest -and
            $Resolved.Verification -ceq 'github' -and
            $Resolved.ReleaseResolved -and
            $Resolved.SelectionStatus -ceq 'pending'
        ) `
        -Message 'latest was not resolved to an exact GitHub release'
    Write-ProjDevBunSelection `
        -Context $Context `
        -Definition $Resolved

    $SecondDefinition = Get-ProjDevBunDefinition
    $Selected = Resolve-ProjDevBunDefinitionForSetup `
        -Context $Context `
        -Definition $SecondDefinition `
        -LatestRelease (New-ProjBunLatestReleaseFixture `
            -Version '1.3.15' `
            -Sha256 ('b' * 64))
    Assert-ProjBunTest `
        -Condition (
            $Selected.Version -ceq '1.3.14' -and
            $Selected.Sha256 -ceq $FirstDigest -and
            $Selected.SelectionStatus -ceq 'loaded'
        ) `
        -Message 'repeated setup implicitly advanced the latest selection'

    $env:SWAWKIT_PROJ_BUN_VERSION = '1.3.15'
    $Exact = Get-ProjDevBunDefinition
    Assert-ProjBunTest `
        -Condition (
            (Find-ProjDevBunResolvedDefinition `
                -Context $Context `
                -Definition $Exact).Version -ceq '1.3.15'
        ) `
        -Message 'an exact declaration was overridden by latest state'

    $env:SWAWKIT_PROJ_BUN_VERSION = 'latest'
    $env:SWAWKIT_PROJ_BUN_SHA256 = 'c' * 64
    Assert-ProjBunThrows `
        -Action { [void](Get-ProjDevBunDefinition) } `
        -Pattern '*latest cannot be combined*'
    $env:SWAWKIT_PROJ_BUN_SHA256 = ''

    [IO.File]::WriteAllText(
        (Get-ProjDevBunSelectionPath -Context $Context),
        '{"schema":"wrong"}',
        [Text.UTF8Encoding]::new($false)
    )
    Assert-ProjBunThrows `
        -Action {
            [void](Find-ProjDevBunResolvedDefinition `
                -Context $Context `
                -Definition (Get-ProjDevBunDefinition))
        } `
        -Pattern '*Bun version selection is invalid*'

    Write-Host '[PASS] Proj Bun latest selection test' `
        -ForegroundColor Green
} finally {
    Exit-ProjBunIsolatedEnvironment -Snapshot $EnvironmentSnapshot
    $ResolvedTemporaryRoot = [IO.Path]::GetFullPath($TemporaryRoot)
    $AllowedRoot = $TestTemporaryBase.TrimEnd('\') + '\'
    if ($ResolvedTemporaryRoot.StartsWith(
        $AllowedRoot,
        [StringComparison]::OrdinalIgnoreCase
    ) -and
        [IO.Path]::GetFileName($ResolvedTemporaryRoot).StartsWith(
            'swawkit-proj-bun-latest-',
            [StringComparison]::Ordinal
        ) -and
        [IO.Directory]::Exists($ResolvedTemporaryRoot)) {
        Remove-Item -LiteralPath $ResolvedTemporaryRoot -Recurse -Force
    }
}
