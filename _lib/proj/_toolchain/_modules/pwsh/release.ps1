Set-StrictMode -Version 2.0

function Get-ProjDevPwshReleaseProperty {
    param(
        [Parameter(Mandatory = $true)][object]$Value,
        [Parameter(Mandatory = $true)][string]$Name
    )

    $Property = $Value.PSObject.Properties[$Name]
    if ($null -eq $Property) {
        return $null
    }
    return $Property.Value
}

function Request-ProjDevPwshGitHubRelease {
    param([Parameter(Mandatory = $true)][object]$Definition)

    $Tag = ([string]$Definition.Release.TagTemplate).Replace(
        '{version}',
        [string]$Definition.Version
    )
    $Endpoint = 'https://api.github.com/repos/{0}/releases/tags/{1}' -f
        [string]$Definition.Release.Repository,
        [Uri]::EscapeDataString($Tag)
    try {
        return Invoke-RestMethod `
            -Uri $Endpoint `
            -Headers @{
                Accept = 'application/vnd.github+json'
                'X-GitHub-Api-Version' = [string]$Definition.Release.ApiVersion
                'User-Agent' = 'swawkit-proj-v0'
            } `
            -UseBasicParsing `
            -TimeoutSec 30 `
            -ErrorAction Stop
    } catch {
        throw (
            "Cannot resolve PowerShell $($Definition.Version) from GitHub " +
            "Releases: $($_.Exception.Message)"
        )
    }
}

function Request-ProjDevPwshGitHubLatestRelease {
    param([Parameter(Mandatory = $true)][object]$Definition)

    $Endpoint = 'https://api.github.com/repos/{0}/releases/latest' -f
        [string]$Definition.Release.Repository
    try {
        return Invoke-RestMethod `
            -Uri $Endpoint `
            -Headers @{
                Accept = 'application/vnd.github+json'
                'X-GitHub-Api-Version' = [string]$Definition.Release.ApiVersion
                'User-Agent' = 'swawkit-proj-v0'
            } `
            -UseBasicParsing `
            -TimeoutSec 30 `
            -ErrorAction Stop
    } catch {
        throw (
            'Cannot resolve the latest PowerShell release from GitHub ' +
            "Releases: $($_.Exception.Message)"
        )
    }
}

function Set-ProjDevPwshResolvedVersion {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][string]$Version
    )

    if ($Version -cnotmatch '^\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?$') {
        throw "GitHub returned an invalid PowerShell release version '$Version'."
    }
    $Coordinates = Get-ProjDevPwshReleaseCoordinates `
        -Manifest (Get-ProjDevPwshManifest) `
        -Version $Version
    $Definition.Version = $Version
    $Definition.Url = [string]$Coordinates.Url
    $Definition.SourceIdentity = [string]$Coordinates.SourceIdentity
    return $Definition
}

function Resolve-ProjDevPwshLatestRelease {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [AllowNull()][object]$Release = $null
    )

    if ([string]$Definition.RequestedVersion -cne 'latest') {
        throw 'The PowerShell definition does not declare the latest selector.'
    }
    if ($null -eq $Release) {
        $Release = Request-ProjDevPwshGitHubLatestRelease `
            -Definition $Definition
    }
    $Tag = [string](Get-ProjDevPwshReleaseProperty `
        -Value $Release `
        -Name 'tag_name')
    if (-not $Tag.StartsWith('v', [StringComparison]::Ordinal)) {
        throw "GitHub returned an invalid latest PowerShell release tag '$Tag'."
    }
    [void](Set-ProjDevPwshResolvedVersion `
        -Definition $Definition `
        -Version $Tag.Substring(1))
    [void](Resolve-ProjDevPwshRelease `
        -Definition $Definition `
        -Release $Release)
    $Definition.SelectionStatus = 'pending'
    return $Definition
}

function Resolve-ProjDevPwshRelease {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [AllowNull()][object]$Release = $null
    )

    if ($null -eq $Release) {
        $Release = Request-ProjDevPwshGitHubRelease -Definition $Definition
    }
    $Coordinates = Get-ProjDevPwshReleaseCoordinates `
        -Manifest (Get-ProjDevPwshManifest) `
        -Version ([string]$Definition.Version)
    $ActualTag = [string](Get-ProjDevPwshReleaseProperty `
        -Value $Release `
        -Name 'tag_name')
    if ($ActualTag -cne [string]$Coordinates.Tag) {
        throw (
            "GitHub returned PowerShell release tag '$ActualTag'; expected " +
            "'$($Coordinates.Tag)'."
        )
    }

    $Assets = @(Get-ProjDevPwshReleaseProperty `
        -Value $Release `
        -Name 'assets')
    $Matches = @($Assets | Where-Object {
        [string](Get-ProjDevPwshReleaseProperty `
            -Value $_ `
            -Name 'name') -ceq [string]$Coordinates.Asset
    })
    if ($Matches.Count -ne 1) {
        throw (
            "GitHub release '$($Coordinates.Tag)' must contain exactly one " +
            "'$($Coordinates.Asset)' asset; found $($Matches.Count)."
        )
    }

    $Asset = $Matches[0]
    $UrlText = [string](Get-ProjDevPwshReleaseProperty `
        -Value $Asset `
        -Name 'browser_download_url')
    $Url = $null
    if (-not [Uri]::TryCreate(
        $UrlText,
        [UriKind]::Absolute,
        [ref]$Url
    ) -or
        $Url.Scheme -cne 'https' -or
        $Url.Host -cne 'github.com' -or
        $Url.AbsoluteUri -cne [string]$Coordinates.Url) {
        throw "GitHub returned an invalid PowerShell asset URL: $UrlText"
    }

    $Digest = [string](Get-ProjDevPwshReleaseProperty `
        -Value $Asset `
        -Name 'digest')
    $GitHubSha256 = ''
    if (-not [string]::IsNullOrWhiteSpace($Digest)) {
        $DigestMatch = [regex]::Match(
            $Digest.Trim(),
            '^sha256:([a-fA-F0-9]{64})$'
        )
        if (-not $DigestMatch.Success) {
            throw (
                "GitHub returned an invalid digest for PowerShell " +
                "$($Definition.Version): $Digest"
            )
        }
        $GitHubSha256 = $DigestMatch.Groups[1].Value.ToLowerInvariant()
    }

    $ProjectSha256 = Get-ProjDevProjectSha256 -Definition $Definition
    if (-not [string]::IsNullOrWhiteSpace($ProjectSha256) -and
        -not [string]::IsNullOrWhiteSpace($GitHubSha256) -and
        $ProjectSha256 -cne $GitHubSha256) {
        throw (
            'SWAWKIT_PROJ_PWSH_SHA256 does not match the GitHub Release ' +
            "digest for PowerShell $($Definition.Version)."
        )
    }

    $Definition.Url = $Url.AbsoluteUri
    if (-not [string]::IsNullOrWhiteSpace($ProjectSha256)) {
        $Definition.Sha256 = $ProjectSha256
        $Definition.Verification = 'project'
    } elseif (-not [string]::IsNullOrWhiteSpace($GitHubSha256)) {
        $Definition.Sha256 = $GitHubSha256
        $Definition.Verification = 'github'
    } else {
        $Definition.Sha256 = ''
        $Definition.Verification = 'unverified'
    }
    $Definition.ReleaseResolved = $true
    return $Definition
}
