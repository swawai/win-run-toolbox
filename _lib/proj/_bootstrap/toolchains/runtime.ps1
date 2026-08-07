Set-StrictMode -Version 2.0

$BootstrapRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $BootstrapRoot '_lib\layout.ps1')
$Layout = Get-ProjBootstrapLayout
. (Join-Path $Layout.KernelRoot '.dev\setup\_lib\bootstrap.ps1')

function Set-ProjBootstrapToolchainDeclarations {
    param([Parameter(Mandatory = $true)][object]$Contract)

    foreach ($Module in @(Get-ProjDevelopmentModuleDeclarationDescriptors)) {
        [Environment]::SetEnvironmentVariable(
            [string]$Module.Mode,
            'disabled',
            [EnvironmentVariableTarget]::Process
        )
        foreach ($Setting in @($Module.Settings)) {
            [Environment]::SetEnvironmentVariable(
                [string]$Setting.Name,
                $null,
                [EnvironmentVariableTarget]::Process
            )
        }
    }
    $Declarations = [ordered]@{
        SWAWKIT_PROJ_MSVC_MODE = 'managed'
        SWAWKIT_PROJ_MSVC_CHANNEL = [string]$Contract.MsvcChannel
        SWAWKIT_PROJ_RUST_MODE = 'rustup'
        SWAWKIT_PROJ_RUST_TOOLCHAIN = [string]$Contract.RustToolchain
        SWAWKIT_PROJ_RUST_PROFILE = 'minimal'
        SWAWKIT_PROJ_RUST_HOST = 'x86_64-pc-windows-msvc'
    }
    foreach ($Pair in $Declarations.GetEnumerator()) {
        [Environment]::SetEnvironmentVariable(
            [string]$Pair.Key,
            [string]$Pair.Value,
            [EnvironmentVariableTarget]::Process
        )
    }
}

function New-ProjBootstrapToolchainContext {
    $Layout = Get-ProjBootstrapLayout
    $DataRoot = Assert-ProjDevControlledRoot `
        -Root $Layout.BootstrapDataRoot `
        -Description 'Bootstrap data root'
    $CacheRoot = Assert-ProjDevControlledRoot `
        -Root $Layout.CacheRoot `
        -Description 'Shared project cache root'
    $ToolchainRoot = Assert-ProjDevelopmentEnvironmentControlledRoot `
        -EnvironmentRoot $Layout.ToolchainRoot
    return [pscustomobject][ordered]@{
        ProjectRoot = $Layout.ProjHome
        CanonicalProjectRoot = Get-ProjDevCanonicalPath -Path $Layout.ProjHome
        DataRoot = $DataRoot
        CacheDataRoot = $CacheRoot
        EnvironmentRoot = $ToolchainRoot
        EnvCmdPath = Join-Path $ToolchainRoot 'env.cmd'
        EnvPs1Path = Join-Path $ToolchainRoot 'env.ps1'
        EnvironmentStatePath = Join-Path $ToolchainRoot '_state.json'
        CacheRoot = Join-Path $CacheRoot 'downloads'
        LockRoot = $Layout.LockRoot
        SetupLockPath = Join-Path $Layout.LockRoot 'toolchain-setup.lock'
        ArtifactLockRoot = Join-Path $CacheRoot '_locks'
        EntryCommand = 'Swaw Kit Proj Bootstrap'
        InvocationDirectory = $Layout.AppRoot
    }
}

function Write-ProjBootstrapToolchainState {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Contract,
        [Parameter(Mandatory = $true)][object]$MsvcDefinition,
        [Parameter(Mandatory = $true)][object]$RustDefinition,
        [Parameter(Mandatory = $true)][string]$GenerationId
    )

    $Msvc = Get-ProjDevMsvcValidMetadata `
        -Context $Context `
        -Definition $MsvcDefinition
    $Rust = Get-ProjDevRustValidMetadata `
        -Context $Context `
        -Definition $RustDefinition
    if ($null -eq $Msvc -or $null -eq $Rust) {
        throw 'Cannot record an invalid Bootstrap toolchain.'
    }
    $State = [ordered]@{
        schema = 'swawkit.proj-bootstrap-state/v1'
        contract = [ordered]@{
            schema = [string]$Contract.Schema
            rustToolchain = [string]$Contract.RustToolchain
            msvcChannel = [string]$Contract.MsvcChannel
        }
        environmentGeneration = $GenerationId
        rust = [ordered]@{
            rustcVersion = [string]$Rust.rustcVersion
            cargoVersion = [string]$Rust.cargoVersion
            rustfmtVersion = [string]$Rust.rustfmtVersion
            components = [string[]]$Rust.components
            rustcCommit = [string]$Rust.rustcCommit
        }
        msvc = [ordered]@{
            toolVersion = [string]$Msvc.toolVersion
            sdkVersion = [string]$Msvc.sdkVersion
            manifestSha256 = [string]$Msvc.manifestSha256
        }
    }
    $Content = ConvertTo-ProjDevJsonText -Value $State
    $Layout = Get-ProjBootstrapLayout
    $Current = if ([IO.File]::Exists($Layout.StatePath)) {
        [IO.File]::ReadAllText($Layout.StatePath, [Text.Encoding]::UTF8)
    } else {
        $null
    }
    if ($Current -cne $Content) {
        Write-ProjDevTextAtomic `
            -Path $Layout.StatePath `
            -Content $Content `
            -ControlledRoot $Context.DataRoot
    }
}

