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
. (Join-Path $RepoRoot '.swaw\proj\publish\_lib\core.ps1')
$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-app-publish-$([Guid]::NewGuid().ToString('N'))"
)
$ProjHome = Join-Path $TemporaryRoot 'home'
$CacheRoot = Join-Path $ProjHome 'data\proj_cache'
$RuntimeDirectory = Join-Path $ProjHome '_lib\proj\_bin'
$RuntimePath = Join-Path $RuntimeDirectory 'swawkit-proj.exe'
$CandidatePath = Join-Path $TemporaryRoot 'candidate.exe'
$Running = $null

try {
    [void][IO.Directory]::CreateDirectory($RuntimeDirectory)
    [void][IO.Directory]::CreateDirectory($CacheRoot)
    [IO.File]::Copy(
        (Join-Path $env:SystemRoot 'System32\cmd.exe'),
        $RuntimePath
    )
    [IO.File]::Copy(
        (Join-Path $env:SystemRoot 'System32\where.exe'),
        $CandidatePath
    )
    $Candidate = Get-Item -LiteralPath $CandidatePath
    $Artifact = [pscustomobject][ordered]@{
        Path = $CandidatePath
        Length = [long]$Candidate.Length
        Sha256 = Get-ProjDevFileSha256 -Path $CandidatePath
    }

    $Running = Start-Process `
        -FilePath $RuntimePath `
        -ArgumentList @('/d', '/c', 'ping -n 8 127.0.0.1 >nul') `
        -WindowStyle Hidden `
        -PassThru
    Start-Sleep -Milliseconds 400
    $Published = Publish-ProjCoreRuntime `
        -Artifact $Artifact `
        -ProjHome $ProjHome `
        -CacheDataRoot $CacheRoot
    Assert-ProjAppPublishTest `
        -Condition (
            $Published.Changed -and
            -not $Running.HasExited -and
            (Get-ProjDevFileSha256 -Path $RuntimePath) -ceq $Artifact.Sha256
        ) `
        -Message 'a running Core could not be replaced atomically'

    $Current = Publish-ProjCoreRuntime `
        -Artifact $Artifact `
        -ProjHome $ProjHome `
        -CacheDataRoot $CacheRoot
    Assert-ProjAppPublishTest `
        -Condition (-not $Current.Changed) `
        -Message 'publishing the current Core was not idempotent'
    Assert-ProjAppPublishTest `
        -Condition (@(
            Get-ChildItem -LiteralPath $RuntimeDirectory -Force |
                Where-Object { $_.Name -like '.swawkit-proj.*' }
        ).Count -eq 0) `
        -Message 'successful publication left runtime-directory files behind'
    Stop-Process -Id $Running.Id -Force
    $Running.WaitForExit()
    $Running = $null
    $Retired = @(Remove-ProjRetiredCoreRuntimes `
        -CacheDataRoot $CacheRoot)
    Assert-ProjAppPublishTest `
        -Condition ($Retired.Count -eq 0) `
        -Message 'an exited Core left a permanent retired runtime behind'
} finally {
    if ($null -ne $Running -and -not $Running.HasExited) {
        Stop-Process -Id $Running.Id -Force
        $Running.WaitForExit()
    }
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        [IO.Directory]::Delete($TemporaryRoot, $true)
    }
}

Write-Host '[PASS] Proj App atomic publication' -ForegroundColor Green
$global:LASTEXITCODE = 0
