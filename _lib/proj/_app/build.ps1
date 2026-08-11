[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$CargoPath,
    [Parameter(Mandatory = $true)][string]$TargetDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if (-not [IO.Path]::IsPathRooted($CargoPath)) {
    throw 'The injected Cargo path must be absolute.'
}
$CargoPath = [IO.Path]::GetFullPath($CargoPath)
if (-not [IO.File]::Exists($CargoPath)) {
    throw "The injected Cargo executable does not exist: $CargoPath"
}
if (-not [IO.Path]::IsPathRooted($TargetDirectory)) {
    throw 'The Cargo target directory must be absolute.'
}
$TargetDirectory = [IO.Path]::GetFullPath($TargetDirectory)

$AppRoot = [IO.Path]::GetFullPath($PSScriptRoot)
$ManifestPath = Join-Path $AppRoot 'Cargo.toml'
if (-not [IO.File]::Exists($ManifestPath)) {
    throw "The Swaw Kit Proj Cargo manifest is missing: $ManifestPath"
}

[string[]]$CargoArguments = @(
    'build'
    '--locked'
    '--release'
    '--manifest-path'
    $ManifestPath
    '--target-dir'
    $TargetDirectory
)
Push-Location $AppRoot
try {
    & $CargoPath @CargoArguments
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo failed with exit code $LASTEXITCODE."
    }
} finally {
    Pop-Location
}

foreach ($Name in @(
    'swawkit-proj.exe',
    'swawkit-proj-host.exe',
    'swawkit-proj-toolchain.exe'
)) {
    $BuiltPath = Join-Path (Join-Path $TargetDirectory 'release') $Name
    if (-not [IO.File]::Exists($BuiltPath)) {
        throw "Cargo reported success but the application is missing: $BuiltPath"
    }
    $BuiltItem = Get-Item -LiteralPath $BuiltPath
    if ($BuiltItem.Length -le 0) {
        throw "Cargo produced an empty application: $BuiltPath"
    }
    Write-Host (
        "[BUILT] $($BuiltItem.FullName) ($($BuiltItem.Length) bytes)"
    ) -ForegroundColor Green
    Write-Output $BuiltItem.FullName
}
