[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjAppPublishTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Proj App publish test failed: $Message"
    }
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
. (Join-Path $RepoRoot '_lib\proj\_toolchain\runtime.ps1')
. (Join-Path $RepoRoot '_lib\proj\_toolchain\_lib\runtime-release.ps1')
$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-app-publish-$([Guid]::NewGuid().ToString('N'))"
)
$ProjHome = Join-Path $TemporaryRoot 'home'
$CacheRoot = Join-Path $ProjHome 'data\proj_cache'
$RuntimeRoot = Join-Path $ProjHome '_lib\proj\_bin'
$JunctionPath = Join-Path $TemporaryRoot 'junction-runtime\releases'
$PreviousCore = Join-Path $TemporaryRoot 'previous\swawkit-proj.exe'
$PreviousHost = Join-Path $TemporaryRoot 'previous\swawkit-proj-host.exe'
$PreviousToolchain = Join-Path $TemporaryRoot (
    'previous\swawkit-proj-toolchain.exe'
)
$CoreCandidate = Join-Path $TemporaryRoot 'candidate\swawkit-proj.exe'
$HostCandidate = Join-Path $TemporaryRoot 'candidate\swawkit-proj-host.exe'
$ToolchainCandidate = Join-Path $TemporaryRoot (
    'candidate\swawkit-proj-toolchain.exe'
)
$Running = $null

