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
$DataRoot = Join-Path $TemporaryRoot 'data'
$CommandDataRoot = Join-Path $DataRoot 'modules\action\proj\build\launcher'
$WorkRoot = Join-Path $CommandDataRoot 'work'
$ExportRoot = Join-Path $CommandDataRoot 'export'
$CandidatePath = Join-Path $WorkRoot 'candidate.exe'
$ExportPath = Join-Path $ExportRoot 'candidate.exe'
$ProducerAddress = 'proj.build.launcher'
$ProducerContract = 'swawkit.proj-build-launcher/v1'

try {
    [void][IO.Directory]::CreateDirectory($WorkRoot)
    [void][IO.Directory]::CreateDirectory($ExportRoot)
    [IO.File]::WriteAllText($CandidatePath, 'new-candidate')
    [IO.File]::WriteAllText($ExportPath, 'last-known-good')

    $Reported = Publish-ProjBuildArtifact `
        -SourcePath $CandidatePath `
        -ExportPath $ExportPath `
        -CommandDataRoot $CommandDataRoot `
        -ProducerAddress $ProducerAddress `
        -ProducerContract $ProducerContract
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

    $Resolved = Get-ProjRequiredBuildArtifact `
        -DataRoot $DataRoot `
        -ProviderAddress $ProducerAddress `
        -EntryCommand 'fixture' `
        -ProducerContract $ProducerContract `
        -ArtifactName 'candidate.exe'
    $State = Read-ProjCommandProviderState `
        -Path (Join-Path $CommandDataRoot '_state.json') `
        -DataRoot $DataRoot
    $ManifestPath = Join-Path $ExportRoot 'manifest.json'
    $Manifest = [IO.File]::ReadAllText($ManifestPath) | ConvertFrom-Json
    Assert-ProjProjectBuildExport `
        -Condition (
            [string]$Resolved.Path -ceq $ExportPath -and
            [string]$State.Status -ceq 'ready' -and
            [string]$Manifest.token -ceq [string]$State.Token -and
            [string]$Manifest.inputRevision -ceq [string]$State.InputRevision -and
            [string]$Manifest.artifact.sha256 -ceq [string]$Resolved.Sha256
        ) `
        -Message 'the build artifact contract was not published consistently'

    $Manifest.token = 'f' * 32
    [IO.File]::WriteAllText(
        $ManifestPath,
        (ConvertTo-ProjDevJsonText -Value $Manifest),
        [Text.UTF8Encoding]::new($false)
    )
    try {
        Get-ProjRequiredBuildArtifact `
            -DataRoot $DataRoot `
            -ProviderAddress $ProducerAddress `
            -EntryCommand 'fixture' `
            -ProducerContract $ProducerContract `
            -ArtifactName 'candidate.exe' | Out-Null
        throw 'a mismatched manifest token unexpectedly passed validation'
    } catch {
        Assert-ProjProjectBuildExport `
            -Condition ($_.Exception.Message.Contains(
                'invalid artifact manifest'
            )) `
            -Message 'a mismatched manifest token failed for the wrong reason'
    }
    [IO.File]::WriteAllText($CandidatePath, 'new-candidate')
    Publish-ProjBuildArtifact `
        -SourcePath $CandidatePath `
        -ExportPath $ExportPath `
        -CommandDataRoot $CommandDataRoot `
        -ProducerAddress $ProducerAddress `
        -ProducerContract $ProducerContract | Out-Null

    [IO.File]::WriteAllText($ExportPath, 'tampered-candidate')
    try {
        Get-ProjRequiredBuildArtifact `
            -DataRoot $DataRoot `
            -ProviderAddress $ProducerAddress `
            -EntryCommand 'fixture' `
            -ProducerContract $ProducerContract `
            -ArtifactName 'candidate.exe' | Out-Null
        throw 'a tampered artifact unexpectedly passed validation'
    } catch {
        Assert-ProjProjectBuildExport `
            -Condition ($_.Exception.Message.Contains(
                'does not match its manifest'
            )) `
            -Message 'a tampered artifact failed for the wrong reason'
    }
    [IO.File]::WriteAllText($CandidatePath, 'new-candidate')
    Publish-ProjBuildArtifact `
        -SourcePath $CandidatePath `
        -ExportPath $ExportPath `
        -CommandDataRoot $CommandDataRoot `
        -ProducerAddress $ProducerAddress `
        -ProducerContract $ProducerContract | Out-Null

    [IO.File]::WriteAllText($ExportPath, 'preserve-me')
    try {
        Publish-ProjBuildArtifact `
            -SourcePath (Join-Path $WorkRoot 'missing.exe') `
            -ExportPath $ExportPath `
            -CommandDataRoot $CommandDataRoot `
            -ProducerAddress $ProducerAddress `
            -ProducerContract $ProducerContract | Out-Null
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
        Publish-ProjBuildArtifact `
            -SourcePath $CandidatePath `
            -ExportPath $InvalidExportPath `
            -CommandDataRoot $CommandDataRoot `
            -ProducerAddress $ProducerAddress `
            -ProducerContract $ProducerContract | Out-Null
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
    $FailedState = Read-ProjCommandProviderState `
        -Path (Join-Path $CommandDataRoot '_state.json') `
        -DataRoot $DataRoot
    Assert-ProjProjectBuildExport `
        -Condition ([string]$FailedState.Status -ceq 'unavailable') `
        -Message 'a failed artifact publication retained Ready state'

    $OutsidePath = Join-Path (Split-Path $CommandDataRoot -Parent) 'escaped.exe'
    try {
        Publish-ProjBuildArtifact `
            -SourcePath $CandidatePath `
            -ExportPath $OutsidePath `
            -CommandDataRoot $CommandDataRoot `
            -ProducerAddress $ProducerAddress `
            -ProducerContract $ProducerContract | Out-Null
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
    }
    Assert-ProjProjectBuildExport `
        -Condition $AppAction.Contains('Publish-ProjBuildReleaseSet') `
        -Message 'the App Action bypasses the Release Set export boundary'
    Assert-ProjProjectBuildExport `
        -Condition $LauncherAction.Contains('Publish-ProjBuildArtifact') `
        -Message 'the Launcher Action bypasses the artifact export boundary'
    Assert-ProjProjectBuildExport `
        -Condition (-not $LauncherAction.Contains('LauncherBuildRoot')) `
        -Message 'the Launcher Action still writes to the Bootstrap build root'
    $LauncherPublishAction = [IO.File]::ReadAllText((Join-Path $RepoRoot (
        '.swaw\proj\publish\launcher\run.ps1'
    )))
    Assert-ProjProjectBuildExport `
        -Condition (
            $LauncherPublishAction.Contains('Get-ProjRequiredBuildArtifact') -and
            $LauncherPublishAction.Contains('ReparsePoint')
        ) `
        -Message 'the Launcher publisher bypasses its verified safe-target boundary'
} finally {
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        [IO.Directory]::Delete($TemporaryRoot, $true)
    }
}

Write-Host '[PASS] Proj project build export boundary' -ForegroundColor Green
$global:LASTEXITCODE = 0
