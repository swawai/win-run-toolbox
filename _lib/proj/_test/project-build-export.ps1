[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjProjectBuildExport {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "Project build export test failed: $Message"
    }
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
. (Join-Path $RepoRoot '_lib\proj\_toolchain\runtime.ps1')
. (Join-Path $RepoRoot '.swaw\proj\build\_lib\export.ps1')
$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-project-build-export-$([Guid]::NewGuid().ToString('N'))"
)
$WorkRoot = Join-Path $TemporaryRoot 'work'
$ExportRoot = Join-Path $TemporaryRoot 'export'
$CandidatePath = Join-Path $WorkRoot 'candidate.exe'
$ExportPath = Join-Path $ExportRoot 'candidate.exe'

try {
    [void][IO.Directory]::CreateDirectory($WorkRoot)
    [void][IO.Directory]::CreateDirectory($ExportRoot)
    [IO.File]::WriteAllText($CandidatePath, 'new-candidate')
    [IO.File]::WriteAllText($ExportPath, 'last-known-good')

    $Reported = Publish-ProjBuildCandidate `
        -SourcePath $CandidatePath `
        -ExportPath $ExportPath `
        -CommandDataRoot $TemporaryRoot
    Assert-ProjProjectBuildExport `
        -Condition ([IO.File]::ReadAllText($ExportPath) -ceq 'new-candidate') `
        -Message 'a valid candidate was not exported'
    Assert-ProjProjectBuildExport `
        -Condition ([string]$Reported -ceq $ExportPath) `
        -Message 'the exported path was not reported'
    Assert-ProjProjectBuildExport `
        -Condition (@(
            Get-ChildItem -LiteralPath $ExportRoot -Force |
                Where-Object { $_.Name -like '.candidate.exe.*' }
        ).Count -eq 0) `
        -Message 'a successful export left a recovery file behind'

    [IO.File]::WriteAllText($ExportPath, 'preserve-me')
    try {
        Publish-ProjBuildCandidate `
            -SourcePath (Join-Path $WorkRoot 'missing.exe') `
            -ExportPath $ExportPath `
            -CommandDataRoot $TemporaryRoot | Out-Null
        throw 'a missing candidate unexpectedly succeeded'
    } catch {
        Assert-ProjProjectBuildExport `
            -Condition ($_.Exception.Message.Contains('missing or empty')) `
            -Message 'a missing candidate failed for the wrong reason'
    }
    Assert-ProjProjectBuildExport `
        -Condition ([IO.File]::ReadAllText($ExportPath) -ceq 'preserve-me') `
        -Message 'a failed publication replaced the last known good export'

    $InvalidExportPath = Join-Path $ExportRoot 'directory-target.exe'
    [void][IO.Directory]::CreateDirectory($InvalidExportPath)
    try {
        Publish-ProjBuildCandidate `
            -SourcePath $CandidatePath `
            -ExportPath $InvalidExportPath `
            -CommandDataRoot $TemporaryRoot | Out-Null
        throw 'an invalid export target unexpectedly succeeded'
    } catch {
        Assert-ProjProjectBuildExport `
            -Condition ($_.Exception.Message.Contains(
                'Recovery files were preserved'
            )) `
            -Message 'a failed commit did not report its recovery files'
    }
    $RecoveryFiles = @(
        Get-ChildItem -LiteralPath $ExportRoot -Force |
            Where-Object {
                $_.Name -like '.directory-target.exe.*.tmp'
            }
    )
    Assert-ProjProjectBuildExport `
        -Condition (
            $RecoveryFiles.Count -eq 1 -and
            [IO.File]::ReadAllText($RecoveryFiles[0].FullName) -ceq
                'new-candidate'
        ) `
        -Message 'a failed commit did not preserve the complete candidate'

    $OutsidePath = Join-Path (Split-Path $TemporaryRoot -Parent) 'escaped.exe'
    try {
        Publish-ProjBuildCandidate `
            -SourcePath $CandidatePath `
            -ExportPath $OutsidePath `
            -CommandDataRoot $TemporaryRoot | Out-Null
        throw 'an escaped export unexpectedly succeeded'
    } catch {
        Assert-ProjProjectBuildExport `
            -Condition ($_.Exception.Message.Contains('outside the controlled data root')) `
            -Message 'an escaped export failed for the wrong reason'
    }

    $AppAction = [IO.File]::ReadAllText((Join-Path $RepoRoot (
        '.swaw\proj\build\app\run.ps1'
    )))
    $LauncherAction = [IO.File]::ReadAllText((Join-Path $RepoRoot (
        '.swaw\proj\build\launcher\run.ps1'
    )))
    foreach ($Action in @($AppAction, $LauncherAction)) {
        Assert-ProjProjectBuildExport `
            -Condition $Action.Contains(
                'SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT'
            ) `
            -Message 'a project build Action ignores its command data root'
        Assert-ProjProjectBuildExport `
            -Condition (-not $Action.Contains('SWAWKIT_PROJ_DATA_ROOT')) `
            -Message 'a project build Action still writes through the Entry data root'
        Assert-ProjProjectBuildExport `
            -Condition $Action.Contains('Publish-ProjBuildCandidate') `
            -Message 'a project build Action bypasses the export boundary'
    }
    Assert-ProjProjectBuildExport `
        -Condition (-not $LauncherAction.Contains('LauncherBuildRoot')) `
        -Message 'the Launcher Action still writes to the Bootstrap build root'
} finally {
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        [IO.Directory]::Delete($TemporaryRoot, $true)
    }
}

Write-Host '[PASS] Proj project build export boundary' -ForegroundColor Green
$global:LASTEXITCODE = 0
