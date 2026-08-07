Set-StrictMode -Version 2.0

function Get-ProjDevGeneratedEnvironmentGeneration {
    param([Parameter(Mandatory = $true)][object]$Context)

    # Command modules own declaration freshness. Shared activation only proves
    # that the environment publication is complete and internally consistent.
    $GenerationId = Get-ProjPublishedDevelopmentEnvironmentGeneration `
        -EnvironmentRoot $Context.EnvironmentRoot `
        -EntryCommand $Context.EntryCommand
    if ($null -eq $GenerationId) {
        throw (
            'The project development environment is not configured. Run ' +
            "'$($Context.EntryCommand) .dev.setup'."
        )
    }
    return $GenerationId
}

function Clear-ProjDevProcessEnvironmentVariables {
    $ProcessEnvironment = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    foreach ($Name in [string[]]@($ProcessEnvironment.Keys)) {
        if ($Name.StartsWith(
            'SWAWKIT_PROJ_DEV_',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
        }
    }
}

function Assert-ProjDevActivatedEnvironmentIdentity {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][string]$GenerationId
    )

    if ([string]$env:SWAWKIT_PROJ_DEV_GENERATION_ID -cne $GenerationId) {
        throw (
            'The active development environment generation is stale. ' +
            'Exit this shell and start a new project shell.'
        )
    }
    if ([string]$env:SWAWKIT_PROJ_DEV_ENV_SCHEMA -cne
        'swawkit.proj-dev.environment.v0') {
        throw "Unsupported generated environment schema. Run '.dev.setup'."
    }
    foreach ($Name in @(
        'SWAWKIT_PROJ_DEV_PROJECT_ROOT',
        'SWAWKIT_PROJ_DEV_ENV_ROOT'
    )) {
        if ([string]::IsNullOrWhiteSpace(
            [Environment]::GetEnvironmentVariable($Name, 'Process')
        )) {
            throw "Generated environment is missing $Name. Run '.dev.setup'."
        }
    }
    if (-not (Get-ProjDevCanonicalPath -Path (
            [string]$env:SWAWKIT_PROJ_DEV_PROJECT_ROOT
        )).Equals(
            $Context.CanonicalProjectRoot,
            [StringComparison]::OrdinalIgnoreCase
        ) -or
        -not (Get-ProjDevCanonicalPath -Path (
            [string]$env:SWAWKIT_PROJ_DEV_ENV_ROOT
        )).Equals(
            (Get-ProjDevCanonicalPath -Path $Context.EnvironmentRoot),
            [StringComparison]::OrdinalIgnoreCase
        )) {
        throw 'The generated environment belongs to another project or data root.'
    }
}

function Import-ProjDevGeneratedEnvironment {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [bool]$AlreadyActive = $false
    )

    $GenerationId = Get-ProjDevGeneratedEnvironmentGeneration `
        -Context $Context
    if (-not $AlreadyActive) {
        Clear-ProjDevProcessEnvironmentVariables
        . $Context.EnvPs1Path
    }
    Assert-ProjDevActivatedEnvironmentIdentity `
        -Context $Context `
        -GenerationId $GenerationId

    return [pscustomobject][ordered]@{
        GenerationId = $GenerationId
        ProjectRoot = [string]$env:SWAWKIT_PROJ_DEV_PROJECT_ROOT
        EnvironmentRoot = [string]$env:SWAWKIT_PROJ_DEV_ENV_ROOT
    }
}

function Assert-ProjDevActiveEnvironmentPublished {
    param([Parameter(Mandatory = $true)][object]$Context)

    if (-not (Assert-ProjDevActiveEnvironmentCompatible -Context $Context)) {
        return $false
    }
    $GenerationId = Get-ProjPublishedDevelopmentEnvironmentGeneration `
        -EnvironmentRoot $Context.EnvironmentRoot `
        -EntryCommand $Context.EntryCommand
    if ($null -eq $GenerationId) {
        throw (
            'A project development environment is active, but no environment ' +
            'is published. Exit this shell and run ' +
            "'$($Context.EntryCommand) .dev.setup'."
        )
    }
    Assert-ProjDevActivatedEnvironmentIdentity `
        -Context $Context `
        -GenerationId $GenerationId
    return $true
}

function Import-ProjDevOptionalGeneratedEnvironment {
    param([Parameter(Mandatory = $true)][object]$Context)

    $AlreadyActive = Assert-ProjDevActiveEnvironmentCompatible `
        -Context $Context
    $GenerationId = Get-ProjDevelopmentEnvironmentGeneration `
        -EnvironmentRoot $Context.EnvironmentRoot `
        -EntryCommand $Context.EntryCommand
    if ($null -eq $GenerationId) {
        if ($AlreadyActive) {
            throw (
                'A managed development environment is active, but this ' +
                'project has no published environment. Exit this shell.'
            )
        }
        $Enabled = @(
            Get-ProjEnabledDevelopmentDeclarationNames `
                -Declarations (Get-ProjDevelopmentDeclarationSnapshot)
        )
        if ($Enabled.Count -gt 0) {
            throw (
                'The project declares managed development tools, but no ' +
                'environment has been published. Enabled: ' +
                [string]::Join(', ', $Enabled) + '. Run ' +
                "'$($Context.EntryCommand) .dev.setup'."
            )
        }
        return $false
    }
    if (-not $AlreadyActive) {
        Clear-ProjDevProcessEnvironmentVariables
        . $Context.EnvPs1Path
    }
    Assert-ProjDevActivatedEnvironmentIdentity `
        -Context $Context `
        -GenerationId $GenerationId
    return $true
}