try {
    foreach ($Directory in @(
        $RuntimeRoot,
        $CacheRoot,
        (Split-Path $PreviousCore -Parent),
        (Split-Path $CoreCandidate -Parent)
    )) {
        [void][IO.Directory]::CreateDirectory($Directory)
    }
    [IO.File]::Copy((Join-Path $env:SystemRoot 'System32\cmd.exe'), $PreviousCore)
    [IO.File]::Copy((Join-Path $env:SystemRoot 'System32\where.exe'), $PreviousHost)
    [IO.File]::Copy((Join-Path $env:SystemRoot 'System32\whoami.exe'), $PreviousToolchain)
    $PreviousSet = New-ProjRuntimeReleaseSetFromFiles `
        -Artifacts ([ordered]@{
            'swawkit-proj.exe' = $PreviousCore
            'swawkit-proj-host.exe' = $PreviousHost
            'swawkit-proj-toolchain.exe' = $PreviousToolchain
        })
    $PreviousRelease = Publish-ProjRuntimeReleaseSet `
        -ReleaseSet $PreviousSet `
        -ProjHome $ProjHome `
        -CacheDataRoot $CacheRoot

    $Running = Start-Process `
        -FilePath (Join-Path $PreviousRelease.Root 'swawkit-proj.exe') `
        -ArgumentList @('/d', '/c', 'ping -n 8 127.0.0.1 >nul') `
        -WindowStyle Hidden `
        -PassThru
    Start-Sleep -Milliseconds 400

    [IO.File]::Copy((Join-Path $env:SystemRoot 'System32\where.exe'), $CoreCandidate)
    [IO.File]::Copy((Join-Path $env:SystemRoot 'System32\whoami.exe'), $HostCandidate)
    [IO.File]::Copy((Join-Path $env:SystemRoot 'System32\hostname.exe'), $ToolchainCandidate)
    $ReleaseSet = New-ProjRuntimeReleaseSetFromFiles `
        -Artifacts ([ordered]@{
            'swawkit-proj.exe' = $CoreCandidate
            'swawkit-proj-host.exe' = $HostCandidate
            'swawkit-proj-toolchain.exe' = $ToolchainCandidate
        })
    $Published = Publish-ProjRuntimeReleaseSet `
        -ReleaseSet $ReleaseSet `
        -ProjHome $ProjHome `
        -CacheDataRoot $CacheRoot
    $ReleaseRoot = Join-Path (Join-Path $RuntimeRoot 'releases') $ReleaseSet.ReleaseId
    Assert-ProjAppPublishTest `
        -Condition (
            -not $Running.HasExited -and
            [string]$Published.ReleaseId -ceq [string]$ReleaseSet.ReleaseId -and
            [IO.File]::ReadAllText(
                (Join-Path $RuntimeRoot 'current'),
                [Text.Encoding]::UTF8
            ) -ceq ([string]$ReleaseSet.ReleaseId + "`n") -and
            [IO.File]::Exists((Join-Path $ReleaseRoot 'manifest.json')) -and
            -not [IO.File]::Exists((Join-Path $RuntimeRoot 'swawkit-proj.exe')) -and
            -not [IO.File]::Exists((Join-Path $RuntimeRoot 'swawkit-proj-host.exe')) -and
            -not [IO.File]::Exists((Join-Path $RuntimeRoot 'swawkit-proj-toolchain.exe'))
        ) `
        -Message 'a complete Release Set was not atomically selected beside a running old release'
    $Selected = Read-ProjSelectedRuntimeReleaseSet -RuntimeRoot $RuntimeRoot
    Assert-ProjAppPublishTest `
        -Condition ([string]$Selected.ReleaseId -ceq [string]$ReleaseSet.ReleaseId) `
        -Message 'the selected Release Set did not pass the Bootstrap read contract'

    $JunctionRuntime = Join-Path $TemporaryRoot 'junction-runtime'
    $ExternalReleases = Join-Path $TemporaryRoot 'external-releases'
    [void][IO.Directory]::CreateDirectory($JunctionRuntime)
    [void][IO.Directory]::CreateDirectory($ExternalReleases)
    Copy-Item `
        -LiteralPath $ReleaseRoot `
        -Destination (Join-Path $ExternalReleases $ReleaseSet.ReleaseId) `
        -Recurse
    [void](New-Item `
        -ItemType Junction `
        -Path $JunctionPath `
        -Target $ExternalReleases)
    try {
        Read-ProjRuntimeReleaseSet `
            -ReleaseRoot (Join-Path (
                Join-Path $JunctionRuntime 'releases'
            ) $ReleaseSet.ReleaseId) `
            -ReleaseId $ReleaseSet.ReleaseId |
            Out-Null
        throw 'a Release Set behind a parent junction unexpectedly passed validation'
    } catch {
        Assert-ProjAppPublishTest `
            -Condition $_.Exception.Message.Contains('directory is unsafe') `
            -Message 'a parent-junction Release Set failed for the wrong reason'
    }

    $Before = (Get-Item -LiteralPath $ReleaseRoot).CreationTimeUtc
    [void](Publish-ProjRuntimeReleaseSet `
        -ReleaseSet $ReleaseSet `
        -ProjHome $ProjHome `
        -CacheDataRoot $CacheRoot)
    Assert-ProjAppPublishTest `
        -Condition ((Get-Item -LiteralPath $ReleaseRoot).CreationTimeUtc -eq $Before) `
        -Message 'publishing the current Release Set was not idempotent'
    Assert-ProjAppPublishTest `
        -Condition (@(
            Get-ChildItem -LiteralPath $RuntimeRoot -Force |
                Where-Object { $_.Name -like '.*.tmp' -or $_.Name -like '.current.*' }
        ).Count -eq 0) `
        -Message 'successful publication left runtime temporary files behind'

    $ReleaseHost = Join-Path $ReleaseRoot 'swawkit-proj-host.exe'
    [IO.File]::WriteAllText($ReleaseHost, 'coherently-tampered-host')
    $ManifestPath = Join-Path $ReleaseRoot 'manifest.json'
    $Manifest = [IO.File]::ReadAllText(
        $ManifestPath,
        [Text.Encoding]::UTF8
    ) | ConvertFrom-Json
    $HostRecord = @(
        $Manifest.artifacts |
            Where-Object name -CEQ 'swawkit-proj-host.exe'
    )[0]
    $HostRecord.length = (Get-Item -LiteralPath $ReleaseHost).Length
    $HostRecord.sha256 = Get-ProjDevFileSha256 -Path $ReleaseHost
    [IO.File]::WriteAllText(
        $ManifestPath,
        (ConvertTo-ProjDevJsonText -Value $Manifest),
        [Text.UTF8Encoding]::new($false)
    )
    try {
        Read-ProjSelectedRuntimeReleaseSet `
            -RuntimeRoot $RuntimeRoot |
            Out-Null
        throw 'a Release Set with a stale content identity unexpectedly passed validation'
    } catch {
        Assert-ProjAppPublishTest `
            -Condition $_.Exception.Message.Contains(
                'ID does not match its artifacts'
            ) `
            -Message 'a stale runtime content identity failed for the wrong reason'
    }
} finally {
    if ($null -ne $Running -and -not $Running.HasExited) {
        Stop-Process -Id $Running.Id -Force
        $Running.WaitForExit()
    }
    if ([IO.Directory]::Exists($JunctionPath)) {
        [IO.Directory]::Delete($JunctionPath)
    }
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        [IO.Directory]::Delete($TemporaryRoot, $true)
    }
}

Write-Host '[PASS] Proj App Release Set publication' -ForegroundColor Green
$global:LASTEXITCODE = 0
