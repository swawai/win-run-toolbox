Set-StrictMode -Version 2.0

function Get-ProjDevRustEnvironmentLayout {
    param(
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $CargoHome = Join-Path $InstallRoot 'cargo'
    $RustupHome = Join-Path $InstallRoot 'rustup'
    $ToolchainBin = Join-Path $RustupHome (
        "toolchains\$($Definition.ToolchainName)\bin"
    )
    $Rustc = Join-Path $ToolchainBin 'rustc.exe'
    $Rustdoc = Join-Path $ToolchainBin 'rustdoc.exe'
    $Variables = [ordered]@{
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
    return [pscustomobject][ordered]@{
        Variables = $Variables
        PathPrefixes = [string[]]@((Join-Path $CargoHome 'bin'))
    }
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
    $Layout = Get-ProjDevRustEnvironmentLayout `
        -InstallRoot $InstallRoot `
        -Definition $Definition
    foreach ($Name in $Layout.Variables.Keys) {
        Set-ProjDevEnvironmentVariable `
            -Plan $Plan `
            -Name ([string]$Name) `
            -Value $Layout.Variables[$Name]
    }
    Add-ProjDevEnvironmentPath `
        -Plan $Plan `
        -Path ([string]$Layout.PathPrefixes[0])
}

function Assert-ProjDevRustEnvironmentCurrent {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $InstallRoot = Get-ProjDevRustInstallRoot `
        -Context $Context `
        -Definition $Definition
    $ExpectedEnvironment = Get-ProjDevRustEnvironmentLayout `
        -InstallRoot $InstallRoot `
        -Definition $Definition
    $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
    foreach ($Name in $ExpectedEnvironment.Variables.Keys) {
        $Expected = $ExpectedEnvironment.Variables[$Name]
        $Actual = [Environment]::GetEnvironmentVariable(
            [string]$Name,
            [EnvironmentVariableTarget]::Process
        )
        $Matches = if ($null -eq $Expected) {
            [string]::IsNullOrWhiteSpace([string]$Actual)
        } else {
            [string]$Actual -ceq [string]$Expected
        }
        if (-not $Matches) {
            throw (
                "The generated Rust environment has a stale $Name. Run " +
                "'$Repair'."
            )
        }
    }

    $ExpectedCargoBin = [string]$ExpectedEnvironment.PathPrefixes[0]
    $CargoPathPresent = $false
    foreach ($Entry in ([string]$env:PATH).Split(
        [IO.Path]::PathSeparator,
        [StringSplitOptions]::RemoveEmptyEntries
    )) {
        try {
            $ActualPath = Get-ProjDevCanonicalPath -Path ([string]$Entry)
        } catch {
            continue
        }
        if ($ActualPath.Equals(
            (Get-ProjDevCanonicalPath -Path $ExpectedCargoBin),
            [StringComparison]::OrdinalIgnoreCase
        )) {
            $CargoPathPresent = $true
            break
        }
    }
    if (-not $CargoPathPresent) {
        throw "The generated Rust environment has an incomplete PATH. Run '$Repair'."
    }

    $CargoCommand = Get-Command cargo.exe `
        -CommandType Application `
        -ErrorAction SilentlyContinue |
        Select-Object -First 1
    $ExpectedCargo = Join-Path $ExpectedCargoBin 'cargo.exe'
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
