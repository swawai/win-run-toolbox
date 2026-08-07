Set-StrictMode -Version 2.0

function Get-ProjDevExpectedSha256 {
    param([Parameter(Mandatory = $true)][object]$Definition)

    $Expected = ([string]$Definition.Sha256).Trim().ToLowerInvariant()
    if ([string]::IsNullOrWhiteSpace($Expected)) {
        return ''
    }
    if ($Expected -cnotmatch '^[a-f0-9]{64}$') {
        throw "Invalid SHA-256 for $($Definition.Name) $($Definition.Version)."
    }
    return $Expected
}

function Get-ProjDevProjectSha256 {
    param([Parameter(Mandatory = $true)][object]$Definition)

    $Expected = ([string]$Definition.ProjectSha256).Trim().ToLowerInvariant()
    if ([string]::IsNullOrWhiteSpace($Expected)) {
        return ''
    }
    if ($Expected -cnotmatch '^[a-f0-9]{64}$') {
        throw (
            "Invalid project SHA-256 for $($Definition.Name) " +
            "$($Definition.Version)."
        )
    }
    return $Expected
}

function Test-ProjDevRequiredFiles {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string[]]$RelativePaths
    )

    foreach ($RelativePath in $RelativePaths) {
        $RequiredPath = Resolve-ProjDevChildPath `
            -Root $Root `
            -RelativePath $RelativePath `
            -Description 'required file'
        if (-not [IO.File]::Exists($RequiredPath) -or
            (Get-Item -LiteralPath $RequiredPath).Length -le 0) {
            return $false
        }
    }
    return $true
}

function Assert-ProjDevArchiveDefinition {
    param([Parameter(Mandatory = $true)][object]$Definition)

    [void](Get-ProjDevSafeSegment `
        -Value ([string]$Definition.Name) `
        -Description 'tool name')
    [void](Get-ProjDevSafeSegment `
        -Value ([string]$Definition.Version) `
        -Description "version for $($Definition.Name)")
    if ([string]::IsNullOrWhiteSpace([string]$Definition.Url)) {
        throw "No artifact URL is declared for $($Definition.Name)."
    }
    [void](Get-ProjDevExpectedSha256 -Definition $Definition)
    [void](Get-ProjDevProjectSha256 -Definition $Definition)
    [void](Get-ProjDevSafeSegment `
        -Value ([string]$Definition.RecipeVersion) `
        -Description "recipe version for $($Definition.Name)")
    if (@($Definition.RequiredPaths).Count -eq 0) {
        throw "No required files are declared for $($Definition.Name)."
    }
    foreach ($RelativePath in [string[]]$Definition.RequiredPaths) {
        [void](Resolve-ProjDevChildPath `
            -Root ([IO.Path]::GetTempPath()) `
            -RelativePath $RelativePath `
            -Description "required file for $($Definition.Name)")
    }
    if ([string]::IsNullOrWhiteSpace([string]$Definition.Executable)) {
        throw "No executable is declared for $($Definition.Name)."
    }
    [void](Resolve-ProjDevChildPath `
        -Root ([IO.Path]::GetTempPath()) `
        -RelativePath ([string]$Definition.Executable) `
        -Description "executable for $($Definition.Name)")
    if ([string[]]$Definition.RequiredPaths -cnotcontains
        [string]$Definition.Executable) {
        throw (
            "Executable '$($Definition.Executable)' must be included in the " +
            "required files for $($Definition.Name)."
        )
    }
    if (-not [string]::IsNullOrWhiteSpace(
        [string]$Definition.ArchiveSubdir
    )) {
        [void](Resolve-ProjDevChildPath `
            -Root ([IO.Path]::GetTempPath()) `
            -RelativePath ([string]$Definition.ArchiveSubdir) `
            -Description "archive subdirectory for $($Definition.Name)")
    }
}

function Get-ProjDevDefinitionSignature {
    param([Parameter(Mandatory = $true)][object]$Definition)

    Assert-ProjDevArchiveDefinition -Definition $Definition
    $Identity = [string]::Join("`n", [string[]]@(
        'swawkit.proj-dev.definition.v1',
        [string]$Definition.Name,
        [string]$Definition.Mode,
        [string]$Definition.Version,
        [string]$Definition.SourceIdentity,
        (Get-ProjDevProjectSha256 -Definition $Definition),
        [string]$Definition.ArchiveSubdir,
        [string]$Definition.RecipeVersion,
        [string]$Definition.Executable,
        [string]::Join('|', [string[]]$Definition.RequiredPaths)
    ))
    return Get-ProjDevSha256Text -Value $Identity
}

function Get-ProjDevInstallRoot {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $Name = Get-ProjDevSafeSegment `
        -Value ([string]$Definition.Name) `
        -Description 'tool name'
    $Version = Get-ProjDevSafeSegment `
        -Value ([string]$Definition.Version) `
        -Description "version for $Name"
    return Join-Path (
        Join-Path (Join-Path $Context.EnvironmentRoot $Name) 'installs'
    ) $Version
}

function Get-ProjDevInstallMetadataPath {
    param([Parameter(Mandatory = $true)][string]$InstallRoot)

    return Join-Path $InstallRoot '.swawkit-dev-install.json'
}

