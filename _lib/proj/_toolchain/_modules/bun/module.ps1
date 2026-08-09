Set-StrictMode -Version 2.0

# Bun declaration and environment contract shared by setup and .dev.bun.
$script:ProjDevBunManifestPath = Join-Path $PSScriptRoot 'module.psd1'

function Assert-ProjDevBunDictionaryKeys {
    param(
        [Parameter(Mandatory = $true)][Collections.IDictionary]$Dictionary,
        [Parameter(Mandatory = $true)][string[]]$Expected,
        [Parameter(Mandatory = $true)][string]$Description
    )

    $Actual = [string[]]@($Dictionary.Keys | ForEach-Object {
        [string]$_
    })
    foreach ($Name in $Expected) {
        if ($Actual -cnotcontains $Name) {
            throw "$Description is missing '$Name'."
        }
    }
    foreach ($Name in $Actual) {
        if ($Expected -cnotcontains $Name) {
            throw "$Description contains unknown field '$Name'."
        }
    }
}

function Assert-ProjDevBunManifest {
    param([Parameter(Mandatory = $true)][Collections.IDictionary]$Manifest)

    Assert-ProjDevBunDictionaryKeys `
        -Dictionary $Manifest `
        -Expected @(
            'Schema',
            'Name',
            'ModeVariable',
            'SetupImplemented',
            'VersionVariable',
            'HashVariable',
            'InstallMode',
            'RecipeVersion',
            'Executable',
            'RequiredPaths',
            'Release'
        ) `
        -Description 'Bun module manifest'
    if ([string]$Manifest.Schema -cne 'swawkit.proj-dev.module.v0' -or
        [string]$Manifest.Name -cne 'bun' -or
        [string]$Manifest.ModeVariable -cne 'SWAWKIT_PROJ_BUN_MODE' -or
        $Manifest.SetupImplemented -isnot [bool] -or
        -not [bool]$Manifest.SetupImplemented -or
        [string]$Manifest.VersionVariable -cne 'SWAWKIT_PROJ_BUN_VERSION' -or
        [string]$Manifest.HashVariable -cne 'SWAWKIT_PROJ_BUN_SHA256' -or
        [string]$Manifest.InstallMode -cne 'managed') {
        throw 'The Bun module manifest identity is invalid.'
    }
    if ($Manifest.Release -isnot [Collections.IDictionary]) {
        throw 'The Bun module manifest must declare its release source.'
    }
    Assert-ProjDevBunDictionaryKeys `
        -Dictionary $Manifest.Release `
        -Expected @(
            'Provider',
            'Repository',
            'ApiVersion',
            'TagTemplate',
            'Asset',
            'ArchiveSubdir'
        ) `
        -Description 'Bun release declaration'
    if ([string]$Manifest.Release.Provider -cne 'github' -or
        [string]$Manifest.Release.Repository -cne 'oven-sh/bun' -or
        [string]$Manifest.Release.ApiVersion -cnotmatch '^\d{4}-\d{2}-\d{2}$' -or
        [string]$Manifest.Release.TagTemplate -cne 'bun-v{version}' -or
        [string]$Manifest.Release.Asset -cne 'bun-windows-x64.zip' -or
        [string]$Manifest.Release.ArchiveSubdir -cne 'bun-windows-x64') {
        throw 'The Bun release declaration is invalid.'
    }
}

