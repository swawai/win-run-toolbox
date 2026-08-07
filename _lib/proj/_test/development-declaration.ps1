[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\_lib\declaration.ps1')

function Assert-ProjDevelopmentDeclarationTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "Development declaration test failed: $Message"
    }
}

$Descriptors = @(Get-ProjDevelopmentModuleDeclarationDescriptors)
$VariableNames = [Collections.Generic.SortedSet[string]]::new(
    [StringComparer]::Ordinal
)
foreach ($Descriptor in $Descriptors) {
    [void]$VariableNames.Add([string]$Descriptor.Mode)
    foreach ($Setting in @($Descriptor.Settings)) {
        [void]$VariableNames.Add([string]$Setting.Name)
    }
}
$SavedEnvironment = @{}
foreach ($Name in $VariableNames) {
    $SavedEnvironment[$Name] = [Environment]::GetEnvironmentVariable(
        $Name,
        [EnvironmentVariableTarget]::Process
    )
    [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
}

try {
    $ByName = @{}
    foreach ($Descriptor in $Descriptors) {
        $ByName[[string]$Descriptor.Name] = $Descriptor
    }
    foreach ($Name in @('go', 'python', 'uv')) {
        Assert-ProjDevelopmentDeclarationTest `
            -Condition ($ByName.ContainsKey($Name) -and
                -not [bool]$ByName[$Name].SetupImplemented -and
                -not [IO.File]::Exists((Join-Path `
                    $script:ProjDevelopmentModuleRoot `
                    "$Name\module.ps1"))) `
            -Message "$Name is not a manifest-only pending module"
    }

    $Applied = Get-ProjDevelopmentDeclarationSnapshot
    foreach ($ModeVariable in @(
        'SWAWKIT_PROJ_GO_MODE',
        'SWAWKIT_PROJ_PYTHON_MODE',
        'SWAWKIT_PROJ_UV_MODE'
    )) {
        Assert-ProjDevelopmentDeclarationTest `
            -Condition ([string]$Applied[$ModeVariable] -ceq 'disabled') `
            -Message "$ModeVariable was omitted from the disabled snapshot"
    }

    $env:SWAWKIT_PROJ_GO_MODE = ' MANAGED '
    $env:SWAWKIT_PROJ_GO_VERSION = '1.22.4'
    $env:SWAWKIT_PROJ_PYTHON_MODE = 'uv'
    $env:SWAWKIT_PROJ_PYTHON_VERSION = '3.13'
    $env:SWAWKIT_PROJ_UV_MODE = 'managed'
    $env:SWAWKIT_PROJ_UV_VERSION = '0.10.2'
    $Declared = Get-ProjDevelopmentDeclarationSnapshot

    Assert-ProjDevelopmentDeclarationTest `
        -Condition (
            [string]$Declared.SWAWKIT_PROJ_GO_MODE -ceq 'managed' -and
            [string]$Declared.SWAWKIT_PROJ_GO_VERSION -ceq '1.22.4' -and
            [string]$Declared.SWAWKIT_PROJ_PYTHON_MODE -ceq 'uv' -and
            [string]$Declared.SWAWKIT_PROJ_PYTHON_VERSION -ceq '3.13' -and
            [string]$Declared.SWAWKIT_PROJ_UV_MODE -ceq 'managed' -and
            [string]$Declared.SWAWKIT_PROJ_UV_VERSION -ceq '0.10.2'
        ) `
        -Message 'pending module declarations were not captured by their manifests'

    $Pending = @(
        Get-ProjPendingDevelopmentSetupModuleNames -Declarations $Declared
    )
    Assert-ProjDevelopmentDeclarationTest `
        -Condition ([string]::Join(',', $Pending) -ceq 'go,python,uv') `
        -Message 'the setup fence was not derived from module manifests'

    $Differences = @(
        Compare-ProjDevelopmentDeclarations `
            -Applied $Applied `
            -Declared $Declared
    )
    $ChangedNames = [string[]]@($Differences | ForEach-Object Name)
    foreach ($ModeVariable in @(
        'SWAWKIT_PROJ_GO_MODE',
        'SWAWKIT_PROJ_PYTHON_MODE',
        'SWAWKIT_PROJ_UV_MODE'
    )) {
        Assert-ProjDevelopmentDeclarationTest `
            -Condition ($ChangedNames -ccontains $ModeVariable) `
            -Message "$ModeVariable disabled-to-enabled change did not stale state"
    }

    Write-Host '[PASS] Proj development declaration test' `
        -ForegroundColor Green
} finally {
    foreach ($Name in $VariableNames) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            $SavedEnvironment[$Name],
            [EnvironmentVariableTarget]::Process
        )
    }
}

$global:LASTEXITCODE = 0
