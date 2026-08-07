[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$CandidatePath,
    [Parameter(Mandatory = $true)][string]$RuntimePath,
    [Parameter(Mandatory = $true)][string]$CandidateRoot
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

. (Join-Path $PSScriptRoot '..\_toolchain\runtime.ps1')
$CandidatePath = Assert-ProjDevPathInsideDataRoot `
    -Path $CandidatePath `
    -DataRoot $CandidateRoot `
    -Activity 'publishing the Bootstrap application'
if (-not [IO.Path]::IsPathRooted($RuntimePath)) {
    throw 'The Core runtime path must be absolute.'
}
$RuntimePath = [IO.Path]::GetFullPath($RuntimePath)
if (-not [IO.File]::Exists($CandidatePath) -or
    (Get-Item -LiteralPath $CandidatePath).Length -le 0) {
    throw "The Bootstrap application candidate is missing or empty: $CandidatePath"
}
if ([IO.File]::Exists($RuntimePath)) {
    throw (
        'Bootstrap publication refuses to replace an existing shared Core: ' +
        $RuntimePath
    )
}

$RuntimeDirectory = Split-Path -Path $RuntimePath -Parent
$RuntimeDirectoryItem = Get-Item `
    -LiteralPath $RuntimeDirectory `
    -Force `
    -ErrorAction SilentlyContinue
if ($null -ne $RuntimeDirectoryItem -and
    (-not $RuntimeDirectoryItem.PSIsContainer -or
        ($RuntimeDirectoryItem.Attributes -band
            [IO.FileAttributes]::ReparsePoint) -ne 0)) {
    throw "The shared runtime directory is unsafe: $RuntimeDirectory"
}
if ($null -eq $RuntimeDirectoryItem) {
    [void][IO.Directory]::CreateDirectory($RuntimeDirectory)
}
$StagedPath = Join-Path $RuntimeDirectory (
    ".swawkit-proj.$([Guid]::NewGuid().ToString('N')).tmp"
)
try {
    [IO.File]::Copy($CandidatePath, $StagedPath, $false)
    [IO.File]::Move($StagedPath, $RuntimePath)
} finally {
    if ([IO.File]::Exists($StagedPath)) {
        [IO.File]::Delete($StagedPath)
    }
}

Write-Host "[PUBLISHED] $RuntimePath" -ForegroundColor Green
