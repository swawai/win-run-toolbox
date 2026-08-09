Set-StrictMode -Version 2.0

$script:ProjDevSetupExportVariablePrefix =
    'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_'

function Get-ProjDevGeneratedEnvironmentPublication {
    param([Parameter(Mandatory = $true)][object]$Context)

    $Publication = Get-ProjRequiredCommandExport `
        -DataRoot ([string]$Context.DataRoot) `
        -ProviderAddress ([string]$Context.EnvironmentProviderAddress) `
        -EntryCommand ([string]$Context.EntryCommand) `
        -InputRevision ([string]$Context.EnvironmentInputRevision) `
        -ProducerContract (Get-ProjDevSetupProducerContract)
    if (-not [IO.File]::Exists([string]$Context.EnvCmdPath) -or
        -not [IO.File]::Exists([string]$Context.EnvPs1Path)) {
        $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
        throw (
            'The development environment export is incomplete. Run ' +
            "'$Repair'."
        )
    }
    return $Publication
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

function Clear-ProjDevSetupPublicationMetadata {
    foreach ($Name in @(
        $script:ProjDevSetupPublicationTokenVariable,
        $script:ProjDevSetupExportRevisionVariable
    )) {
        [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
    }
}

function Assert-ProjDevLoadedEnvironmentPublication {
    param([Parameter(Mandatory = $true)][object]$Publication)

    $LoadedToken = [Environment]::GetEnvironmentVariable(
        $script:ProjDevSetupPublicationTokenVariable,
        [EnvironmentVariableTarget]::Process
    )
    if ([string]$LoadedToken -cne [string]$Publication.Token) {
        throw (
            'The loaded development environment does not match its ' +
            'published provider state.'
        )
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

    $Publication = Get-ProjDevGeneratedEnvironmentPublication `
        -Context $Context
    Clear-ProjDevSetupExportMetadata
    try {
        . $Context.EnvPs1Path
        Assert-ProjDevLoadedEnvironmentPublication `
            -Publication $Publication
        Assert-ProjCommandProviderPublicationCurrent `
            -Context $Context `
            -Publication $Publication
    } catch {
        Clear-ProjDevSetupExportMetadata
        throw
    }
    Clear-ProjDevSetupPublicationMetadata

    return [pscustomobject][ordered]@{
        Token = [string]$Publication.Token
    }
}

function Import-ProjDevOptionalGeneratedEnvironment {
    param([Parameter(Mandatory = $true)][object]$Context)

    Clear-ProjDevSetupExportMetadata
    $Declarations = Get-ProjDevelopmentDeclarationSnapshot
    Assert-ProjDevelopmentSetupDeclarationsSupported `
        -Declarations $Declarations
    $Enabled = @(
        Get-ProjEnabledDevelopmentDeclarationNames `
            -Declarations $Declarations
    )
    if ($Enabled.Count -eq 0) {
        return $false
    }
    try {
        [void](Import-ProjDevGeneratedEnvironment -Context $Context)
    } finally {
        Clear-ProjDevSetupExportMetadata
    }
    return $true
}
