Set-StrictMode -Version 2.0

$script:ProjDevMsvcManifestPath = Join-Path $PSScriptRoot 'module.psd1'

function Assert-ProjDevMsvcDictionaryKeys {
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

function Get-ProjDevMsvcManifest {
    $Manifest = Import-PowerShellDataFile `
        -LiteralPath $script:ProjDevMsvcManifestPath
    Assert-ProjDevMsvcDictionaryKeys `
        -Dictionary $Manifest `
        -Expected @(
            'Schema', 'Name', 'ModeVariable', 'SetupImplemented',
            'ChannelVariable',
            'InstallMode', 'RecipeVersion', 'ChannelUrlTemplate',
            'VisualStudioManifestId', 'ResourceLanguage',
            'ToolPackageTemplates', 'SdkMsiNames'
        ) `
        -Description 'MSVC module manifest'
    if ([string]$Manifest.Schema -cne 'swawkit.proj-dev.module.v0' -or
        [string]$Manifest.Name -cne 'msvc' -or
        [string]$Manifest.ModeVariable -cne 'SWAWKIT_PROJ_MSVC_MODE' -or
        $Manifest.SetupImplemented -isnot [bool] -or
        -not [bool]$Manifest.SetupImplemented -or
        [string]$Manifest.ChannelVariable -cne 'SWAWKIT_PROJ_MSVC_CHANNEL' -or
        [string]$Manifest.InstallMode -cne 'managed' -or
        [string]$Manifest.ChannelUrlTemplate -cne
            'https://aka.ms/vs/{channel}/release/channel' -or
        [string]$Manifest.VisualStudioManifestId -cne
            'Microsoft.VisualStudio.Manifests.VisualStudio' -or
        [string]$Manifest.ResourceLanguage -cne 'en-US' -or
        @($Manifest.ToolPackageTemplates).Count -eq 0 -or
        @($Manifest.SdkMsiNames).Count -eq 0) {
        throw 'The MSVC module manifest is invalid.'
    }
    return $Manifest
}

function New-ProjDevMsvcDefinition {
    param([Parameter(Mandatory = $true)][string]$Channel)

    $Manifest = Get-ProjDevMsvcManifest
    $Channel = $Channel.Trim()
    [void](Get-ProjDevSafeSegment `
        -Value $Channel `
        -Description 'MSVC channel')
    if ($Channel -cnotmatch '^\d+$') {
        throw "$($Manifest.ChannelVariable) must be a numeric VS channel."
    }
    $ChannelUrl = ([string]$Manifest.ChannelUrlTemplate).Replace(
        '{channel}',
        $Channel
    )
    return [pscustomobject][ordered]@{
        Schema = [string]$Manifest.Schema
        Name = [string]$Manifest.Name
        Mode = [string]$Manifest.InstallMode
        Version = $Channel
        Channel = $Channel
        ChannelUrl = $ChannelUrl
        RecipeVersion = [string]$Manifest.RecipeVersion
        VisualStudioManifestId = [string]$Manifest.VisualStudioManifestId
        ResourceLanguage = [string]$Manifest.ResourceLanguage
        ToolPackageTemplates = [string[]]$Manifest.ToolPackageTemplates
        SdkMsiNames = [string[]]$Manifest.SdkMsiNames
    }
}

function Get-ProjDevMsvcDefinition {
    $Manifest = Get-ProjDevMsvcManifest
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

    $Channel = [string][Environment]::GetEnvironmentVariable(
        [string]$Manifest.ChannelVariable,
        [EnvironmentVariableTarget]::Process
    )
    return New-ProjDevMsvcDefinition -Channel $Channel
}

function Get-ProjDevMsvcDefinitionSignature {
    param([Parameter(Mandatory = $true)][object]$Definition)

    return Get-ProjDevSha256Text -Value ([string]::Join("`n", [string[]]@(
        'swawkit.proj-dev.msvc-definition.v0',
        [string]$Definition.Mode,
        [string]$Definition.Channel,
        [string]$Definition.ChannelUrl,
        [string]$Definition.RecipeVersion,
        [string]$Definition.ResourceLanguage,
        [string]::Join('|', [string[]]$Definition.ToolPackageTemplates),
        [string]::Join('|', [string[]]$Definition.SdkMsiNames)
    )))
}

function Get-ProjDevMsvcInstallRoot {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    return Join-Path (
        Join-Path (Join-Path $Context.EnvironmentRoot 'msvc') 'installs'
    ) ([string]$Definition.Channel)
}

function Get-ProjDevMsvcMetadataPath {
    param([Parameter(Mandatory = $true)][string]$InstallRoot)

    return Join-Path $InstallRoot '.swawkit-dev-msvc.json'
}

function Get-ProjDevMsvcRequiredPaths {
    param(
        [Parameter(Mandatory = $true)][string]$ToolVersion,
        [Parameter(Mandatory = $true)][string]$SdkVersion
    )

    return [string[]]@(
        'setup_x64.bat'
        "VC\Tools\MSVC\$ToolVersion\bin\Hostx64\x64\cl.exe"
        "VC\Tools\MSVC\$ToolVersion\bin\Hostx64\x64\link.exe"
        "VC\Tools\MSVC\$ToolVersion\bin\Hostx64\x64\lib.exe"
        "VC\Tools\MSVC\$ToolVersion\bin\Hostx64\x64\msdia140.dll"
        "VC\Tools\MSVC\$ToolVersion\include\yvals_core.h"
        "Windows Kits\10\bin\$SdkVersion\x64\rc.exe"
        "Windows Kits\10\Include\$SdkVersion\ucrt\stdio.h"
        "Windows Kits\10\Include\$SdkVersion\um\windows.h"
        "Windows Kits\10\Lib\$SdkVersion\ucrt\x64\ucrt.lib"
        "Windows Kits\10\Lib\$SdkVersion\um\x64\kernel32.lib"
    )
}

function Get-ProjDevMsvcInstallFileRecords {
    param(
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [Parameter(Mandatory = $true)][string[]]$RelativePaths
    )

    foreach ($RelativePath in $RelativePaths) {
        $Path = Resolve-ProjDevChildPath `
            -Root $InstallRoot `
            -RelativePath $RelativePath `
            -Description 'MSVC installed file'
        if (-not [IO.File]::Exists($Path) -or
            (Get-Item -LiteralPath $Path).Length -le 0) {
            throw "Required MSVC installed file is missing or empty: $RelativePath"
        }
        $Item = Get-Item -LiteralPath $Path
        [pscustomobject][ordered]@{
            path = $RelativePath
            length = [long]$Item.Length
            sha256 = Get-ProjDevFileSha256 -Path $Path
        }
    }
}

function Write-ProjDevMsvcMetadata {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Recipe,
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [Parameter(Mandatory = $true)][string]$ToolVersion,
        [Parameter(Mandatory = $true)][string]$SdkVersion
    )

    $RequiredPaths = Get-ProjDevMsvcRequiredPaths `
        -ToolVersion $ToolVersion `
        -SdkVersion $SdkVersion
    $Metadata = [ordered]@{
        schema = 'swawkit.proj-dev.msvc-install.v0'
        name = 'msvc'
        channel = [string]$Definition.Channel
        channelUrl = [string]$Definition.ChannelUrl
        recipeVersion = [string]$Definition.RecipeVersion
        definitionSignature = Get-ProjDevMsvcDefinitionSignature `
            -Definition $Definition
        manifestUrl = [string]$Recipe.ManifestUrl
        manifestSha256 = [string]$Recipe.ManifestSha256
        toolPackageVersion = [string]$Recipe.ToolPackageVersion
        toolVersion = $ToolVersion
        sdkPackage = [string]$Recipe.SdkPackageId
        sdkVersion = $SdkVersion
        sourceVerification = 'microsoft-manifest'
        files = @(
            Get-ProjDevMsvcInstallFileRecords `
                -InstallRoot $InstallRoot `
                -RelativePaths $RequiredPaths
        )
    }
    Write-ProjDevTextAtomic `
        -Path (Get-ProjDevMsvcMetadataPath -InstallRoot $InstallRoot) `
        -Content (ConvertTo-ProjDevJsonText -Value $Metadata) `
        -ControlledRoot $InstallRoot
}

function Get-ProjDevMsvcValidMetadata {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [AllowNull()][string]$InstallRoot = $null
    )

    if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
        $InstallRoot = Get-ProjDevMsvcInstallRoot `
            -Context $Context `
            -Definition $Definition
    }
    $MetadataPath = Get-ProjDevMsvcMetadataPath -InstallRoot $InstallRoot
    if (-not [IO.File]::Exists($MetadataPath)) {
        return $null
    }
    try {
        $Metadata = [IO.File]::ReadAllText(
            $MetadataPath,
            [Text.Encoding]::UTF8
        ) | ConvertFrom-Json
        if ([string]$Metadata.schema -cne
                'swawkit.proj-dev.msvc-install.v0' -or
            [string]$Metadata.name -cne 'msvc' -or
            [string]$Metadata.channel -cne [string]$Definition.Channel -or
            [string]$Metadata.recipeVersion -cne
                [string]$Definition.RecipeVersion -or
            [string]$Metadata.definitionSignature -cne
                (Get-ProjDevMsvcDefinitionSignature -Definition $Definition) -or
            [string]$Metadata.manifestSha256 -cnotmatch '^[a-f0-9]{64}$' -or
            [string]$Metadata.toolVersion -cnotmatch '^\d+(\.\d+)+$' -or
            [string]$Metadata.sdkVersion -cnotmatch '^\d+(\.\d+)+$' -or
            [string]$Metadata.sourceVerification -cne
                'microsoft-manifest') {
            return $null
        }
        $ExpectedPaths = Get-ProjDevMsvcRequiredPaths `
            -ToolVersion ([string]$Metadata.toolVersion) `
            -SdkVersion ([string]$Metadata.sdkVersion)
        $Records = @($Metadata.files)
        if ($Records.Count -ne $ExpectedPaths.Count) {
            return $null
        }
        foreach ($RelativePath in $ExpectedPaths) {
            $RecordsForPath = @($Records | Where-Object {
                [string]$_.path -ceq $RelativePath
            })
            if ($RecordsForPath.Count -ne 1 -or
                [string]$RecordsForPath[0].sha256 -cnotmatch
                    '^[a-f0-9]{64}$') {
                return $null
            }
            $Path = Resolve-ProjDevChildPath `
                -Root $InstallRoot `
                -RelativePath $RelativePath `
                -Description 'MSVC installed file'
            if (-not [IO.File]::Exists($Path) -or
                [long](Get-Item -LiteralPath $Path).Length -ne
                    [long]($RecordsForPath[0].length)) {
                return $null
            }
        }
        return $Metadata
    } catch {
        return $null
    }
}

function Test-ProjDevMsvcInstalled {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [AllowNull()][string]$InstallRoot = $null
    )

    if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
        $InstallRoot = Get-ProjDevMsvcInstallRoot `
            -Context $Context `
            -Definition $Definition
    }
    $Metadata = Get-ProjDevMsvcValidMetadata `
        -Context $Context `
        -Definition $Definition `
        -InstallRoot $InstallRoot
    if ($null -eq $Metadata) {
        return $false
    }
    foreach ($Record in @($Metadata.files)) {
        $Path = Resolve-ProjDevChildPath `
            -Root $InstallRoot `
            -RelativePath ([string]$Record.path) `
            -Description 'MSVC installed file'
        if ((Get-ProjDevFileSha256 -Path $Path) -cne
            [string]$Record.sha256) {
            return $false
        }
    }
    return $true
}
