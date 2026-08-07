Set-StrictMode -Version 2.0

function Get-ProjDevRustValidMetadata {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [AllowNull()][string]$InstallRoot = $null
    )

    if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
        $InstallRoot = Get-ProjDevRustInstallRoot `
            -Context $Context `
            -Definition $Definition
    }
    $MetadataPath = Get-ProjDevRustMetadataPath -InstallRoot $InstallRoot
    if (-not [IO.File]::Exists($MetadataPath)) {
        return $null
    }
    try {
        $Metadata = [IO.File]::ReadAllText(
            $MetadataPath,
            [Text.Encoding]::UTF8
        ) | ConvertFrom-Json
        if ([string]$Metadata.schema -cne
                'swawkit.proj-dev.rust-install.v0' -or
            [string]$Metadata.name -cne 'rust' -or
            [string]$Metadata.inventory -cne 'toolchain-files-v0' -or
            [string]$Metadata.declaredToolchain -cne
                [string]$Definition.Toolchain -or
            [string]$Metadata.toolchainName -cne
                [string]$Definition.ToolchainName -or
            [string]$Metadata.profile -cne [string]$Definition.Profile -or
            [string]$Metadata.host -cne [string]$Definition.Host -or
            [string]$Metadata.recipeVersion -cne
                [string]$Definition.RecipeVersion -or
            [string]$Metadata.definitionSignature -cne
                (Get-ProjDevRustDefinitionSignature -Definition $Definition) -or
            [string]$Metadata.rustupInitUrl -cne
                [string]$Definition.RustupInitUrl -or
            [string]$Metadata.rustupInitSha256 -cnotmatch
                '^[a-f0-9]{64}$' -or
            [string]$Metadata.rustupVersion -cnotmatch '^\d+\.\d+\.\d+' -or
            [string]$Metadata.rustcVersion -cnotmatch '^\d+\.\d+\.\d+' -or
            [string]$Metadata.rustcCommit -cnotmatch '^[a-f0-9]{40}$' -or
            [string]$Metadata.cargoVersion -cnotmatch '^\d+\.\d+\.\d+' -or
            [string]$Metadata.rustfmtVersion -cnotmatch '^\d+\.\d+\.\d+' -or
            [string]$Metadata.sourceVerification -cne
                'rust-static-sha256') {
            return $null
        }

        $MetadataComponents = [string[]]@($Metadata.components)
        $RequiredComponents = [string[]]$Definition.RequiredComponents
        if ($MetadataComponents.Count -ne $RequiredComponents.Count) {
            return $null
        }
        for ($Index = 0; $Index -lt $RequiredComponents.Count; $Index++) {
            if ($MetadataComponents[$Index] -cne $RequiredComponents[$Index]) {
                return $null
            }
        }

        $RequiredPaths = Get-ProjDevRustRequiredPaths `
            -ToolchainName ([string]$Definition.ToolchainName) `
            -HostTriple ([string]$Definition.Host) `
            -RequiredComponents $RequiredComponents
        $Records = @($Metadata.files)
        if ($Records.Count -le $RequiredPaths.Count) {
            return $null
        }
        $Seen = [Collections.Generic.HashSet[string]]::new(
            [StringComparer]::Ordinal
        )
        $RustupRecord = $null
        foreach ($Record in $Records) {
            $RelativePath = [string]$Record.path
            if ([string]::IsNullOrWhiteSpace($RelativePath) -or
                -not $Seen.Add($RelativePath) -or
                [string]$Record.kind -cnotmatch '^(file|symlink)$' -or
                [string]$Record.sha256 -cnotmatch '^[a-f0-9]{64}$') {
                return $null
            }
            $CurrentShape = Get-ProjDevRustInstallFileShape `
                -InstallRoot $InstallRoot `
                -RelativePath $RelativePath
            if ([string]$CurrentShape.kind -cne [string]$Record.kind -or
                [string]$CurrentShape.target -cne [string]$Record.target -or
                [long]$CurrentShape.length -ne [long]$Record.length) {
                return $null
            }
            if ($RelativePath -ceq 'cargo\bin\rustup.exe') {
                $RustupRecord = $Record
            }
        }
        foreach ($RelativePath in $RequiredPaths) {
            if (-not $Seen.Contains($RelativePath)) {
                return $null
            }
        }
        $CurrentPaths = @(Get-ProjDevRustInventoryPaths `
            -InstallRoot $InstallRoot `
            -Definition $Definition)
        if ($CurrentPaths.Count -ne $Seen.Count) {
            return $null
        }
        foreach ($RelativePath in $CurrentPaths) {
            if (-not $Seen.Contains($RelativePath)) {
                return $null
            }
        }
        if ($null -eq $RustupRecord -or
            [string]$RustupRecord.sha256 -cne
                [string]$Metadata.rustupInitSha256) {
            return $null
        }
        return $Metadata
    } catch {
        return $null
    }
}

function Test-ProjDevRustInstalled {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [AllowNull()][string]$InstallRoot = $null
    )

    if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
        $InstallRoot = Get-ProjDevRustInstallRoot `
            -Context $Context `
            -Definition $Definition
    }
    $Metadata = Get-ProjDevRustValidMetadata `
        -Context $Context `
        -Definition $Definition `
        -InstallRoot $InstallRoot
    if ($null -eq $Metadata) {
        return $false
    }
    try {
        foreach ($Record in @($Metadata.files)) {
            $Path = Resolve-ProjDevChildPath `
                -Root $InstallRoot `
                -RelativePath ([string]$Record.path) `
                -Description 'Rust installed file'
            if ((Get-ProjDevFileSha256 -Path $Path) -cne
                [string]$Record.sha256) {
                return $false
            }
        }
        return $true
    } catch {
        return $false
    }
}
