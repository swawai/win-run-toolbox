[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\setup.ps1')

function Assert-ProjMsvcCacheTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "MSVC cache test failed: $Message"
    }
}

$PreviousMode = [Environment]::GetEnvironmentVariable(
    'SWAWKIT_PROJ_MSVC_MODE',
    'Process'
)
$PreviousChannel = [Environment]::GetEnvironmentVariable(
    'SWAWKIT_PROJ_MSVC_CHANNEL',
    'Process'
)
$TestTemporaryBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TestTemporaryBase)
$TemporaryRoot = Join-Path $TestTemporaryBase (
    "swawkit-proj-msvc-cache-$([Guid]::NewGuid().ToString('N'))"
)

try {
    $env:SWAWKIT_PROJ_MSVC_MODE = 'managed'
    $env:SWAWKIT_PROJ_MSVC_CHANNEL = '17'
    $Definition = Get-ProjDevMsvcDefinition
    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    [void][IO.Directory]::CreateDirectory($ProjectRoot)
    $Context = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot (Join-Path $TemporaryRoot 'data') `
        -CacheDataRoot (Join-Path $TemporaryRoot 'shared cache')

    $ChannelRoot = Join-Path $Context.CacheRoot 'msvc\17\manifests'
    [void][IO.Directory]::CreateDirectory($ChannelRoot)
    $InterruptedChannelRefresh = Join-Path $ChannelRoot (
        '.channel-11111111111111111111111111111111.json'
    )
    $UnrelatedHiddenFile = Join-Path $ChannelRoot '.channel-not-a-guid.json'
    [IO.File]::WriteAllText($InterruptedChannelRefresh, 'partial')
    [IO.File]::WriteAllText($UnrelatedHiddenFile, 'preserve')
    Remove-ProjDevMsvcChannelRefreshResidues `
        -Root $ChannelRoot `
        -ControlledRoot $Context.CacheDataRoot
    Assert-ProjMsvcCacheTest `
        -Condition (-not [IO.File]::Exists($InterruptedChannelRefresh) -and
            [IO.File]::Exists($UnrelatedHiddenFile)) `
        -Message 'MSVC channel refresh residue cleanup was not strict'

    $PayloadSource = Join-Path $TemporaryRoot 'payload.vsix'
    [IO.File]::WriteAllText($PayloadSource, 'payload-a')
    $Payload = [pscustomobject]@{
        LeafName = 'payload.vsix'
        Sha256 = Get-ProjDevFileSha256 -Path $PayloadSource
        Url = $PayloadSource
    }
    $CachedPayload = Get-ProjDevMsvcVerifiedPayload `
        -Context $Context `
        -Definition $Definition `
        -Payload $Payload
    [IO.File]::Delete($PayloadSource)
    $PeerContext = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot (Join-Path $TemporaryRoot 'peer data') `
        -CacheDataRoot $Context.CacheDataRoot
    $PeerCachedPayload = Get-ProjDevMsvcVerifiedPayload `
        -Context $PeerContext `
        -Definition $Definition `
        -Payload $Payload
    Assert-ProjMsvcCacheTest `
        -Condition ($PeerCachedPayload.Equals(
            $CachedPayload,
            [StringComparison]::OrdinalIgnoreCase
        ) -and
            [IO.Path]::GetFileName(
                [IO.Path]::GetDirectoryName($CachedPayload)
            ) -ceq [string]$Payload.Sha256) `
        -Message 'payload was not shared by verified content identity'

    $SecondSource = Join-Path $TemporaryRoot 'second\payload.vsix'
    [void][IO.Directory]::CreateDirectory(
        [IO.Path]::GetDirectoryName($SecondSource)
    )
    [IO.File]::WriteAllText($SecondSource, 'payload-b')
    $SecondPayload = [pscustomobject]@{
        LeafName = 'payload.vsix'
        Sha256 = Get-ProjDevFileSha256 -Path $SecondSource
        Url = $SecondSource
    }
    $SecondCachedPayload = Get-ProjDevMsvcVerifiedPayload `
        -Context $Context `
        -Definition $Definition `
        -Payload $SecondPayload
    Assert-ProjMsvcCacheTest `
        -Condition (-not $SecondCachedPayload.Equals(
            $CachedPayload,
            [StringComparison]::OrdinalIgnoreCase
        ) -and
            [IO.File]::Exists($CachedPayload) -and
            [IO.File]::Exists($SecondCachedPayload)) `
        -Message 'same-name payload revisions collided'

    $SourceRoot = Join-Path $Context.DataRoot `
        'dev_env\msvc\installs\.fixture.work-source'
    $SourcePath = Copy-ProjDevMsvcPayloadToSourceRoot `
        -Context $Context `
        -Payload $Payload `
        -VerifiedPath $CachedPayload `
        -SourceRoot $SourceRoot
    Assert-ProjMsvcCacheTest `
        -Condition (
            [IO.Path]::GetDirectoryName($SourcePath).Equals(
                $SourceRoot,
                [StringComparison]::OrdinalIgnoreCase
            ) -and
            (Get-ProjDevFileSha256 -Path $SourcePath) -ceq
                [string]$Payload.Sha256
        ) `
        -Message 'payload was not staged into a flat MSI source'

    $ManifestSource = Join-Path $TemporaryRoot 'product.vsman'
    [IO.File]::WriteAllText($ManifestSource, '{"packages":[]}')
    $ManifestActualSha256 = Get-ProjDevFileSha256 -Path $ManifestSource
    $ManifestPayload = [pscustomobject]@{
        Sha256 = 'd' * 64
        Url = $ManifestSource
    }
    $CachedManifest = Get-ProjDevMsvcProductManifestPath `
        -Context $Context `
        -Definition $Definition `
        -Payload $ManifestPayload
    [IO.File]::AppendAllText($CachedManifest, 'damage')
    $RepairedManifest = Get-ProjDevMsvcProductManifestPath `
        -Context $PeerContext `
        -Definition $Definition `
        -Payload $ManifestPayload
    Assert-ProjMsvcCacheTest `
        -Condition ($RepairedManifest.Equals(
            $CachedManifest,
            [StringComparison]::OrdinalIgnoreCase
        ) -and
            (Get-ProjDevFileSha256 -Path $RepairedManifest) -ceq
                $ManifestActualSha256 -and
            [IO.File]::ReadAllText(
                "$RepairedManifest.actual.sha256"
            ).Trim() -ceq $ManifestActualSha256) `
        -Message 'corrupt product manifest was not repaired'

    Write-Host '[PASS] Proj MSVC shared cache test' -ForegroundColor Green
} finally {
    [Environment]::SetEnvironmentVariable(
        'SWAWKIT_PROJ_MSVC_MODE',
        $PreviousMode,
        'Process'
    )
    [Environment]::SetEnvironmentVariable(
        'SWAWKIT_PROJ_MSVC_CHANNEL',
        $PreviousChannel,
        'Process'
    )
    $ResolvedTemporaryRoot = [IO.Path]::GetFullPath($TemporaryRoot)
    $AllowedRoot = $TestTemporaryBase.TrimEnd('\') + '\'
    if ($ResolvedTemporaryRoot.StartsWith(
        $AllowedRoot,
        [StringComparison]::OrdinalIgnoreCase
    ) -and
        [IO.Path]::GetFileName($ResolvedTemporaryRoot).StartsWith(
            'swawkit-proj-msvc-cache-',
            [StringComparison]::Ordinal
        ) -and
        [IO.Directory]::Exists($ResolvedTemporaryRoot)) {
        Remove-Item -LiteralPath $ResolvedTemporaryRoot -Recurse -Force
    }
}