function Initialize-ProjBootstrapToolchain {
    $Contract = Read-ProjBootstrapContract
    Set-ProjBootstrapToolchainDeclarations -Contract $Contract
    $Context = New-ProjBootstrapToolchainContext
    $MsvcDefinition = Get-ProjDevMsvcDefinition
    $RustDefinition = Get-ProjDevRustDefinition
    if ($null -eq $MsvcDefinition -or $null -eq $RustDefinition) {
        throw 'The Bootstrap Rust and MSVC declarations are incomplete.'
    }

    $SetupLock = Enter-ProjDevFileLock `
        -Path $Context.SetupLockPath `
        -ControlledRoot $Context.DataRoot `
        -TimeoutSeconds 1800
    try {
        [void](Install-ProjDevMsvc `
            -Context $Context `
            -Definition $MsvcDefinition)
        [void](Install-ProjDevRust `
            -Context $Context `
            -Definition $RustDefinition)
        $Plan = New-ProjDevEnvironmentPlan -Context $Context
        Add-ProjDevMsvcEnvironment `
            -Context $Context `
            -Definition $MsvcDefinition `
            -Plan $Plan
        Add-ProjDevRustEnvironment `
            -Context $Context `
            -Definition $RustDefinition `
            -Plan $Plan
        $Scripts = ConvertTo-ProjDevEnvironmentScripts -Plan $Plan
        [void](Publish-ProjDevEnvironmentScripts `
            -Context $Context `
            -Scripts $Scripts)
        Write-ProjBootstrapToolchainState `
            -Context $Context `
            -Contract $Contract `
            -MsvcDefinition $MsvcDefinition `
            -RustDefinition $RustDefinition `
            -GenerationId ([string]$Scripts.GenerationId)
    } finally {
        $SetupLock.Dispose()
    }

    Clear-ProjDevProcessEnvironmentVariables
    . $Context.EnvPs1Path
    Assert-ProjDevActivatedEnvironmentIdentity `
        -Context $Context `
        -GenerationId ([string]$Scripts.GenerationId)
    Assert-ProjDevMsvcEnvironmentCurrent `
        -Context $Context `
        -Definition $MsvcDefinition
    Assert-ProjDevRustEnvironmentCurrent `
        -Context $Context `
        -Definition $RustDefinition

    $RustRoot = Get-ProjDevRustInstallRoot `
        -Context $Context `
        -Definition $RustDefinition
    $CargoPath = Resolve-ProjDevChildPath `
        -Root $RustRoot `
        -RelativePath (
            "rustup\toolchains\$($RustDefinition.ToolchainName)\bin\cargo.exe"
        ) `
        -Description 'Bootstrap Cargo executable'
    if (-not [IO.File]::Exists($CargoPath)) {
        throw "The Bootstrap Cargo executable is missing: $CargoPath"
    }
    return [pscustomobject][ordered]@{
        Context = $Context
        Contract = $Contract
        CargoPath = $CargoPath
    }
}
