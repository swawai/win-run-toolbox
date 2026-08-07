[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjBootstrapContractTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Assertion failed: $Message"
    }
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$ProjRoot = Join-Path $RepoRoot '_lib\proj'
. (Join-Path $ProjRoot '_toolchain\bootstrap-layout.ps1')

$Layout = Get-ProjBootstrapLayout
$Contract = Read-ProjBootstrapContract
Assert-ProjBootstrapContractTest `
    -Condition (
        [string]$Contract.Schema -ceq 'swawkit.proj-bootstrap/v1' -and
        [string]$Contract.RustToolchain -cmatch '^\d+\.\d+\.\d+$' -and
        [string]$Contract.MsvcChannel -cmatch '^\d+$'
    ) `
    -Message 'the Bootstrap contract does not pin a valid product toolchain'
Assert-ProjBootstrapContractTest `
    -Condition (
        [IO.Path]::GetFullPath($Layout.BootstrapDataRoot).Equals(
            (Join-Path $RepoRoot 'data\proj_cache\bootstrap'),
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [IO.Path]::GetFullPath($Layout.BuildRoot).Equals(
            (Join-Path $RepoRoot 'data\proj_cache\bootstrap\build\app'),
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [IO.Path]::GetFullPath($Layout.LauncherBuildRoot).Equals(
            (Join-Path $RepoRoot 'data\proj_cache\bootstrap\build\launcher'),
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [IO.Path]::GetFullPath($Layout.LauncherCandidatePath).Equals(
            (Join-Path $RepoRoot (
                'data\proj_cache\bootstrap\build\launcher\release\' +
                'template.proj1.exe'
            )),
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [IO.Path]::GetFullPath($Layout.LauncherTemplatePath).Equals(
            (Join-Path $RepoRoot 'Favorites\template.proj1.exe'),
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [IO.Path]::GetFullPath($Layout.LauncherBuildPath).Equals(
            (Join-Path $RepoRoot '_lib\proj\_launcher\build.ps1'),
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [IO.Path]::GetFullPath($Layout.ContractPath).Equals(
            (Join-Path $RepoRoot '_lib\proj\bootstrap.json'),
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [IO.Path]::GetFullPath($Layout.BootstrapEntryPath).Equals(
            (Join-Path $RepoRoot '_lib\proj\bootstrap.ps1'),
            [StringComparison]::OrdinalIgnoreCase
        )
    ) `
    -Message 'the Bootstrap generated state escaped the shared Proj cache'

$AppBuild = [IO.File]::ReadAllText(
    (Join-Path $RepoRoot '_lib\proj\_app\build.ps1')
)
Assert-ProjBootstrapContractTest `
    -Condition (
        -not $AppBuild.Contains('_bin') -and
        -not $AppBuild.Contains('[IO.File]::Replace')
    ) `
    -Message 'the App build primitive still owns runtime publication'

$LauncherBuild = [IO.File]::ReadAllText($Layout.LauncherBuildPath)
Assert-ProjBootstrapContractTest `
    -Condition (
        -not $LauncherBuild.Contains('_toolchain') -and
        -not $LauncherBuild.Contains('bootstrap.ps1') -and
        -not $LauncherBuild.Contains('launcher-runtime.ps1')
    ) `
    -Message 'the Launcher build primitive still owns orchestration'

$BootstrapEntry = [IO.File]::ReadAllText($Layout.BootstrapEntryPath)
Assert-ProjBootstrapContractTest `
    -Condition (-not $BootstrapEntry.Contains('LauncherBuild')) `
    -Message 'the cold Bootstrap entry still builds the Launcher'

$BootstrapToolchain = [IO.File]::ReadAllText(
    (Join-Path $ProjRoot '_toolchain\bootstrap.ps1')
)
Assert-ProjBootstrapContractTest `
    -Condition (
        -not $BootstrapToolchain.Contains('.dev\setup') -and
        -not $BootstrapToolchain.Contains(
            'Set-ProjBootstrapToolchainDeclarations'
        )
    ) `
    -Message 'the Bootstrap toolchain still depends on development setup'

Assert-ProjBootstrapContractTest `
    -Condition (-not [IO.Directory]::Exists((Join-Path $RepoRoot (
        '.swaw\proj\build\app\bootstrap'
    )))) `
    -Message 'the internal Bootstrap build is still exposed as an Action'

Write-Host '[PASS] Proj Bootstrap contract' -ForegroundColor Green
$global:LASTEXITCODE = 0
