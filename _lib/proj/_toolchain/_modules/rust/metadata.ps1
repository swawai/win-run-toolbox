Set-StrictMode -Version 2.0

function Write-ProjDevRustMetadata {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Probe,
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [Parameter(Mandatory = $true)][string]$RustupInitSha256
    )

    $InventoryPaths = Get-ProjDevRustInventoryPaths `
        -InstallRoot $InstallRoot `
        -Definition $Definition
    $Metadata = [ordered]@{
        schema = 'swawkit.proj-dev.rust-install.v0'
        name = 'rust'
        inventory = 'toolchain-files-v0'
        declaredToolchain = [string]$Definition.Toolchain
        toolchainName = [string]$Definition.ToolchainName
        profile = [string]$Definition.Profile
        host = [string]$Definition.Host
        components = [string[]]$Definition.RequiredComponents
        recipeVersion = [string]$Definition.RecipeVersion
        definitionSignature = Get-ProjDevRustDefinitionSignature `
            -Definition $Definition
        rustupInitUrl = [string]$Definition.RustupInitUrl
        rustupInitSha256 = $RustupInitSha256
        rustupVersion = [string]$Probe.RustupVersion
        rustcVersion = [string]$Probe.RustcVersion
        rustcCommit = [string]$Probe.RustcCommit
        cargoVersion = [string]$Probe.CargoVersion
        rustfmtVersion = [string]$Probe.RustfmtVersion
        sourceVerification = 'rust-static-sha256'
        files = @(
            Get-ProjDevRustInstallFileRecords `
                -InstallRoot $InstallRoot `
                -RelativePaths $InventoryPaths
        )
    }
    Write-ProjDevTextAtomic `
        -Path (Get-ProjDevRustMetadataPath -InstallRoot $InstallRoot) `
        -Content (ConvertTo-ProjDevJsonText -Value $Metadata) `
        -ControlledRoot $InstallRoot
}