function Get-ProjDevInstallFileRecords {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][string]$InstallRoot
    )

    foreach ($RelativePath in [string[]]$Definition.RequiredPaths) {
        $FilePath = Resolve-ProjDevChildPath `
            -Root $InstallRoot `
            -RelativePath $RelativePath `
            -Description 'installed file'
        if (-not [IO.File]::Exists($FilePath)) {
            throw "Required installed file is missing: $RelativePath"
        }
        $Item = Get-Item -LiteralPath $FilePath
        if ($Item.Length -le 0) {
            throw "Required installed file is empty: $RelativePath"
        }
        [pscustomobject][ordered]@{
            path = $RelativePath
            length = [long]$Item.Length
            sha256 = Get-ProjDevFileSha256 -Path $FilePath
        }
    }
}

function Write-ProjDevInstallMetadata {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][string]$InstallRoot
    )

    $SourceSha256 = Get-ProjDevExpectedSha256 -Definition $Definition
    if ([string]::IsNullOrWhiteSpace($SourceSha256)) {
        throw (
            "The downloaded SHA-256 was not recorded for " +
            "$($Definition.Name) $($Definition.Version)."
        )
    }
    $Metadata = [ordered]@{
        schema = 'swawkit.proj-dev.install.v0'
        name = [string]$Definition.Name
        version = [string]$Definition.Version
        sourceUrl = [string]$Definition.Url
        sourceSha256 = $SourceSha256
        sourceVerification = [string]$Definition.Verification
        recipeVersion = [string]$Definition.RecipeVersion
        definitionSignature = Get-ProjDevDefinitionSignature `
            -Definition $Definition
        files = @(
            Get-ProjDevInstallFileRecords `
                -Definition $Definition `
                -InstallRoot $InstallRoot
        )
    }
    Write-ProjDevTextAtomic `
        -Path (Get-ProjDevInstallMetadataPath -InstallRoot $InstallRoot) `
        -Content (ConvertTo-ProjDevJsonText -Value $Metadata) `
        -ControlledRoot $InstallRoot
}

function Get-ProjDevValidInstallMetadata {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [AllowNull()][string]$InstallRoot = $null
    )

    Assert-ProjDevArchiveDefinition -Definition $Definition
    if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
        $InstallRoot = Get-ProjDevInstallRoot `
            -Context $Context `
            -Definition $Definition
    }
    if (-not [IO.Directory]::Exists($InstallRoot)) {
        return $null
    }
    $MetadataPath = Get-ProjDevInstallMetadataPath -InstallRoot $InstallRoot
    if (-not [IO.File]::Exists($MetadataPath)) {
        return $null
    }

    try {
        $Metadata = Get-Content `
            -LiteralPath $MetadataPath `
            -Raw `
            -Encoding UTF8 | ConvertFrom-Json
        $ProjectSha256 = Get-ProjDevProjectSha256 -Definition $Definition
        $RecordedSha256 = (
            [string]$Metadata.sourceSha256
        ).Trim().ToLowerInvariant()
        if ([string]$Metadata.schema -cne 'swawkit.proj-dev.install.v0' -or
            [string]$Metadata.name -cne [string]$Definition.Name -or
            [string]$Metadata.version -cne [string]$Definition.Version -or
            $RecordedSha256 -cnotmatch '^[a-f0-9]{64}$' -or
            (-not [string]::IsNullOrWhiteSpace($ProjectSha256) -and
                $RecordedSha256 -cne $ProjectSha256) -or
            [string]$Metadata.recipeVersion -cne (
                [string]$Definition.RecipeVersion
            ) -or
            [string]$Metadata.definitionSignature -cne (
                Get-ProjDevDefinitionSignature -Definition $Definition
            )) {
            return $null
        }

        $Records = @($Metadata.files)
        if ($Records.Count -ne @($Definition.RequiredPaths).Count) {
            return $null
        }
        foreach ($RelativePath in [string[]]$Definition.RequiredPaths) {
            $Matches = @($Records | Where-Object {
                [string]$_.path -ceq $RelativePath
            })
            if ($Matches.Count -ne 1) {
                return $null
            }
            $FilePath = Resolve-ProjDevChildPath `
                -Root $InstallRoot `
                -RelativePath $RelativePath `
                -Description 'installed file'
            if (-not [IO.File]::Exists($FilePath)) {
                return $null
            }
            $Item = Get-Item -LiteralPath $FilePath
            if ([long]$Matches[0].length -ne [long]$Item.Length -or
                [string]$Matches[0].sha256 -cnotmatch '^[a-f0-9]{64}$') {
                return $null
            }
        }
        return $Metadata
    } catch {
        return $null
    }
}

function Test-ProjDevRunnable {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    return $null -ne (Get-ProjDevValidInstallMetadata `
        -Context $Context `
        -Definition $Definition)
}

function Test-ProjDevInstalled {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [AllowNull()][string]$InstallRoot = $null
    )

    $Metadata = Get-ProjDevValidInstallMetadata `
        -Context $Context `
        -Definition $Definition `
        -InstallRoot $InstallRoot
    if ($null -eq $Metadata) {
        return $false
    }
    if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
        $InstallRoot = Get-ProjDevInstallRoot `
            -Context $Context `
            -Definition $Definition
    }
    foreach ($Record in @($Metadata.files)) {
        $FilePath = Resolve-ProjDevChildPath `
            -Root $InstallRoot `
            -RelativePath ([string]$Record.path) `
            -Description 'installed file'
        if ([string]$Record.sha256 -cne (
            Get-ProjDevFileSha256 -Path $FilePath
        )) {
            return $false
        }
    }
    return $true
}
