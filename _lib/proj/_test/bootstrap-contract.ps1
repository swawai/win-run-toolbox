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
$BootstrapRoot = Join-Path $RepoRoot '_lib\proj\_bootstrap'
. (Join-Path $BootstrapRoot '_lib\layout.ps1')

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

Write-Host '[PASS] Proj Bootstrap contract' -ForegroundColor Green
$global:LASTEXITCODE = 0
