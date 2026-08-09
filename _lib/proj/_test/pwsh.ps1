[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\setup.ps1')

function Assert-ProjPwshTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "PowerShell test failed: $Message"
    }
}

function Assert-ProjPwshThrows {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [Parameter(Mandatory = $true)][string]$Pattern
    )

    $Message = ''
    try {
        & $Action
    } catch {
        $Message = $_.Exception.Message
    }
    Assert-ProjPwshTest `
        -Condition ($Message -like $Pattern) `
        -Message "expected error '$Pattern', received '$Message'"
}

function New-ProjPwshReleaseFixture {
    param(
        [Parameter(Mandatory = $true)][string]$Version,
        [Parameter(Mandatory = $true)][string]$Sha256
    )

    $Tag = "v$Version"
    $Asset = "PowerShell-$Version-win-x64.zip"
    return [pscustomobject][ordered]@{
        tag_name = $Tag
        assets = @(
            [pscustomobject][ordered]@{
                name = $Asset
                browser_download_url = (
                    'https://github.com/PowerShell/PowerShell/releases/' +
                    "download/$Tag/$Asset"
                )
                digest = "sha256:$Sha256"
            }
        )
    }
}

function New-ProjPwshFixtureExecutable {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Version
    )

    $ClassName = "ProjPwshFixture$([Guid]::NewGuid().ToString('N'))"
    $Source = @"
using System;

public static class $ClassName
{
    public static int Main(string[] args)
    {
        if (args.Length == 1 && args[0] == "-Version")
        {
            Console.WriteLine("PowerShell $Version");
            return 0;
        }
        return 2;
    }
}
"@
    Add-Type `
        -TypeDefinition $Source `
        -Language CSharp `
        -OutputAssembly $Path `
        -OutputType ConsoleApplication
}

$EnvironmentNames = @(
    'SWAWKIT_PROJ_PWSH_MODE',
    'SWAWKIT_PROJ_PWSH_VERSION',
    'SWAWKIT_PROJ_PWSH_SHA256'
)
$EnvironmentSnapshot = @{}
foreach ($Name in $EnvironmentNames) {
    $EnvironmentSnapshot[$Name] = [Environment]::GetEnvironmentVariable(
        $Name,
        [EnvironmentVariableTarget]::Process
    )
    [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
}

$TestTemporaryBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TestTemporaryBase)
$TemporaryRoot = Join-Path $TestTemporaryBase (
    "swawkit-proj-pwsh-$([Guid]::NewGuid().ToString('N'))"
)

