Set-StrictMode -Version 2.0

function New-ProjDevContext {
    param(
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$CacheDataRoot,
        [string]$EntryCommand = 'swawkit',
        [AllowNull()][string]$InvocationDirectory = $null,
        [AllowNull()][string]$EnvironmentInputRevision = $null,
        [AllowNull()][string]$CommandProfileRevision = $null
    )

    $ResolvedProjectRoot = Get-ProjDevFullPath -Path $ProjectRoot
    $ResolvedDataRoot = Assert-ProjDevControlledRoot `
        -Root $DataRoot `
        -Description 'Project development data root'
    $ResolvedCacheDataRoot = Assert-ProjDevControlledRoot `
        -Root $CacheDataRoot `
        -Description 'Shared project cache data root'
    if (-not [IO.Directory]::Exists($ResolvedProjectRoot)) {
        throw "Declared project directory does not exist: $ResolvedProjectRoot"
    }

    if ([string]::IsNullOrWhiteSpace($InvocationDirectory)) {
        $ResolvedInvocationDirectory = $ResolvedProjectRoot
    } else {
        $ResolvedInvocationDirectory = Get-ProjDevFullPath -Path $InvocationDirectory
    }
    if (-not [IO.Directory]::Exists($ResolvedInvocationDirectory)) {
        throw "Invocation directory does not exist: $ResolvedInvocationDirectory"
    }

    $EnvironmentProviderAddress = '.dev.setup'
    $SetupCommandRoot = Get-ProjKernelCommandDataRoot `
        -DataRoot $ResolvedDataRoot `
        -Address $EnvironmentProviderAddress
    $EnvironmentRoot = Resolve-ProjCommandExportPath `
        -DataRoot $ResolvedDataRoot `
        -ProviderAddress $EnvironmentProviderAddress
    $LockRoot = Assert-ProjDevPathInsideDataRoot `
        -Path (Join-Path $SetupCommandRoot 'locks') `
        -DataRoot $ResolvedDataRoot `
        -Activity 'resolving the development setup lock root'
    return [pscustomobject][ordered]@{
        ProjectRoot = $ResolvedProjectRoot
        DataRoot = $ResolvedDataRoot
        ProfilePath = Join-Path $ResolvedDataRoot '_profile.json'
        CacheDataRoot = $ResolvedCacheDataRoot
        EnvironmentRoot = $EnvironmentRoot
        EnvironmentProviderAddress = $EnvironmentProviderAddress
        EnvironmentInputRevision = $EnvironmentInputRevision
        CommandProfileRevision = $CommandProfileRevision
        SetupCommandRoot = $SetupCommandRoot
        ProviderStatePath = Join-Path $SetupCommandRoot '_state.json'
        EnvCmdPath = Join-Path $EnvironmentRoot 'env.cmd'
        EnvPs1Path = Join-Path $EnvironmentRoot 'env.ps1'
        LegacyEnvironmentStatePath = Join-Path $EnvironmentRoot '_state.json'
        CacheRoot = Join-Path $ResolvedCacheDataRoot 'downloads'
        LockRoot = $LockRoot
        SetupLockPath = Join-Path $LockRoot 'setup.lock'
        ProviderStateLockPath = Join-Path $LockRoot 'state.lock'
        ArtifactLockRoot = Join-Path $ResolvedCacheDataRoot '_locks'
        EntryCommand = $EntryCommand
        InvocationDirectory = $ResolvedInvocationDirectory
    }
}

function New-ProjDevContextFromEnvironment {
    if ([string]$env:SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL -cne '1') {
        throw (
            'Unsupported or missing SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL. ' +
            'Expected protocol version 1.'
        )
    }

    $InvocationDirectory = [Environment]::GetEnvironmentVariable(
        'SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR',
        [EnvironmentVariableTarget]::Process
    )
    $ProjHome = Get-ProjDevFullPath -Path (
        Get-ProjDevRequiredEnvironmentValue -Name 'SWAWKIT_HOME'
    )
    if (-not [IO.Directory]::Exists($ProjHome)) {
        throw "Declared Swaw Kit Proj home does not exist: $ProjHome"
    }
    return New-ProjDevContext `
        -ProjectRoot (Get-ProjDevRequiredEnvironmentValue -Name 'SWAWKIT_PROJ_TARGET_PROJECT_ROOT') `
        -DataRoot (Get-ProjDevRequiredEnvironmentValue -Name 'SWAWKIT_PROJ_DATA_ROOT') `
        -CacheDataRoot (Join-Path $ProjHome 'data\proj_cache') `
        -EntryCommand (Get-ProjDevRequiredEnvironmentValue -Name 'SWAWKIT_PROJ_ENTRY_COMMAND') `
        -InvocationDirectory $InvocationDirectory `
        -EnvironmentInputRevision (Get-ProjDevCommandEnvironmentInputRevision) `
        -CommandProfileRevision (Get-ProjDevCommandProfileRevision)
}