function Get-ProjDevBunManifest {
    $Manifest = Import-PowerShellDataFile `
        -LiteralPath $script:ProjDevBunManifestPath
    Assert-ProjDevBunManifest -Manifest $Manifest
    return $Manifest
}

function Get-ProjDevBunReleaseCoordinates {
    param(
        [Parameter(Mandatory = $true)][Collections.IDictionary]$Manifest,
        [Parameter(Mandatory = $true)][string]$Version
    )

    $Tag = ([string]$Manifest.Release.TagTemplate).Replace(
        '{version}',
        $Version
    )
    return [pscustomobject][ordered]@{
        Tag = $Tag
        Url = 'https://github.com/{0}/releases/download/{1}/{2}' -f
            [string]$Manifest.Release.Repository,
            $Tag,
            [string]$Manifest.Release.Asset
        SourceIdentity = 'github:{0}@{1}#{2}' -f
            [string]$Manifest.Release.Repository,
            $Tag,
            [string]$Manifest.Release.Asset
    }
}

function Get-ProjDevBunDefinition {
    $Manifest = Get-ProjDevBunManifest
    $Mode = [string][Environment]::GetEnvironmentVariable(
        [string]$Manifest.ModeVariable,
        [EnvironmentVariableTarget]::Process
    )
    $Mode = $Mode.Trim().ToLowerInvariant()
    if ([string]::IsNullOrWhiteSpace($Mode) -or $Mode -ceq 'disabled') {
        return $null
    }
    if ($Mode -cne [string]$Manifest.InstallMode) {
        throw (
            "Unsupported $($Manifest.ModeVariable) value '$Mode'. Expected " +
            "'$($Manifest.InstallMode)' or 'disabled'."
        )
    }

    $Version = [string][Environment]::GetEnvironmentVariable(
        [string]$Manifest.VersionVariable,
        [EnvironmentVariableTarget]::Process
    )
    $Version = $Version.Trim()
    if ([string]::IsNullOrWhiteSpace($Version)) {
        throw "Enabled Bun must declare $($Manifest.VersionVariable)."
    }
    [void](Get-ProjDevSafeSegment `
        -Value $Version `
        -Description 'Bun version')

    $ProjectSha256 = [string][Environment]::GetEnvironmentVariable(
        [string]$Manifest.HashVariable,
        [EnvironmentVariableTarget]::Process
    )
    $ProjectSha256 = $ProjectSha256.Trim().ToLowerInvariant()
    if ($ProjectSha256.StartsWith('sha256:')) {
        $ProjectSha256 = $ProjectSha256.Substring(7)
    }
    if (-not [string]::IsNullOrWhiteSpace($ProjectSha256) -and
        $ProjectSha256 -cnotmatch '^[a-f0-9]{64}$') {
        throw (
            "$($Manifest.HashVariable) must be empty or a 64-character " +
            'SHA-256 value.'
        )
    }
    if ($Version -ceq 'latest' -and
        -not [string]::IsNullOrWhiteSpace($ProjectSha256)) {
        throw (
            'SWAWKIT_PROJ_BUN_VERSION=latest cannot be combined with ' +
            'SWAWKIT_PROJ_BUN_SHA256. Use an exact Bun version when ' +
            'pinning a project SHA-256.'
        )
    }

    $Coordinates = Get-ProjDevBunReleaseCoordinates `
        -Manifest $Manifest `
        -Version $Version

    return [pscustomobject][ordered]@{
        Schema = [string]$Manifest.Schema
        Name = [string]$Manifest.Name
        Mode = $Mode
        RequestedVersion = $Version
        Version = $Version
        Url = [string]$Coordinates.Url
        SourceIdentity = [string]$Coordinates.SourceIdentity
        ProjectSha256 = $ProjectSha256
        Sha256 = $ProjectSha256
        Verification = if ([string]::IsNullOrWhiteSpace($ProjectSha256)) {
            'unresolved'
        } else {
            'project'
        }
        Release = $Manifest.Release
        ArchiveSubdir = [string]$Manifest.Release.ArchiveSubdir
        RecipeVersion = [string]$Manifest.RecipeVersion
        Executable = [string]$Manifest.Executable
        RequiredPaths = [string[]]$Manifest.RequiredPaths
        ReleaseResolved = $false
        SelectionStatus = 'none'
    }
}

function Get-ProjDevBunTrustStatus {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $Metadata = Get-ProjDevValidInstallMetadata `
        -Context $Context `
        -Definition $Definition
    if (-not [string]::IsNullOrWhiteSpace(
        (Get-ProjDevProjectSha256 -Definition $Definition)
    )) {
        return [pscustomobject][ordered]@{
            Level = 'pinned'
            Message = 'project SHA-256'
            Metadata = $Metadata
        }
    }

    $Verification = if ($null -eq $Metadata) {
        [string]$Definition.Verification
    } else {
        [string]$Metadata.sourceVerification
    }
    if ($Verification -ceq 'github') {
        return [pscustomobject][ordered]@{
            Level = 'upstream'
            Message = 'GitHub Release digest'
            Metadata = $Metadata
        }
    }
    if ($null -eq $Metadata) {
        return [pscustomobject][ordered]@{
            Level = 'unpinned'
            Message = 'awaiting GitHub Release resolution'
            Metadata = $null
        }
    }
    return [pscustomobject][ordered]@{
        Level = 'unpinned'
        Message = 'no comparable release SHA-256'
        Metadata = $Metadata
    }
}

function Write-ProjDevBunTrustWarning {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $Trust = Get-ProjDevBunTrustStatus `
        -Context $Context `
        -Definition $Definition
    if ($Trust.Level -ceq 'pinned') {
        return
    }
    if ($Trust.Level -ceq 'upstream') {
        Write-Warning (
            "Bun $($Definition.Version) was verified with the GitHub Release " +
            'digest; SWAWKIT_PROJ_BUN_SHA256 is not pinned by this project.'
        )
        return
    }
    if ($null -eq $Trust.Metadata) {
        Write-Warning (
            "Bun $($Definition.Version) is not pinned by " +
            'SWAWKIT_PROJ_BUN_SHA256; .dev.setup will use the GitHub Release ' +
            'digest when available.'
        )
        return
    }
    Write-Warning (
        "Bun $($Definition.Version) has no comparable GitHub Release digest " +
        'or project SHA-256; installation is allowed but not content-pinned.'
    )
}

function Add-ProjDevBunEnvironment {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Plan
    )

    $InstallRoot = Get-ProjDevInstallRoot `
        -Context $Context `
        -Definition $Definition
    Add-ProjDevEnvironmentPath -Plan $Plan -Path $InstallRoot
}

function Assert-ProjDevBunEnvironmentCurrent {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $InstallRoot = Get-ProjDevInstallRoot `
        -Context $Context `
        -Definition $Definition
    $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
    $PathEntries = @(([string]$env:PATH).Split(
        [IO.Path]::PathSeparator
    ) | Where-Object {
        -not [string]::IsNullOrWhiteSpace([string]$_)
    })
    if ($PathEntries.Count -eq 0) {
        throw "The generated Bun environment has an empty PATH. Run '$Repair'."
    }
    try {
        $FirstPath = Get-ProjDevCanonicalPath -Path ([string]$PathEntries[0])
    } catch {
        throw (
            'The generated Bun environment has an invalid PATH prefix. Run ' +
            "'$Repair'."
        )
    }
    if (-not $FirstPath.Equals(
        (Get-ProjDevCanonicalPath -Path $InstallRoot),
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw (
            'The managed Bun directory is not first in PATH. Run ' +
            "'$Repair'."
        )
    }

    $ExecutableName = [IO.Path]::GetFileName(
        [string]$Definition.Executable
    )
    $Command = Get-Command $ExecutableName `
        -CommandType Application `
        -ErrorAction SilentlyContinue |
        Select-Object -First 1
    $ExpectedExecutable = Resolve-ProjDevChildPath `
        -Root $InstallRoot `
        -RelativePath ([string]$Definition.Executable) `
        -Description 'Bun executable'
    if ($null -eq $Command -or
        -not (Get-ProjDevCanonicalPath -Path $Command.Source).Equals(
            (Get-ProjDevCanonicalPath -Path $ExpectedExecutable),
            [StringComparison]::OrdinalIgnoreCase
        )) {
        throw "The managed Bun executable is not selected. Run '$Repair'."
    }
}
