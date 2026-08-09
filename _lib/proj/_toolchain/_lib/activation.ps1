Set-StrictMode -Version 2.0

$script:ProjDevSetupExportVariablePrefix =
    'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_'
$script:ProjDevSetupExportRevisionVariable =
    'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_EXPORT_REVISION'

function Get-ProjDevGeneratedEnvironmentRevision {
    param([Parameter(Mandatory = $true)][object]$Context)

    [void](Get-ProjRequiredCommandExport `
        -DataRoot ([string]$Context.DataRoot) `
        -ProviderAddress ([string]$Context.EnvironmentProviderAddress) `
        -EntryCommand ([string]$Context.EntryCommand))
    # Command modules own declaration freshness. Shared activation only proves
    # that the environment publication is complete and internally consistent.
    $Revision = Get-ProjPublishedDevelopmentEnvironmentRevision `
        -Context $Context
    if ($null -eq $Revision) {
        $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
        throw (
            'The project development environment is not configured. Run ' +
            "'$Repair'."
        )
    }
    return $Revision
}

function Clear-ProjDevSetupExportMetadata {
    $ProcessEnvironment = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    foreach ($Name in [string[]]@($ProcessEnvironment.Keys)) {
        if ($Name.StartsWith(
            $script:ProjDevSetupExportVariablePrefix,
            [StringComparison]::OrdinalIgnoreCase
        )) {
            [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
        }
    }
}

function Assert-ProjDevLoadedEnvironmentRevision {
    param([Parameter(Mandatory = $true)][string]$Revision)

    $LoadedRevision = [Environment]::GetEnvironmentVariable(
        $script:ProjDevSetupExportRevisionVariable,
        [EnvironmentVariableTarget]::Process
    )
    if ([string]$LoadedRevision -cne $Revision) {
        throw (
            'The loaded development environment does not match its published ' +
            'export revision.'
        )
    }
}

function Import-ProjDevGeneratedEnvironment {
    param([Parameter(Mandatory = $true)][object]$Context)

    $Revision = Get-ProjDevGeneratedEnvironmentRevision -Context $Context
    Clear-ProjDevSetupExportMetadata
    . $Context.EnvPs1Path
    Assert-ProjDevLoadedEnvironmentRevision -Revision $Revision

    return [pscustomobject][ordered]@{
        Revision = $Revision
    }
}

function Import-ProjDevOptionalGeneratedEnvironment {
    param([Parameter(Mandatory = $true)][object]$Context)

    $Revision = Get-ProjDevelopmentEnvironmentRevision -Context $Context
    if ($null -eq $Revision) {
        $Enabled = @(
            Get-ProjEnabledDevelopmentDeclarationNames `
                -Declarations (Get-ProjDevelopmentDeclarationSnapshot)
        )
        if ($Enabled.Count -gt 0) {
            [void](Get-ProjRequiredCommandExport `
                -DataRoot ([string]$Context.DataRoot) `
                -ProviderAddress ([string]$Context.EnvironmentProviderAddress) `
                -EntryCommand ([string]$Context.EntryCommand))
            $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
            throw (
                'The project declares managed development tools, but no ' +
                'environment has been published. Enabled: ' +
                [string]::Join(', ', $Enabled) + '. Run ' +
                "'$Repair'."
            )
        }
        return $false
    }

    Clear-ProjDevSetupExportMetadata
    try {
        . $Context.EnvPs1Path
        Assert-ProjDevLoadedEnvironmentRevision -Revision $Revision
    } finally {
        Clear-ProjDevSetupExportMetadata
    }
    return $true
}
