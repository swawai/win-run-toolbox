Set-StrictMode -Version 2.0

$script:ProjDevelopmentEnvironmentStateSchema =
    'swawkit.proj-dev.environment-state.v2'

$script:ProjDevelopmentModuleRoot = [IO.Path]::GetFullPath(
    (Join-Path $PSScriptRoot '..\_modules')
)

function Get-ProjDevelopmentModuleDeclarationDescriptors {
    if (-not [IO.Directory]::Exists($script:ProjDevelopmentModuleRoot)) {
        throw (
            'Development module directory is missing: ' +
            $script:ProjDevelopmentModuleRoot
        )
    }

    foreach ($Directory in @(Get-ChildItem `
        -LiteralPath $script:ProjDevelopmentModuleRoot `
        -Directory `
        -Force | Sort-Object Name)) {
        if (($Directory.Attributes -band
            [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Development module cannot be a reparse point: $($Directory.FullName)"
        }
        $ManifestPath = Join-Path $Directory.FullName 'module.psd1'
        if (-not [IO.File]::Exists($ManifestPath)) {
            throw "Development module manifest is missing: $ManifestPath"
        }
        $ManifestItem = Get-Item -LiteralPath $ManifestPath -Force
        if (($ManifestItem.Attributes -band
            [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Development module manifest cannot be a reparse point: $ManifestPath"
        }
        try {
            $Manifest = Import-PowerShellDataFile -LiteralPath $ManifestPath
        } catch {
            throw "Cannot read development module manifest '$ManifestPath': $($_.Exception.Message)"
        }
        $Name = [string]$Manifest.Name
        $ModeVariable = [string]$Manifest.ModeVariable
        if ([string]$Manifest.Schema -cne 'swawkit.proj-dev.module.v0' -or
            [string]::IsNullOrWhiteSpace($Name) -or
            $Name -cne $Directory.Name -or
            $Manifest.SetupImplemented -isnot [bool] -or
            $ModeVariable -cnotmatch '^SWAWKIT_PROJ_[A-Z0-9]+_MODE$') {
            throw "Invalid development module declaration manifest: $ManifestPath"
        }

        $Settings = [Collections.Generic.List[object]]::new()
        foreach ($PropertyName in @($Manifest.Keys | Where-Object {
            ([string]$_).EndsWith('Variable', [StringComparison]::Ordinal) -and
            [string]$_ -cne 'ModeVariable'
        } | Sort-Object)) {
            $VariableName = [string]$Manifest[$PropertyName]
            if ($VariableName -cnotmatch '^SWAWKIT_PROJ_[A-Z0-9_]+$') {
                throw "Invalid declaration variable in '$ManifestPath': $VariableName"
            }
            $Normalization = if (
                ([string]$PropertyName).EndsWith(
                    'HashVariable',
                    [StringComparison]::Ordinal
                )
            ) {
                'hash'
            } else {
                'literal'
            }
            $Settings.Add([pscustomobject][ordered]@{
                Name = $VariableName
                Normalization = $Normalization
            })
        }
        [pscustomobject][ordered]@{
            Name = $Name
            Mode = $ModeVariable
            SetupImplemented = [bool]$Manifest.SetupImplemented
            Settings = $Settings.ToArray()
        }
    }
}

function Get-ProjDevelopmentDeclarationValue {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)]
        [ValidateSet('mode', 'hash', 'literal')]
        [string]$Normalization
    )

    $Value = [string][Environment]::GetEnvironmentVariable(
        $Name,
        [EnvironmentVariableTarget]::Process
    )
    $Value = $Value.Trim()
    if ($Normalization -in @('mode', 'hash')) {
        $Value = $Value.ToLowerInvariant()
    }
    if ($Normalization -ceq 'hash' -and $Value.StartsWith('sha256:')) {
        $Value = $Value.Substring('sha256:'.Length)
    }
    return $Value
}

function Get-ProjDevelopmentDeclarationSnapshot {
    $Snapshot = [ordered]@{}
    foreach ($Module in @(Get-ProjDevelopmentModuleDeclarationDescriptors)) {
        if ($Snapshot.Contains([string]$Module.Mode)) {
            throw "Duplicate development declaration: $($Module.Mode)"
        }
        $Mode = Get-ProjDevelopmentDeclarationValue `
            -Name $Module.Mode `
            -Normalization mode
        if ([string]::IsNullOrWhiteSpace($Mode)) {
            $Mode = 'disabled'
        }
        $Snapshot.Add([string]$Module.Mode, $Mode)
        if ($Mode -ceq 'disabled') {
            continue
        }
        foreach ($Setting in @($Module.Settings)) {
            $Name = [string]$Setting.Name
            if ($Snapshot.Contains($Name)) {
                throw "Duplicate development declaration: $Name"
            }
            $Snapshot.Add(
                $Name,
                (Get-ProjDevelopmentDeclarationValue `
                    -Name $Name `
                    -Normalization ([string]$Setting.Normalization))
            )
        }
    }

    return $Snapshot
}

function Get-ProjDevelopmentEnvironmentStatePath {
    param([Parameter(Mandatory = $true)][string]$EnvironmentRoot)

    return Join-Path $EnvironmentRoot '_state.json'
}

function Get-ProjEnabledDevelopmentDeclarationNames {
    param(
        [Parameter(Mandatory = $true)]
        [Collections.IDictionary]$Declarations
    )

    foreach ($Name in @($Declarations.Keys | Sort-Object)) {
        $Match = [regex]::Match(
            [string]$Name,
            '^SWAWKIT_PROJ_([A-Z0-9]+)_MODE$'
        )
        if ($Match.Success -and
            [string]$Declarations[$Name] -cne 'disabled') {
            $Match.Groups[1].Value.ToLowerInvariant()
        }
    }
}

function Get-ProjPendingDevelopmentSetupModuleNames {
    param(
        [Parameter(Mandatory = $true)]
        [Collections.IDictionary]$Declarations
    )

    foreach ($Module in @(Get-ProjDevelopmentModuleDeclarationDescriptors)) {
        if (-not $Declarations.Contains([string]$Module.Mode)) {
            throw "Development declaration is missing: $($Module.Mode)"
        }
        if (-not [bool]$Module.SetupImplemented -and
            [string]$Declarations[[string]$Module.Mode] -cne 'disabled') {
            [string]$Module.Name
        }
    }
}

function New-ProjDevelopmentEnvironmentState {
    param(
        [Parameter(Mandatory = $true)][string]$Revision,
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][string]$ExportRoot,
        [Parameter(Mandatory = $true)]
        [Collections.IDictionary]$Declarations
    )

    if ($Revision -cnotmatch '^[a-f0-9]{16}$') {
        throw "Invalid development environment export revision: $Revision"
    }
    return [ordered]@{
        schema = $script:ProjDevelopmentEnvironmentStateSchema
        revision = $Revision
        projectRoot = Get-ProjDevFullPath -Path $ProjectRoot
        exportRoot = Get-ProjDevFullPath -Path $ExportRoot
        declarations = $Declarations
    }
}

function Read-ProjDevelopmentEnvironmentState {
    param([Parameter(Mandatory = $true)][string]$EnvironmentRoot)

    $Path = Get-ProjDevelopmentEnvironmentStatePath `
        -EnvironmentRoot $EnvironmentRoot
    if (-not [IO.File]::Exists($Path)) {
        return $null
    }
    try {
        $State = [IO.File]::ReadAllText(
            $Path,
            [Text.Encoding]::UTF8
        ) | ConvertFrom-Json
    } catch {
        throw (
            'Cannot parse the published development environment state: ' +
            $_.Exception.Message
        )
    }
    if ([string]$State.schema -cne
            $script:ProjDevelopmentEnvironmentStateSchema -or
        [string]$State.revision -cnotmatch '^[a-f0-9]{16}$' -or
        [string]::IsNullOrWhiteSpace([string]$State.projectRoot) -or
        -not [IO.Path]::IsPathRooted([string]$State.projectRoot) -or
        [string]::IsNullOrWhiteSpace([string]$State.exportRoot) -or
        -not [IO.Path]::IsPathRooted([string]$State.exportRoot) -or
        $null -eq $State.declarations) {
        throw "The published development environment state is invalid: $Path"
    }

    $Declarations = [ordered]@{}
    foreach ($Property in @($State.declarations.PSObject.Properties)) {
        $Name = [string]$Property.Name
        if ($Name -cnotmatch '^SWAWKIT_PROJ_[A-Z0-9_]+$' -or
            $Declarations.Contains($Name) -or
            $null -eq $Property.Value -or
            $Property.Value -isnot [string]) {
            throw "The published development environment state is invalid: $Path"
        }
        $Declarations.Add($Name, [string]$Property.Value)
    }
    return [pscustomobject][ordered]@{
        Path = $Path
        Revision = [string]$State.revision
        ProjectRoot = [string]$State.projectRoot
        ExportRoot = [string]$State.exportRoot
        Declarations = $Declarations
    }
}

function Compare-ProjDevelopmentDeclarations {
    param(
        [Parameter(Mandatory = $true)]
        [Collections.IDictionary]$Applied,
        [Parameter(Mandatory = $true)]
        [Collections.IDictionary]$Declared
    )

    $Names = [Collections.Generic.SortedSet[string]]::new(
        [StringComparer]::Ordinal
    )
    foreach ($Name in $Applied.Keys) {
        [void]$Names.Add([string]$Name)
    }
    foreach ($Name in $Declared.Keys) {
        [void]$Names.Add([string]$Name)
    }
    foreach ($Name in $Names) {
        $AppliedValue = if ($Applied.Contains($Name)) {
            [string]$Applied[$Name]
        } else {
            '<unset>'
        }
        $DeclaredValue = if ($Declared.Contains($Name)) {
            [string]$Declared[$Name]
        } else {
            '<unset>'
        }
        if ($AppliedValue -cne $DeclaredValue) {
            [pscustomobject][ordered]@{
                Name = $Name
                Applied = $AppliedValue
                Declared = $DeclaredValue
            }
        }
    }
}
