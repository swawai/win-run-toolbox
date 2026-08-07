Set-StrictMode -Version 2.0

$script:ProjDevRustManifestPath = Join-Path $PSScriptRoot 'module.psd1'

function Get-ProjDevRustAmbientOverrideNames {
    return [string[]]@(
        'RUSTUP_TOOLCHAIN'
        'RUSTUP_TOOLCHAIN_SOURCE'
        'RUSTUP_DIST_SERVER'
        'RUSTUP_DIST_ROOT'
        'RUSTUP_UPDATE_ROOT'
        'RUSTUP_VERSION'
    )
}

function Assert-ProjDevRustDictionaryKeys {
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

function Get-ProjDevRustManifest {
    $Manifest = Import-PowerShellDataFile `
        -LiteralPath $script:ProjDevRustManifestPath
    Assert-ProjDevRustDictionaryKeys `
        -Dictionary $Manifest `
        -Expected @(
            'Schema', 'Name', 'ModeVariable', 'SetupImplemented',
            'ToolchainVariable',
            'ProfileVariable', 'HostVariable', 'InstallMode',
            'RecipeVersion', 'SupportedProfiles', 'SupportedHost',
            'RequiredComponents', 'RustupInit'
        ) `
        -Description 'Rust module manifest'
    if ($Manifest.RustupInit -isnot [Collections.IDictionary]) {
        throw 'The Rust module manifest must declare rustup-init.'
    }
    Assert-ProjDevRustDictionaryKeys `
        -Dictionary $Manifest.RustupInit `
        -Expected @('Url', 'ChecksumUrl') `
        -Description 'Rust rustup-init declaration'
    if ([string]$Manifest.Schema -cne 'swawkit.proj-dev.module.v0' -or
        [string]$Manifest.Name -cne 'rust' -or
        [string]$Manifest.ModeVariable -cne 'SWAWKIT_PROJ_RUST_MODE' -or
        $Manifest.SetupImplemented -isnot [bool] -or
        -not [bool]$Manifest.SetupImplemented -or
        [string]$Manifest.ToolchainVariable -cne
            'SWAWKIT_PROJ_RUST_TOOLCHAIN' -or
        [string]$Manifest.ProfileVariable -cne
            'SWAWKIT_PROJ_RUST_PROFILE' -or
        [string]$Manifest.HostVariable -cne 'SWAWKIT_PROJ_RUST_HOST' -or
        [string]$Manifest.InstallMode -cne 'rustup' -or
        [string]$Manifest.SupportedHost -cne
            'x86_64-pc-windows-msvc' -or
        @($Manifest.SupportedProfiles).Count -eq 0 -or
        @($Manifest.RequiredComponents).Count -eq 0) {
        throw 'The Rust module manifest is invalid.'
    }
    $Components = [string[]]@(
        $Manifest.RequiredComponents | ForEach-Object {
            ([string]$_).Trim().ToLowerInvariant()
        }
    )
    $SeenComponents = [Collections.Generic.HashSet[string]]::new(
        [StringComparer]::Ordinal
    )
    foreach ($Component in $Components) {
        if ($Component -cne 'rustfmt' -or
            -not $SeenComponents.Add($Component)) {
            throw "Unsupported or duplicate required Rust component '$Component'."
        }
    }
    [Array]::Sort($Components, [StringComparer]::Ordinal)
    $Manifest.RequiredComponents = $Components
    return $Manifest
}

function New-ProjDevRustDefinition {
    param(
        [Parameter(Mandatory = $true)][string]$Toolchain,
        [Parameter(Mandatory = $true)][string]$Profile,
        [Parameter(Mandatory = $true)][string]$HostTriple
    )

    $Manifest = Get-ProjDevRustManifest
    $Toolchain = $Toolchain.Trim().ToLowerInvariant()
    if ($Toolchain -cnotmatch (
        '^(?:(?:stable|beta|nightly)(?:-\d{4}-\d{2}-\d{2})?|' +
        '\d+\.\d+(?:\.\d+)?(?:-beta(?:\.\d+)?)?)$'
    )) {
        throw (
            "$($Manifest.ToolchainVariable) must be stable, beta, nightly, " +
            'a Rust version, or a dated channel.'
        )
    }
    [void](Get-ProjDevSafeSegment `
        -Value $Toolchain `
        -Description 'Rust toolchain')

    $Profile = $Profile.Trim().ToLowerInvariant()
    if ([string[]]$Manifest.SupportedProfiles -cnotcontains $Profile) {
        throw (
            "Unsupported Rust profile '$Profile'. Expected one of: " +
            [string]::Join(', ', [string[]]$Manifest.SupportedProfiles)
        )
    }
    $HostTriple = $HostTriple.Trim().ToLowerInvariant()
    if ($HostTriple -cne [string]$Manifest.SupportedHost) {
        throw (
            "Rust V0 supports host '$($Manifest.SupportedHost)' only; " +
            "received '$HostTriple'."
        )
    }

    return [pscustomobject][ordered]@{
        Schema = [string]$Manifest.Schema
        Name = [string]$Manifest.Name
        Mode = [string]$Manifest.InstallMode
        Version = $Toolchain
        Toolchain = $Toolchain
        Profile = $Profile
        Host = $HostTriple
        ToolchainName = "$Toolchain-$HostTriple"
        RequiredComponents = [string[]]$Manifest.RequiredComponents
        RecipeVersion = [string]$Manifest.RecipeVersion
        RustupInitUrl = [string]$Manifest.RustupInit.Url
        RustupInitChecksumUrl = [string]$Manifest.RustupInit.ChecksumUrl
    }
}

function Get-ProjDevRustDefinition {
    $Manifest = Get-ProjDevRustManifest
    $Mode = [string][Environment]::GetEnvironmentVariable(
        [string]$Manifest.ModeVariable,
        'Process'
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

    return New-ProjDevRustDefinition `
        -Toolchain ([string][Environment]::GetEnvironmentVariable(
            [string]$Manifest.ToolchainVariable,
            'Process')) `
        -Profile ([string][Environment]::GetEnvironmentVariable(
            [string]$Manifest.ProfileVariable,
            'Process')) `
        -HostTriple ([string][Environment]::GetEnvironmentVariable(
            [string]$Manifest.HostVariable,
            'Process'))
}

function Get-ProjDevRustDefinitionSignature {
    param([Parameter(Mandatory = $true)][object]$Definition)

    return Get-ProjDevSha256Text -Value ([string]::Join("`n", [string[]]@(
        'swawkit.proj-dev.rust-definition.v0',
        [string]$Definition.Mode,
        [string]$Definition.Toolchain,
        [string]$Definition.Profile,
        [string]$Definition.Host,
        [string]::Join(',', [string[]]$Definition.RequiredComponents),
        [string]$Definition.RecipeVersion,
        [string]$Definition.RustupInitUrl,
        [string]$Definition.RustupInitChecksumUrl
    )))
}

function Get-ProjDevRustInstallRoot {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    return Join-Path (
        Join-Path (Join-Path $Context.EnvironmentRoot 'rust') 'installs'
    ) ([string]$Definition.Toolchain)
}

function Get-ProjDevRustMetadataPath {
    param([Parameter(Mandatory = $true)][string]$InstallRoot)

    return Join-Path $InstallRoot '.swawkit-dev-rust.json'
}

function Get-ProjDevRustRequiredPaths {
    param(
        [Parameter(Mandatory = $true)][string]$ToolchainName,
        [Parameter(Mandatory = $true)][string]$HostTriple,
        [Parameter(Mandatory = $true)][string[]]$RequiredComponents
    )

    $Paths = [Collections.Generic.List[string]]::new()
    foreach ($RelativePath in [string[]]@(
        'cargo\bin\rustup.exe'
        'cargo\bin\rustc.exe'
        'cargo\bin\cargo.exe'
        'cargo\bin\rustfmt.exe'
        'cargo\bin\cargo-fmt.exe'
        'rustup\settings.toml'
        "rustup\toolchains\$ToolchainName\bin\rustc.exe"
        "rustup\toolchains\$ToolchainName\bin\cargo.exe"
        "rustup\toolchains\$ToolchainName\bin\rustdoc.exe"
        (
            "rustup\toolchains\$ToolchainName\lib\rustlib\" +
            "manifest-rust-std-$HostTriple"
        )
    )) {
        $Paths.Add($RelativePath)
    }
    foreach ($Component in $RequiredComponents) {
        switch -CaseSensitive ($Component) {
            'rustfmt' {
                $Paths.Add("rustup\toolchains\$ToolchainName\bin\rustfmt.exe")
                $Paths.Add("rustup\toolchains\$ToolchainName\bin\cargo-fmt.exe")
            }
            default {
                throw "Unsupported required Rust component '$Component'."
            }
        }
    }
    return [string[]]$Paths.ToArray()
}

function Get-ProjDevRustInventoryPaths {
    param(
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $Paths = [Collections.Generic.HashSet[string]]::new(
        [StringComparer]::Ordinal
    )
    foreach ($RelativePath in Get-ProjDevRustRequiredPaths `
        -ToolchainName ([string]$Definition.ToolchainName) `
        -HostTriple ([string]$Definition.Host) `
        -RequiredComponents ([string[]]$Definition.RequiredComponents)) {
        [void]$Paths.Add($RelativePath)
    }
    $ToolchainRelative = "rustup\toolchains\$($Definition.ToolchainName)"
    $ToolchainRoot = Resolve-ProjDevChildPath `
        -Root $InstallRoot `
        -RelativePath $ToolchainRelative `
        -Description 'Rust toolchain directory'
    if (-not [IO.Directory]::Exists($ToolchainRoot)) {
        throw "Rust toolchain directory is missing: $ToolchainRelative"
    }
    $InstallPrefix = (Get-ProjDevFullPath -Path $InstallRoot).
        TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    foreach ($Item in Get-ChildItem `
        -LiteralPath $ToolchainRoot `
        -File `
        -Recurse `
        -Force) {
        $FullPath = Get-ProjDevFullPath -Path $Item.FullName
        if (-not $FullPath.StartsWith(
            $InstallPrefix,
            [StringComparison]::OrdinalIgnoreCase
        )) {
            throw "Rust inventory escaped its installation root: $FullPath"
        }
        $RelativePath = $FullPath.Substring($InstallPrefix.Length)
        [void](Resolve-ProjDevChildPath `
            -Root $InstallRoot `
            -RelativePath $RelativePath `
            -Description 'Rust inventory file')
        [void]$Paths.Add($RelativePath)
    }
    $Result = [string[]]@($Paths)
    [Array]::Sort($Result, [StringComparer]::Ordinal)
    return $Result
}

function Get-ProjDevRustInstallFileShape {
    param(
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [Parameter(Mandatory = $true)][string]$RelativePath
    )

    $Path = Resolve-ProjDevChildPath `
        -Root $InstallRoot `
        -RelativePath $RelativePath `
        -Description 'Rust installed file'
    if (-not [IO.File]::Exists($Path)) {
        throw "Required Rust installed file is missing: $RelativePath"
    }
    $Item = Get-Item -LiteralPath $Path -Force
    $IsReparsePoint = ($Item.Attributes -band
        [IO.FileAttributes]::ReparsePoint) -ne 0
    $Kind = 'file'
    $Target = ''
    if ($IsReparsePoint) {
        $AllowedProxyLinks = @(
            'cargo\bin\rustc.exe',
            'cargo\bin\cargo.exe',
            'cargo\bin\rustfmt.exe',
            'cargo\bin\cargo-fmt.exe'
        )
        $LinkTypeProperty = $Item.PSObject.Properties['LinkType']
        $TargetProperty = $Item.PSObject.Properties['Target']
        if ($null -eq $TargetProperty) {
            $Targets = @()
        } else {
            $Targets = @($TargetProperty.Value)
        }
        if ($AllowedProxyLinks -cnotcontains $RelativePath -or
            $null -eq $LinkTypeProperty -or
            [string]$LinkTypeProperty.Value -cne 'SymbolicLink' -or
            $Targets.Count -ne 1 -or
            [string]$Targets[0] -cne 'rustup.exe') {
            throw "Rust installed link is not an owned rustup proxy: $RelativePath"
        }
        $Kind = 'symlink'
        $Target = [string]$Targets[0]
    } elseif ($Item.Length -le 0) {
        throw "Required Rust installed file is empty: $RelativePath"
    }
    return [pscustomobject][ordered]@{
        path = $RelativePath
        kind = $Kind
        target = $Target
        length = [long]$Item.Length
    }
}

function Get-ProjDevRustInstallFileRecords {
    param(
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [Parameter(Mandatory = $true)][string[]]$RelativePaths
    )

    foreach ($RelativePath in $RelativePaths) {
        $Shape = Get-ProjDevRustInstallFileShape `
            -InstallRoot $InstallRoot `
            -RelativePath $RelativePath
        $Path = Resolve-ProjDevChildPath `
            -Root $InstallRoot `
            -RelativePath $RelativePath `
            -Description 'Rust installed file'
        [pscustomobject][ordered]@{
            path = [string]$Shape.path
            kind = [string]$Shape.kind
            target = [string]$Shape.target
            length = [long]$Shape.length
            sha256 = Get-ProjDevFileSha256 -Path $Path
        }
    }
}
