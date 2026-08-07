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

function New-ProjBunReleaseFixture {
    param(
        [Parameter(Mandatory = $true)][string]$Tag,
        [Parameter(Mandatory = $true)][string]$Asset,
        [AllowEmptyString()][string]$Digest
    )

    return [pscustomobject][ordered]@{
        tag_name = $Tag
        assets = @(
            [pscustomobject][ordered]@{
                name = $Asset
                browser_download_url = (
                    "https://github.com/oven-sh/bun/releases/download/" +
                    "$Tag/$Asset"
                )
                digest = $Digest
            }
        )
    }
}

try {
    $Digest = 'a' * 64
    Set-ProjBunProcessEnvironment -Values @{
        SWAWKIT_PROJ_BUN_MODE = 'managed'
        SWAWKIT_PROJ_BUN_VERSION = '1.3.14'
        SWAWKIT_PROJ_BUN_SHA256 = ''
    }
    $Definition = Get-ProjDevBunDefinition
    Assert-ProjBunTest `
        -Condition (
            $Definition.SourceIdentity -ceq
                'github:oven-sh/bun@bun-v1.3.14#bun-windows-x64.zip' -and
            [string]::IsNullOrWhiteSpace($Definition.Sha256)
        ) `
        -Message 'arbitrary Bun version was not accepted as a release declaration'
    [void](Resolve-ProjDevBunRelease `
        -Definition $Definition `
        -Release (New-ProjBunReleaseFixture `
            -Tag 'bun-v1.3.14' `
            -Asset 'bun-windows-x64.zip' `
            -Digest "sha256:$Digest"))
    Assert-ProjBunTest `
        -Condition (
            $Definition.Sha256 -ceq $Digest -and
            $Definition.Verification -ceq 'github' -and
            $Definition.Url -ceq (
                'https://github.com/oven-sh/bun/releases/download/' +
                'bun-v1.3.14/bun-windows-x64.zip'
            )
        ) `
        -Message 'GitHub release digest and asset URL were not resolved'

    $env:SWAWKIT_PROJ_BUN_SHA256 = "sha256:$Digest"
    $PinnedDefinition = Get-ProjDevBunDefinition
    [void](Resolve-ProjDevBunRelease `
        -Definition $PinnedDefinition `
        -Release (New-ProjBunReleaseFixture `
            -Tag 'bun-v1.3.14' `
            -Asset 'bun-windows-x64.zip' `
            -Digest "sha256:$Digest"))
    Assert-ProjBunTest `
        -Condition (
            $PinnedDefinition.ProjectSha256 -ceq $Digest -and
            $PinnedDefinition.Verification -ceq 'project'
        ) `
        -Message 'optional project SHA-256 did not take precedence'

    $env:SWAWKIT_PROJ_BUN_SHA256 = 'b' * 64
    Assert-ProjBunThrows `
        -Action {
            $MismatchDefinition = Get-ProjDevBunDefinition
            [void](Resolve-ProjDevBunRelease `
                -Definition $MismatchDefinition `
                -Release (New-ProjBunReleaseFixture `
                    -Tag 'bun-v1.3.14' `
                    -Asset 'bun-windows-x64.zip' `
                    -Digest "sha256:$Digest"))
        } `
        -Pattern '*does not match the GitHub Release digest*'

    $env:SWAWKIT_PROJ_BUN_SHA256 = ''
    $UnverifiedDefinition = Get-ProjDevBunDefinition
    [void](Resolve-ProjDevBunRelease `
        -Definition $UnverifiedDefinition `
        -Release (New-ProjBunReleaseFixture `
            -Tag 'bun-v1.3.14' `
            -Asset 'bun-windows-x64.zip' `
            -Digest ''))
    Assert-ProjBunTest `
        -Condition (
            $UnverifiedDefinition.Verification -ceq 'unverified' -and
            [string]::IsNullOrWhiteSpace($UnverifiedDefinition.Sha256)
        ) `
        -Message 'release without digest was blocked or misclassified'

    Assert-ProjBunThrows `
        -Action {
            $MissingAssetDefinition = Get-ProjDevBunDefinition
            [void](Resolve-ProjDevBunRelease `
                -Definition $MissingAssetDefinition `
                -Release (New-ProjBunReleaseFixture `
                    -Tag 'bun-v1.3.14' `
                    -Asset 'bun-linux-x64.zip' `
                    -Digest "sha256:$Digest"))
        } `
        -Pattern "*must contain exactly one 'bun-windows-x64.zip' asset*"

    Assert-ProjBunThrows `
        -Action {
            $BadDigestDefinition = Get-ProjDevBunDefinition
            [void](Resolve-ProjDevBunRelease `
                -Definition $BadDigestDefinition `
                -Release (New-ProjBunReleaseFixture `
                    -Tag 'bun-v1.3.14' `
                    -Asset 'bun-windows-x64.zip' `
                    -Digest 'sha256:not-a-digest'))
        } `
        -Pattern '*GitHub returned an invalid digest for Bun*'

    Write-Host '[PASS] Proj Bun GitHub release resolver test' `
        -ForegroundColor Green
} finally {
    Exit-ProjBunIsolatedEnvironment -Snapshot $EnvironmentSnapshot
}
