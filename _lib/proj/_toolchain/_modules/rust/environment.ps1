Set-StrictMode -Version 2.0

function Get-ProjDevRustRuntimeSignature {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Metadata
    )

    return Get-ProjDevSha256Text -Value ([string]::Join("`n", [string[]]@(
        (Get-ProjDevRustDefinitionSignature -Definition $Definition)
        [string]$Metadata.rustupInitSha256
        [string]$Metadata.rustupVersion
        [string]$Metadata.rustcVersion
        [string]$Metadata.rustcCommit
        [string]$Metadata.cargoVersion
        [string]$Metadata.rustfmtVersion
        [string]::Join(',', [string[]]$Metadata.components)
    )))
}

function Add-ProjDevRustEnvironment {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Plan
    )

    $InstallRoot = Get-ProjDevRustInstallRoot `
        -Context $Context `
        -Definition $Definition
    $Metadata = Get-ProjDevRustValidMetadata `
        -Context $Context `
        -Definition $Definition
    if ($null -eq $Metadata) {
        throw 'Cannot generate an environment from an invalid Rust installation.'
    }
    $CargoHome = Join-Path $InstallRoot 'cargo'
    $RustupHome = Join-Path $InstallRoot 'rustup'
    $ToolchainBin = Join-Path $RustupHome (
        "toolchains\$($Definition.ToolchainName)\bin"
    )
    $Rustc = Join-Path $ToolchainBin 'rustc.exe'
    $Rustdoc = Join-Path $ToolchainBin 'rustdoc.exe'
    $Variables = [ordered]@{
        SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_MODE = [string]$Definition.Mode
        SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_TOOLCHAIN = [string]$Definition.Toolchain
        SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_TOOLCHAIN_NAME = [string]$Definition.ToolchainName
        SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_PROFILE = [string]$Definition.Profile
        SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_HOST = [string]$Definition.Host
        SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_RUSTC_VERSION = [string]$Metadata.rustcVersion
        SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_CARGO_VERSION = [string]$Metadata.cargoVersion
        SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_RUSTFMT_VERSION = `
            [string]$Metadata.rustfmtVersion
        SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_HOME = $InstallRoot
        SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_SIGNATURE = Get-ProjDevRustRuntimeSignature `
            -Definition $Definition `
            -Metadata $Metadata
        RUSTUP_HOME = $RustupHome
        CARGO_HOME = $CargoHome
        RUSTUP_TOOLCHAIN = [string]$Definition.ToolchainName
        RUSTC = $Rustc
        RUSTDOC = $Rustdoc
        CARGO_BUILD_RUSTC = $Rustc
        CARGO_BUILD_RUSTDOC = $Rustdoc
    }
    foreach ($Name in Get-ProjDevRustAmbientOverrideNames |
        Where-Object { $_ -cne 'RUSTUP_TOOLCHAIN' }) {
        $Variables[$Name] = $null
    }
    foreach ($Name in $Variables.Keys) {
        Set-ProjDevEnvironmentVariable `
            -Plan $Plan `
            -Name ([string]$Name) `
            -Value $Variables[$Name]
    }
    Add-ProjDevEnvironmentPath `
        -Plan $Plan `
        -Path (Join-Path $CargoHome 'bin')
}

function Assert-ProjDevRustEnvironmentCurrent {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $InstallRoot = Get-ProjDevRustInstallRoot `
        -Context $Context `
        -Definition $Definition
    $Metadata = Get-ProjDevRustValidMetadata `
        -Context $Context `
        -Definition $Definition
    if ($null -eq $Metadata) {
        throw 'The generated Rust environment has no valid installation.'
    }
    $ExpectedCargoHome = Join-Path $InstallRoot 'cargo'
    $ExpectedRustupHome = Join-Path $InstallRoot 'rustup'
    $ExpectedToolchainBin = Join-Path $ExpectedRustupHome (
        "toolchains\$($Definition.ToolchainName)\bin"
    )
    $ExpectedRustc = Join-Path $ExpectedToolchainBin 'rustc.exe'
    $ExpectedRustdoc = Join-Path $ExpectedToolchainBin 'rustdoc.exe'
    $ExpectedSignature = Get-ProjDevRustRuntimeSignature `
        -Definition $Definition `
        -Metadata $Metadata
    $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
    $ValuesMatch =
        [string]$env:SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_MODE -ceq
            [string]$Definition.Mode -and
        [string]$env:SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_TOOLCHAIN -ceq
            [string]$Definition.Toolchain -and
        [string]$env:SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_TOOLCHAIN_NAME -ceq
            [string]$Definition.ToolchainName -and
        [string]$env:SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_PROFILE -ceq
            [string]$Definition.Profile -and
        [string]$env:SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_HOST -ceq
            [string]$Definition.Host -and
        [string]$env:SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_RUST_SIGNATURE -ceq $ExpectedSignature -and
        [string]$env:RUSTUP_TOOLCHAIN -ceq
            [string]$Definition.ToolchainName
    if (-not $ValuesMatch) {
        throw (
            'The generated Rust environment does not match the project ' +
            "declaration. Run '$Repair'."
        )
    }
    foreach ($Name in Get-ProjDevRustAmbientOverrideNames |
        Where-Object { $_ -cne 'RUSTUP_TOOLCHAIN' }) {
        $Value = [Environment]::GetEnvironmentVariable($Name, 'Process')
        if (-not [string]::IsNullOrWhiteSpace($Value)) {
            throw "The generated Rust environment retained ambient $Name."
        }
    }
    foreach ($Pair in @(
        [pscustomobject]@{
            Actual = [string]$env:CARGO_HOME
            Expected = $ExpectedCargoHome
        }
        [pscustomobject]@{
            Actual = [string]$env:RUSTUP_HOME
            Expected = $ExpectedRustupHome
        }
        [pscustomobject]@{
            Actual = [string]$env:RUSTC
            Expected = $ExpectedRustc
        }
        [pscustomobject]@{
            Actual = [string]$env:CARGO_BUILD_RUSTC
            Expected = $ExpectedRustc
        }
        [pscustomobject]@{
            Actual = [string]$env:RUSTDOC
            Expected = $ExpectedRustdoc
        }
        [pscustomobject]@{
            Actual = [string]$env:CARGO_BUILD_RUSTDOC
            Expected = $ExpectedRustdoc
        }
    )) {
        if ([string]::IsNullOrWhiteSpace($Pair.Actual) -or
            -not (Get-ProjDevCanonicalPath -Path $Pair.Actual).Equals(
                (Get-ProjDevCanonicalPath -Path $Pair.Expected),
                [StringComparison]::OrdinalIgnoreCase
            )) {
            throw 'The generated Rust runtime paths are stale.'
        }
    }
    $CargoCommand = Get-Command cargo.exe `
        -CommandType Application `
        -ErrorAction SilentlyContinue |
        Select-Object -First 1
    $ExpectedCargo = Join-Path $ExpectedCargoHome 'bin\cargo.exe'
    if ($null -eq $CargoCommand -or
        -not (Get-ProjDevCanonicalPath -Path $CargoCommand.Source).Equals(
            (Get-ProjDevCanonicalPath -Path $ExpectedCargo),
            [StringComparison]::OrdinalIgnoreCase
        )) {
        throw (
            "The managed Cargo proxy is not selected. Run '$Repair'."
        )
    }
}

function Assert-ProjDevRustReady {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    if ($null -eq (Get-ProjDevRustValidMetadata `
        -Context $Context `
        -Definition $Definition)) {
        $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
        throw (
            'The managed Rust installation is missing or inconsistent. Run ' +
            "'$Repair'."
        )
    }
}
