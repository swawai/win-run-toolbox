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
    Assert-ProjDevWindowsX64 -ToolName 'Managed MSVC command'
    return [pscustomobject][ordered]@{
        Context = $Context
        Definition = $Definition
    }
}

function Assert-ProjDevMsvcCommandReady {
    $Requirement = Get-ProjDevMsvcCommandRequirement
    $GenerationId = Get-ProjDevGeneratedEnvironmentGeneration `
        -Context $Requirement.Context
    Assert-ProjDevMsvcReady `
        -Context $Requirement.Context `
        -Definition $Requirement.Definition
    return [pscustomobject][ordered]@{
        Context = $Requirement.Context
        Definition = $Requirement.Definition
        GenerationId = $GenerationId
    }
}

function Import-ProjDevMsvcCommandEnvironment {
    $Requirement = Assert-ProjDevMsvcCommandReady
    $AlreadyActive = Assert-ProjDevActiveEnvironmentCompatible `
        -Context $Requirement.Context
    if (-not $AlreadyActive) {
        Clear-ProjDevProcessEnvironmentVariables
        . $Requirement.Context.EnvPs1Path
    }
    Assert-ProjDevActivatedEnvironmentIdentity `
        -Context $Requirement.Context `
        -GenerationId ([string]$Requirement.GenerationId)
    Assert-ProjDevMsvcEnvironmentCurrent `
        -Context $Requirement.Context `
        -Definition $Requirement.Definition
    return $Requirement
}