try {
    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    $DataRoot = Join-Path $TemporaryRoot 'data'
    $CacheDataRoot = Join-Path $TemporaryRoot 'cache'
    $FixtureRoot = Join-Path $TemporaryRoot 'fixture'
    $ArchiveRoot = Join-Path $FixtureRoot 'archive'
    foreach ($Directory in @($ProjectRoot, $ArchiveRoot)) {
        [void][IO.Directory]::CreateDirectory($Directory)
    }
    $Context = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $DataRoot `
        -CacheDataRoot $CacheDataRoot `
        -EntryCommand 'swawkit'

    $env:SWAWKIT_PROJ_PWSH_MODE = 'managed'
    $env:SWAWKIT_PROJ_PWSH_VERSION = '7.6.4'
    $env:SWAWKIT_PROJ_PWSH_SHA256 = ''
    $Exact = Get-ProjDevPwshDefinition
    Assert-ProjPwshTest `
        -Condition (
            $Exact.Url -ceq (
                'https://github.com/PowerShell/PowerShell/releases/' +
                'download/v7.6.4/PowerShell-7.6.4-win-x64.zip'
            ) -and
            $Exact.SourceIdentity -ceq (
                'github:PowerShell/PowerShell@v7.6.4#' +
                'PowerShell-7.6.4-win-x64.zip'
            )
        ) `
        -Message 'exact version did not produce stable release coordinates'

    $Digest = 'a' * 64
    [void](Resolve-ProjDevPwshRelease `
        -Definition $Exact `
        -Release (New-ProjPwshReleaseFixture `
            -Version '7.6.4' `
            -Sha256 $Digest))
    Assert-ProjPwshTest `
        -Condition (
            $Exact.Sha256 -ceq $Digest -and
            $Exact.Verification -ceq 'github'
        ) `
        -Message 'GitHub release digest was not selected'

    $env:SWAWKIT_PROJ_PWSH_SHA256 = 'b' * 64
    Assert-ProjPwshThrows `
        -Action {
            [void](Resolve-ProjDevPwshRelease `
                -Definition (Get-ProjDevPwshDefinition) `
                -Release (New-ProjPwshReleaseFixture `
                    -Version '7.6.4' `
                    -Sha256 $Digest))
        } `
        -Pattern '*does not match the GitHub Release digest*'
    $env:SWAWKIT_PROJ_PWSH_SHA256 = ''

    $BadDigest = New-ProjPwshReleaseFixture `
        -Version '7.6.4' `
        -Sha256 $Digest
    $BadDigest.assets[0].digest = 'sha256:not-a-digest'
    Assert-ProjPwshThrows `
        -Action {
            [void](Resolve-ProjDevPwshRelease `
                -Definition (Get-ProjDevPwshDefinition) `
                -Release $BadDigest)
        } `
        -Pattern '*GitHub returned an invalid digest for PowerShell*'

    $env:SWAWKIT_PROJ_PWSH_VERSION = 'latest'
    $Latest = Resolve-ProjDevPwshDefinitionForSetup `
        -Context $Context `
        -Definition (Get-ProjDevPwshDefinition) `
        -LatestRelease (New-ProjPwshReleaseFixture `
            -Version '7.6.4' `
            -Sha256 $Digest)
    Assert-ProjPwshTest `
        -Condition (
            $Latest.Version -ceq '7.6.4' -and
            $Latest.SelectionStatus -ceq 'pending'
        ) `
        -Message 'latest was not resolved to an exact release'
    Write-ProjDevPwshSelection -Context $Context -Definition $Latest
    $Selected = Resolve-ProjDevPwshDefinitionForSetup `
        -Context $Context `
        -Definition (Get-ProjDevPwshDefinition) `
        -LatestRelease (New-ProjPwshReleaseFixture `
            -Version '7.6.5' `
            -Sha256 ('b' * 64))
    Assert-ProjPwshTest `
        -Condition (
            $Selected.Version -ceq '7.6.4' -and
            $Selected.SelectionStatus -ceq 'loaded'
        ) `
        -Message 'repeated setup implicitly advanced latest'

    $env:SWAWKIT_PROJ_PWSH_SHA256 = 'c' * 64
    Assert-ProjPwshThrows `
        -Action { [void](Get-ProjDevPwshDefinition) } `
        -Pattern '*latest cannot be combined*'
    $env:SWAWKIT_PROJ_PWSH_SHA256 = ''

    $FixtureExecutable = Join-Path $ArchiveRoot 'pwsh.exe'
    New-ProjPwshFixtureExecutable `
        -Path $FixtureExecutable `
        -Version '7.6.4'
    $ArchivePath = Join-Path $FixtureRoot 'PowerShell-7.6.4-win-x64.zip'
    [IO.Compression.ZipFile]::CreateFromDirectory($ArchiveRoot, $ArchivePath)
    $InstallDefinition = Get-ProjDevPwshDefinition
    [void](Set-ProjDevPwshResolvedVersion `
        -Definition $InstallDefinition `
        -Version '7.6.4')
    $InstallDefinition.Url = $ArchivePath
    $InstallDefinition.SourceIdentity = "fixture:$ArchivePath"
    $InstallDefinition.ProjectSha256 = Get-ProjDevFileSha256 -Path $ArchivePath
    $InstallDefinition.Sha256 = $InstallDefinition.ProjectSha256
    $InstallDefinition.Verification = 'project'
    $InstallDefinition.Release = @{ Provider = 'fixture' }
    $InstallDefinition.ReleaseResolved = $true
    $InstallDefinition.SelectionStatus = 'none'

    Assert-ProjPwshTest `
        -Condition (Install-ProjDevPwsh `
            -Context $Context `
            -Definition $InstallDefinition) `
        -Message 'first fixture installation was skipped'
    Assert-ProjPwshTest `
        -Condition (Test-ProjDevInstalled `
            -Context $Context `
            -Definition $InstallDefinition) `
        -Message 'installed fixture was not recognized'
    Assert-ProjPwshTest `
        -Condition (-not (Install-ProjDevPwsh `
            -Context $Context `
            -Definition $InstallDefinition)) `
        -Message 'valid fixture installation was needlessly replaced'

    $Plan = New-ProjDevEnvironmentPlan
    Add-ProjDevPwshEnvironment `
        -Context $Context `
        -Definition $InstallDefinition `
        -Plan $Plan
    Assert-ProjPwshTest `
        -Condition (
            $Plan.Variables.Count -eq 0 -and
            $Plan.PathPrefixes.Count -eq 1 -and
            [string]$Plan.PathPrefixes[0] -ceq
                (Get-ProjDevInstallRoot `
                    -Context $Context `
                    -Definition $InstallDefinition)
        ) `
        -Message 'managed PowerShell was not added to the environment plan'

    Write-Host '[PASS] Proj PowerShell module test' -ForegroundColor Green
} finally {
    foreach ($Name in $EnvironmentNames) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            $EnvironmentSnapshot[$Name],
            [EnvironmentVariableTarget]::Process
        )
    }
    $ResolvedTemporaryRoot = [IO.Path]::GetFullPath($TemporaryRoot)
    $AllowedRoot = $TestTemporaryBase.TrimEnd('\') + '\'
    if ($ResolvedTemporaryRoot.StartsWith(
        $AllowedRoot,
        [StringComparison]::OrdinalIgnoreCase
    ) -and
        [IO.Path]::GetFileName($ResolvedTemporaryRoot).StartsWith(
            'swawkit-proj-pwsh-',
            [StringComparison]::Ordinal
        ) -and
        [IO.Directory]::Exists($ResolvedTemporaryRoot)) {
        Remove-Item -LiteralPath $ResolvedTemporaryRoot -Recurse -Force
    }
}

$global:LASTEXITCODE = 0
