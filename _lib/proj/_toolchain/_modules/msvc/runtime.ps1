Set-StrictMode -Version 2.0

$SetupRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
foreach ($RelativePath in @(
    '_lib\runtime.ps1',
    '_modules\msvc\module.ps1',
    '_modules\msvc\environment.ps1',
    '_modules\msvc\command.ps1'
)) {
    . (Join-Path $SetupRoot $RelativePath)
}

function Get-ProjDevMsvcCommandRequirement {
    $Context = New-ProjDevContextFromEnvironment
    $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
    $Definition = Get-ProjDevMsvcDefinition
    if ($null -eq $Definition) {
        throw (
            'This command requires project-managed MSVC. Enable ' +
            'SWAWKIT_PROJ_MSVC_MODE and run ' +
            "'$Repair'."
        )
    }
    return [pscustomobject][ordered]@{
        Context = $Context
        Definition = $Definition
    }
}

function Import-ProjDevMsvcCommandEnvironment {
    $Requirement = Get-ProjDevMsvcCommandRequirement
    try {
        Import-ProjDevGeneratedEnvironment `
            -Context $Requirement.Context | Out-Null
        Assert-ProjDevWindowsX64 -ToolName 'Managed MSVC command'
        Assert-ProjDevMsvcReady `
            -Context $Requirement.Context `
            -Definition $Requirement.Definition
        Assert-ProjDevMsvcEnvironmentCurrent `
            -Context $Requirement.Context `
            -Definition $Requirement.Definition
    } finally {
        Clear-ProjDevSetupExportMetadata
    }
    return $Requirement
}
