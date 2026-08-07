Set-StrictMode -Version 2.0

$script:ProjDevPwshManifestPath = Join-Path $PSScriptRoot 'module.psd1'

function Assert-ProjDevPwshDictionaryKeys {
    param(
        [Parameter(Mandatory = $true)][Collections.IDictionary]$Dictionary,
        [Parameter(Mandatory = $true)][string[]]$Expected,
        [Parameter(Mandatory = $true)][string]$Description
    )

    $Actual = [string[]]@($Dictionary.Keys | ForEach-Object { [string]$_ })
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

function Assert-ProjDevPwshManifest {
    param([Parameter(Mandatory = $true)][Collections.IDictionary]$Manifest)

    Assert-ProjDevPwshDictionaryKeys `
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
        -Description 'PowerShell module manifest'
    if ([string]$Manifest.Schema -cne 'swawkit.proj-dev.module.v0' -or
        [string]$Manifest.Name -cne 'pwsh' -or
        [string]$Manifest.ModeVariable -cne 'SWAWKIT_PROJ_PWSH_MODE' -or
        $Manifest.SetupImplemented -isnot [bool] -or
        -not [bool]$Manifest.SetupImplemented -or
        [string]$Manifest.VersionVariable -cne 'SWAWKIT_PROJ_PWSH_VERSION' -or
        [string]$Manifest.HashVariable -cne 'SWAWKIT_PROJ_PWSH_SHA256' -or
        [string]$Manifest.InstallMode -cne 'managed' -or
        [string]$Manifest.Executable -cne 'pwsh.exe') {
        throw 'The PowerShell module manifest identity is invalid.'
    }
    if ($Manifest.Release -isnot [Collections.IDictionary]) {
        throw 'The PowerShell module manifest must declare its release source.'
    }
    Assert-ProjDevPwshDictionaryKeys `
        -Dictionary $Manifest.Release `
        -Expected @(
            'Provider',
            'Repository',
            'ApiVersion',
            'TagTemplate',
            'AssetTemplate'
        ) `
        -Description 'PowerShell release declaration'
    if ([string]$Manifest.Release.Provider -cne 'github' -or
        [string]$Manifest.Release.Repository -cne 'PowerShell/PowerShell' -or
        [string]$Manifest.Release.ApiVersion -cnotmatch '^\d{4}-\d{2}-\d{2}$' -or
        [string]$Manifest.Release.TagTemplate -cne 'v{version}' -or
        [string]$Manifest.Release.AssetTemplate -cne
            'PowerShell-{version}-win-x64.zip') {
        throw 'The PowerShell release declaration is invalid.'
    }
}

function Get-ProjDevPwshManifest {
    $Manifest = Import-PowerShellDataFile `
        -LiteralPath $script:ProjDevPwshManifestPath
    Assert-ProjDevPwshManifest -Manifest $Manifest
    return $Manifest
}

function Get-ProjDevPwshReleaseCoordinates {
    param(
        [Parameter(Mandatory = $true)][Collections.IDictionary]$Manifest,
        [Parameter(Mandatory = $true)][string]$Version
    )

    $Tag = ([string]$Manifest.Release.TagTemplate).Replace(
        '{version}',
        $Version
    )
    $Asset = ([string]$Manifest.Release.AssetTemplate).Replace(
        '{version}',
        $Version
    )
    return [pscustomobject][ordered]@{
        Tag = $Tag
        Asset = $Asset
        Url = 'https://github.com/{0}/releases/download/{1}/{2}' -f
            [string]$Manifest.Release.Repository,
            $Tag,
            $Asset
        SourceIdentity = 'github:{0}@{1}#{2}' -f
            [string]$Manifest.Release.Repository,
            $Tag,
            $Asset
    }
}

function Get-ProjDevPwshDefinition {
    $Manifest = Get-ProjDevPwshManifest
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
        throw "Enabled PowerShell must declare $($Manifest.VersionVariable)."
    }
    if ($Version -cne 'latest' -and
        $Version -cnotmatch '^\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?$') {
        throw "Invalid PowerShell version '$Version'."
    }

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
            'SWAWKIT_PROJ_PWSH_VERSION=latest cannot be combined with ' +
            'SWAWKIT_PROJ_PWSH_SHA256. Use an exact PowerShell version when ' +
            'pinning a project SHA-256.'
        )
    }

    $Coordinates = Get-ProjDevPwshReleaseCoordinates `
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
        ArchiveSubdir = ''
        RecipeVersion = [string]$Manifest.RecipeVersion
        Executable = [string]$Manifest.Executable
        RequiredPaths = [string[]]$Manifest.RequiredPaths
        ReleaseResolved = $false
        SelectionStatus = 'none'
    }
}

function Get-ProjDevPwshTrustStatus {
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

function Write-ProjDevPwshTrustWarning {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $Trust = Get-ProjDevPwshTrustStatus `
        -Context $Context `
        -Definition $Definition
    if ($Trust.Level -ceq 'pinned') {
        return
    }
    if ($Trust.Level -ceq 'upstream') {
        Write-Warning (
            "PowerShell $($Definition.Version) was verified with the GitHub " +
            'Release digest; SWAWKIT_PROJ_PWSH_SHA256 is not pinned by this ' +
            'project.'
        )
        return
    }
    if ($null -eq $Trust.Metadata) {
        Write-Warning (
            "PowerShell $($Definition.Version) is not pinned by " +
            'SWAWKIT_PROJ_PWSH_SHA256; .dev.setup will use the GitHub Release ' +
            'digest when available.'
        )
        return
    }
    Write-Warning (
        "PowerShell $($Definition.Version) has no comparable GitHub Release " +
        'digest or project SHA-256; installation is allowed but not ' +
        'content-pinned.'
    )
}

function Add-ProjDevPwshEnvironment {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Plan
    )

    $InstallRoot = Get-ProjDevInstallRoot `
        -Context $Context `
        -Definition $Definition
    Set-ProjDevEnvironmentVariable `
        -Plan $Plan `
        -Name 'SWAWKIT_PROJ_DEV_PWSH_MODE' `
        -Value ([string]$Definition.Mode)
    Set-ProjDevEnvironmentVariable `
        -Plan $Plan `
        -Name 'SWAWKIT_PROJ_DEV_PWSH_VERSION' `
        -Value ([string]$Definition.Version)
    Set-ProjDevEnvironmentVariable `
        -Plan $Plan `
        -Name 'SWAWKIT_PROJ_DEV_PWSH_SIGNATURE' `
        -Value (Get-ProjDevDefinitionSignature -Definition $Definition)
    Set-ProjDevEnvironmentVariable `
        -Plan $Plan `
        -Name 'SWAWKIT_PROJ_DEV_PWSH_HOME' `
        -Value $InstallRoot
    Add-ProjDevEnvironmentPath -Plan $Plan -Path $InstallRoot
}
