Set-StrictMode -Version 2.0

$script:ProjBootstrapRoot = [IO.Path]::GetFullPath(
    (Join-Path $PSScriptRoot '..')
)

function Get-ProjBootstrapLayout {
    $KernelRoot = [IO.Path]::GetFullPath(
        (Join-Path $script:ProjBootstrapRoot '..')
    )
    $ProjHome = [IO.Path]::GetFullPath((Join-Path $KernelRoot '..\..'))
    $CacheRoot = Join-Path $ProjHome 'data\proj_cache'
    $BootstrapDataRoot = Join-Path $CacheRoot 'bootstrap'
    $LauncherBuildRoot = Join-Path $BootstrapDataRoot 'build\launcher'
    return [pscustomobject][ordered]@{
        BootstrapRoot = $script:ProjBootstrapRoot
        ContractPath = Join-Path $script:ProjBootstrapRoot 'bootstrap.json'
        KernelRoot = $KernelRoot
        ProjHome = $ProjHome
        AppRoot = Join-Path $KernelRoot '_app'
        AppBuildPath = Join-Path $KernelRoot '_app\build.ps1'
        RuntimePath = Join-Path $KernelRoot '_bin\swawkit-proj.exe'
        LauncherBuildPath = Join-Path $KernelRoot '_launcher\build.ps1'
        LauncherBuildRoot = $LauncherBuildRoot
        LauncherCandidatePath = Join-Path $LauncherBuildRoot (
            'release\template.proj1.exe'
        )
        LauncherTemplatePath = Join-Path $ProjHome (
            'Favorites\template.proj1.exe'
        )
        CacheRoot = $CacheRoot
        BootstrapDataRoot = $BootstrapDataRoot
        ToolchainRoot = Join-Path $BootstrapDataRoot 'toolchains'
        BuildRoot = Join-Path $BootstrapDataRoot 'build\app'
        LockRoot = Join-Path $BootstrapDataRoot '_locks'
        StatePath = Join-Path $BootstrapDataRoot 'state.json'
    }
}

function Read-ProjBootstrapContract {
    $Layout = Get-ProjBootstrapLayout
    if (-not [IO.File]::Exists($Layout.ContractPath)) {
        throw "The Bootstrap contract is missing: $($Layout.ContractPath)"
    }
    try {
        $Contract = [IO.File]::ReadAllText(
            $Layout.ContractPath,
            [Text.Encoding]::UTF8
        ) | ConvertFrom-Json
    } catch {
        throw "Cannot parse the Bootstrap contract: $($_.Exception.Message)"
    }

    [string[]]$Expected = @('schema', 'rustToolchain', 'msvcChannel')
    [string[]]$Actual = @(
        $Contract.PSObject.Properties | ForEach-Object { [string]$_.Name }
    )
    foreach ($Name in $Expected) {
        if ($Actual -cnotcontains $Name) {
            throw "The Bootstrap contract is missing '$Name'."
        }
    }
    foreach ($Name in $Actual) {
        if ($Expected -cnotcontains $Name) {
            throw "The Bootstrap contract contains unknown field '$Name'."
        }
    }

    $RustToolchain = ([string]$Contract.rustToolchain).Trim().ToLowerInvariant()
    $MsvcChannel = ([string]$Contract.msvcChannel).Trim()
    if ([string]$Contract.schema -cne 'swawkit.proj-bootstrap/v1') {
        throw 'Unsupported Bootstrap contract schema.'
    }
    if ($RustToolchain -cnotmatch '^\d+\.\d+\.\d+$') {
        throw 'Bootstrap rustToolchain must be an exact Rust version.'
    }
    if ($MsvcChannel -cnotmatch '^\d+$') {
        throw 'Bootstrap msvcChannel must be a numeric Visual Studio channel.'
    }
    return [pscustomobject][ordered]@{
        Schema = [string]$Contract.schema
        RustToolchain = $RustToolchain
        MsvcChannel = $MsvcChannel
    }
}
