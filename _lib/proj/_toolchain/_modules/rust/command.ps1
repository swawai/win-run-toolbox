Set-StrictMode -Version 2.0

function Resolve-ProjDevRustCommand {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('cargo.exe', 'rustc.exe')]
        [string]$ExecutableName
    )

    $Context = New-ProjDevContextFromEnvironment
    $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
    $Definition = Get-ProjDevRustDefinition
    if ($null -eq $Definition) {
        throw (
            'Rust is disabled for this project. Run ' +
            "'$($Context.EntryCommand) ..entry.env.rust.SWAWKIT_PROJ_RUST_MODE rustup', " +
            "then '$Repair'."
        )
    }
    $MsvcDefinition = Get-ProjDevMsvcDefinition
    if ($null -eq $MsvcDefinition) {
        throw (
            'Rust V0 requires the managed MSVC environment. Run ' +
            "'$($Context.EntryCommand) ..entry.env.msvc.SWAWKIT_PROJ_MSVC_MODE managed', " +
            "then '$Repair'."
        )
    }
    try {
        Import-ProjDevGeneratedEnvironment -Context $Context | Out-Null
        Assert-ProjDevWindowsX64 -ToolName 'Rust'
        Assert-ProjDevMsvcEnvironmentCurrent `
            -Context $Context `
            -Definition $MsvcDefinition
        Assert-ProjDevRustEnvironmentCurrent `
            -Context $Context `
            -Definition $Definition
    } finally {
        Clear-ProjDevSetupExportMetadata
    }

    $InstallRoot = Get-ProjDevRustInstallRoot `
        -Context $Context `
        -Definition $Definition
    $Executable = Resolve-ProjDevChildPath `
        -Root $InstallRoot `
        -RelativePath (
            "rustup\toolchains\$($Definition.ToolchainName)\" +
            "bin\$ExecutableName"
        ) `
        -Description 'Rust command executable'
    if (-not [IO.File]::Exists($Executable)) {
        throw (
            "The managed Rust $ExecutableName is unavailable. Run " +
            "'$Repair'."
        )
    }
    return [pscustomobject][ordered]@{
        Context = $Context
        Definition = $Definition
        Executable = $Executable
    }
}

function Invoke-ProjDevRustCommand {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('cargo.exe', 'rustc.exe')]
        [string]$ExecutableName,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [AllowEmptyString()]
        [string[]]$Arguments
    )

    if ($Arguments.Count -gt 0 -and
        [string]$Arguments[0] -cmatch '^\+') {
        $EntryCommand = [string]$env:SWAWKIT_PROJ_ENTRY_COMMAND
        if ([string]::IsNullOrWhiteSpace($EntryCommand)) {
            $EntryCommand = 'swawkit'
        }
        $Repair = Get-ProjProviderInvocation `
            -EntryCommand $EntryCommand `
            -ProviderAddress '.dev.setup'
        throw (
            'Swaw Kit owns the Rust toolchain selection; +toolchain ' +
            'overrides are not allowed. Run ' +
            "'$EntryCommand ..entry.env.rust.SWAWKIT_PROJ_RUST_TOOLCHAIN <value>', " +
            "then '$Repair'."
        )
    }
    $Command = Resolve-ProjDevRustCommand `
        -ExecutableName $ExecutableName
    return Invoke-ProjDevConsoleProcess `
        -Executable ([string]$Command.Executable) `
        -Arguments $Arguments `
        -WorkingDirectory ([string]$Command.Context.InvocationDirectory)
}
