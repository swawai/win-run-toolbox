[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjLauncherBuildTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Assertion failed: $Message"
    }
}

function Get-ProjLauncherBuildTestEnvironment {
    $Result = [Collections.Generic.Dictionary[string, string]]::new(
        [StringComparer]::Ordinal
    )
    $Environment = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    foreach ($Name in [string[]]@($Environment.Keys)) {
        $Result[$Name] = [string]$Environment[$Name]
    }
    return $Result
}

function Test-ProjLauncherBuildEnvironmentEqual {
    param(
        [Parameter(Mandatory = $true)][object]$Expected,
        [Parameter(Mandatory = $true)][object]$Actual
    )

    if ($Expected.Count -ne $Actual.Count) {
        return $false
    }
    foreach ($Pair in $Expected.GetEnumerator()) {
        if (-not $Actual.ContainsKey([string]$Pair.Key) -or
            [string]$Actual[[string]$Pair.Key] -cne [string]$Pair.Value) {
            return $false
        }
    }
    return $true
}

function Get-ProjLauncherBuildFileState {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not [IO.File]::Exists($Path)) {
        return [pscustomobject]@{ Exists = $false }
    }
    $Item = Get-Item -LiteralPath $Path
    return [pscustomobject]@{
        Exists = $true
        Length = [long]$Item.Length
        LastWriteTimeUtc = $Item.LastWriteTimeUtc
        Sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    }
}

function Test-ProjLauncherBuildFileUnchanged {
    param(
        [Parameter(Mandatory = $true)][object]$Before,
        [Parameter(Mandatory = $true)][string]$Path
    )

    if (-not $Before.Exists) {
        return -not [IO.File]::Exists($Path)
    }
    if (-not [IO.File]::Exists($Path)) {
        return $false
    }
    $After = Get-Item -LiteralPath $Path
    return $After.Length -eq $Before.Length -and
        $After.LastWriteTimeUtc -eq $Before.LastWriteTimeUtc -and
        (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ceq
            $Before.Sha256
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
. (Join-Path $RepoRoot '_lib\proj\_toolchain\bootstrap-layout.ps1')
$Layout = Get-ProjBootstrapLayout
$BuildPath = Join-Path $RepoRoot '_lib\proj\build.ps1'
$CandidatePath = $Layout.LauncherCandidatePath
$TemplatePath = $Layout.LauncherTemplatePath
$RootEntryPath = Join-Path $RepoRoot 'swawkit.exe'
$RuntimePath = $Layout.RuntimeCurrentPath
$TemplateBefore = Get-ProjLauncherBuildFileState -Path $TemplatePath
$RootBefore = Get-ProjLauncherBuildFileState -Path $RootEntryPath
$RuntimeBefore = Get-ProjLauncherBuildFileState -Path $RuntimePath
$EnvironmentBefore = Get-ProjLauncherBuildTestEnvironment

& $BuildPath | Out-Host
Assert-ProjLauncherBuildTest `
    -Condition ($LASTEXITCODE -eq 0) `
    -Message "the source-tree build failed with exit code $LASTEXITCODE"

$EnvironmentAfter = Get-ProjLauncherBuildTestEnvironment
Assert-ProjLauncherBuildTest `
    -Condition (Test-ProjLauncherBuildEnvironmentEqual `
        -Expected $EnvironmentBefore `
        -Actual $EnvironmentAfter) `
    -Message 'the standalone Launcher build polluted its caller environment'

$Candidate = Get-Item -LiteralPath $CandidatePath -ErrorAction SilentlyContinue
Assert-ProjLauncherBuildTest `
    -Condition ($null -ne $Candidate -and
        $Candidate.Length -gt 0 -and
        $Candidate.Length -le 64KB) `
    -Message 'the standalone Launcher build did not produce a thin candidate'

foreach ($Protected in @(
    [pscustomobject]@{
        Name = 'published Launcher template'
        Path = $TemplatePath
        Before = $TemplateBefore
    }
    [pscustomobject]@{
        Name = 'root control-plane Entry'
        Path = $RootEntryPath
        Before = $RootBefore
    }
    [pscustomobject]@{
        Name = 'published runtime selector'
        Path = $RuntimePath
        Before = $RuntimeBefore
    }
)) {
    Assert-ProjLauncherBuildTest `
        -Condition (Test-ProjLauncherBuildFileUnchanged `
            -Before $Protected.Before `
            -Path $Protected.Path) `
        -Message "the Launcher build modified the $($Protected.Name)"
}

Write-Host '[PASS] Proj source-tree build' -ForegroundColor Green
$global:LASTEXITCODE = 0
