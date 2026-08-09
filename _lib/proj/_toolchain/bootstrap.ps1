Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot 'bootstrap-layout.ps1')
. (Join-Path $PSScriptRoot '_lib\runtime.ps1')
foreach ($File in @(
    'artifact.ps1',
    'recovery.ps1',
    'install.ps1',
    'environment.ps1'
)) {
    . (Join-Path (Join-Path $PSScriptRoot '_lib') $File)
}
$ModuleRoot = Join-Path $PSScriptRoot '_modules'
foreach ($File in @(
    'msvc\module.ps1',
    'msvc\payload.ps1',
    'msvc\manifest.ps1',
    'msvc\install.ps1',
    'msvc\environment.ps1',
    'rust\module.ps1',
    'rust\metadata.ps1',
    'rust\state.ps1',
    'rust\release.ps1',
    'rust\process.ps1',
    'rust\install.ps1',
    'rust\environment.ps1'
)) {
    . (Join-Path $ModuleRoot $File)
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
        DataRoot = $DataRoot
        CacheDataRoot = $CacheRoot
        EnvironmentRoot = $ToolchainRoot
        EnvironmentRepairInvocation = $Layout.BootstrapEntryPath
        EnvCmdPath = Join-Path $ToolchainRoot 'env.cmd'
        EnvPs1Path = Join-Path $ToolchainRoot 'env.ps1'
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
        [Parameter(Mandatory = $true)][string]$Revision
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
        schema = 'swawkit.proj-bootstrap-state/v2'
        contract = [ordered]@{
            schema = [string]$Contract.Schema
            rustToolchain = [string]$Contract.RustToolchain
            msvcChannel = [string]$Contract.MsvcChannel
        }
        environmentRevision = $Revision
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
    $RevisionVariable =
        'SWAWKIT_PROJ_TOOLCHAIN_BOOTSTRAP_ENVIRONMENT_REVISION'
    $Contract = Read-ProjBootstrapContract
    $Context = New-ProjBootstrapToolchainContext
    $MsvcDefinition = New-ProjDevMsvcDefinition `
        -Channel ([string]$Contract.MsvcChannel)
    $RustDefinition = New-ProjDevRustDefinition `
        -Toolchain ([string]$Contract.RustToolchain) `
        -Profile 'minimal' `
        -HostTriple 'x86_64-pc-windows-msvc'

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
        $Plan = New-ProjDevEnvironmentPlan
        Add-ProjDevMsvcEnvironment `
            -Context $Context `
            -Definition $MsvcDefinition `
            -Plan $Plan
        Add-ProjDevRustEnvironment `
            -Context $Context `
            -Definition $RustDefinition `
            -Plan $Plan
        $Scripts = ConvertTo-ProjDevEnvironmentScripts `
            -Plan $Plan `
            -RevisionVariable $RevisionVariable
        [void](Publish-ProjDevEnvironmentScripts `
            -Context $Context `
            -Scripts $Scripts)
        Write-ProjBootstrapToolchainState `
            -Context $Context `
            -Contract $Contract `
            -MsvcDefinition $MsvcDefinition `
            -RustDefinition $RustDefinition `
            -Revision ([string]$Scripts.Revision)
    } finally {
        $SetupLock.Dispose()
    }

    [Environment]::SetEnvironmentVariable(
        $RevisionVariable,
        $null,
        [EnvironmentVariableTarget]::Process
    )
    try {
        . $Context.EnvPs1Path
        Assert-ProjDevLoadedEnvironmentRevision `
            -Revision ([string]$Scripts.Revision) `
            -VariableName $RevisionVariable
        Assert-ProjDevMsvcEnvironmentCurrent `
            -Context $Context `
            -Definition $MsvcDefinition
        Assert-ProjDevRustEnvironmentCurrent `
            -Context $Context `
            -Definition $RustDefinition
    } finally {
        [Environment]::SetEnvironmentVariable(
            $RevisionVariable,
            $null,
            [EnvironmentVariableTarget]::Process
        )
    }

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
    $MsvcRoot = Get-ProjDevMsvcInstallRoot `
        -Context $Context `
        -Definition $MsvcDefinition
    $CompilerPath = Resolve-ProjBootstrapMsvcExecutable `
        -Name 'cl.exe' `
        -ManagedRoot $MsvcRoot
    $LinkerPath = Resolve-ProjBootstrapMsvcExecutable `
        -Name 'link.exe' `
        -ManagedRoot $MsvcRoot
    return [pscustomobject][ordered]@{
        Context = $Context
        Contract = $Contract
        MsvcDefinition = $MsvcDefinition
        RustDefinition = $RustDefinition
        CargoPath = $CargoPath
        CompilerPath = $CompilerPath
        LinkerPath = $LinkerPath
    }
}

function Resolve-ProjBootstrapMsvcExecutable {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$ManagedRoot
    )

    $Command = Get-Command $Name `
        -CommandType Application `
        -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $Command) {
        throw "The Bootstrap managed MSVC environment does not expose $Name."
    }
    $RootPrefix = [IO.Path]::GetFullPath($ManagedRoot).TrimEnd('\', '/') +
        [IO.Path]::DirectorySeparatorChar
    $ExecutablePath = [IO.Path]::GetFullPath([string]$Command.Source)
    if (-not $ExecutablePath.StartsWith(
        $RootPrefix,
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw "$Name resolved outside Bootstrap managed MSVC: $ExecutablePath"
    }
    return $ExecutablePath
}

function Invoke-ProjBootstrapToolchain {
    param([Parameter(Mandatory = $true)][scriptblock]$Action)

    $Snapshot = [Collections.Generic.Dictionary[string, string]]::new(
        [StringComparer]::Ordinal
    )
    $Before = [Environment]::GetEnvironmentVariables('Process')
    foreach ($Name in [string[]]@($Before.Keys)) {
        $Snapshot[$Name] = [string]$Before[$Name]
    }
    try {
        $Toolchain = Initialize-ProjBootstrapToolchain
        & $Action $Toolchain (Get-ProjBootstrapLayout)
    } finally {
        $After = [Environment]::GetEnvironmentVariables('Process')
        foreach ($Name in [string[]]@($After.Keys)) {
            if (-not $Snapshot.ContainsKey($Name)) {
                [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
            }
        }
        foreach ($Pair in $Snapshot.GetEnumerator()) {
            $Current = [Environment]::GetEnvironmentVariable(
                [string]$Pair.Key,
                'Process'
            )
            if ([string]$Current -cne [string]$Pair.Value) {
                [Environment]::SetEnvironmentVariable(
                    [string]$Pair.Key,
                    [string]$Pair.Value,
                    'Process'
                )
            }
        }
    }
}
