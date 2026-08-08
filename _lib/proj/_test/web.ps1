[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$ProfilePath = Join-Path $RepoRoot 'data\proj.swawkit\_profile.json'
$WebRoot = Join-Path $RepoRoot '_lib\proj\_app\web'

if (-not [IO.File]::Exists($ProfilePath)) {
    throw "Web tests require the declared Bun profile: '$ProfilePath'. Run '.\swawkit.exe .dev.setup'."
}

$Profile = Get-Content -LiteralPath $ProfilePath -Raw -Encoding UTF8 |
    ConvertFrom-Json
$Bun = $Profile.development.bun
if ($null -eq $Bun -or $Bun.mode -cne 'managed' -or
    [string]::IsNullOrWhiteSpace([string]$Bun.version)) {
    throw 'Web tests require development.bun.mode=managed with a declared version.'
}

$BunExecutable = Join-Path $RepoRoot (
    'data\proj.swawkit\modules\kernel\.dev\setup\export\bun\installs\{0}\bun.exe' -f
    [string]$Bun.version
)
if (-not [IO.File]::Exists($BunExecutable)) {
    throw "Declared Bun $($Bun.version) is not installed at '$BunExecutable'. Run '.\swawkit.exe .dev.setup'."
}

Push-Location $WebRoot
try {
    & $BunExecutable test
    if ($LASTEXITCODE -ne 0) {
        throw "Web tests failed with exit code $LASTEXITCODE."
    }
} finally {
    Pop-Location
}

Write-Host '[PASS] Proj Web test suite' -ForegroundColor Green
$global:LASTEXITCODE = 0
