[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjReleaseSetTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Proj Release Set test failed: $Message"
    }
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
. (Join-Path $RepoRoot '_lib\proj\_toolchain\runtime.ps1')
. (Join-Path $RepoRoot '.swaw\proj\build\_lib\export.ps1')
. (Join-Path $RepoRoot '.swaw\proj\build\_lib\release-set.ps1')
$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-release-set-$([Guid]::NewGuid().ToString('N'))"
)
$DataRoot = Join-Path $TemporaryRoot 'data'
$CommandRoot = Join-Path $DataRoot 'modules\action\proj\build\app'
$WorkRoot = Join-Path $CommandRoot 'work'
$Core = Join-Path $WorkRoot 'swawkit-proj.exe'
$HostCandidate = Join-Path $WorkRoot 'swawkit-proj-host.exe'
$ToolchainCandidate = Join-Path $WorkRoot 'swawkit-proj-toolchain.exe'
$Contract = 'swawkit.proj-build-app/v3'

try {
    [void][IO.Directory]::CreateDirectory($WorkRoot)
    [IO.File]::WriteAllText($Core, 'core-v1')
    [IO.File]::WriteAllText($HostCandidate, 'host-v1')
    [IO.File]::WriteAllText($ToolchainCandidate, 'toolchain-v1')
    $Published = Publish-ProjBuildReleaseSet `
        -Artifacts ([ordered]@{
            'swawkit-proj.exe' = $Core
            'swawkit-proj-host.exe' = $HostCandidate
            'swawkit-proj-toolchain.exe' = $ToolchainCandidate
        }) `
        -CommandDataRoot $CommandRoot `
        -ProducerAddress 'proj.build.app' `
        -ProducerContract $Contract
    $Resolved = Get-ProjRequiredBuildReleaseSet `
        -DataRoot $DataRoot `
        -ProviderAddress 'proj.build.app' `
        -EntryCommand 'fixture' `
        -ProducerContract $Contract `
        -ArtifactNames @(
            'swawkit-proj.exe',
            'swawkit-proj-host.exe',
            'swawkit-proj-toolchain.exe'
        )
    $State = Read-ProjCommandProviderState `
        -Path (Join-Path $CommandRoot '_state.json') `
        -DataRoot $DataRoot
    Assert-ProjReleaseSetTest `
        -Condition (
            [string]$Resolved.ReleaseId -ceq [string]$Published.ReleaseId -and
            [string]$State.InputRevision -ceq
                ('sha256-' + [string]$Resolved.ReleaseId) -and
            @($Resolved.Artifacts).Count -eq 3
        ) `
        -Message 'the build Release Set did not publish one coherent identity'

    $HostExport = @(
        $Resolved.Artifacts | Where-Object Name -CEQ 'swawkit-proj-host.exe'
    )[0].Path
    [IO.File]::WriteAllText($HostExport, 'tampered')
    try {
        Get-ProjRequiredBuildReleaseSet `
            -DataRoot $DataRoot `
            -ProviderAddress 'proj.build.app' `
            -EntryCommand 'fixture' `
            -ProducerContract $Contract `
            -ArtifactNames @(
                'swawkit-proj.exe',
                'swawkit-proj-host.exe',
                'swawkit-proj-toolchain.exe'
            ) |
            Out-Null
        throw 'a tampered Release Set unexpectedly passed validation'
    } catch {
        Assert-ProjReleaseSetTest `
            -Condition $_.Exception.Message.Contains('artifact is corrupt') `
            -Message 'a tampered Release Set failed for the wrong reason'
    }
} finally {
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        [IO.Directory]::Delete($TemporaryRoot, $true)
    }
}

Write-Host '[PASS] Proj build Release Set contract' -ForegroundColor Green
$global:LASTEXITCODE = 0
