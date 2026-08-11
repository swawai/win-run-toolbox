$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if ($args.Count -ne 0) {
    throw 'proj.publish.launcher does not accept dynamic arguments.'
}
$ProjHome = [string]$env:SWAWKIT_HOME
$DataRoot = [string]$env:SWAWKIT_PROJ_DATA_ROOT
$EntryCommand = [string]$env:SWAWKIT_PROJ_ENTRY_COMMAND
if ([string]::IsNullOrWhiteSpace($ProjHome) -or
    [string]::IsNullOrWhiteSpace($DataRoot) -or
    [string]::IsNullOrWhiteSpace($EntryCommand)) {
    throw 'The project runtime context is incomplete.'
}
$KernelRoot = Join-Path $ProjHome '_lib\proj'
. (Join-Path $KernelRoot '_toolchain\runtime.ps1')
. (Join-Path $PSScriptRoot '..\..\build\_lib\export.ps1')

$Artifact = Get-ProjRequiredBuildArtifact `
    -DataRoot $DataRoot `
    -ProviderAddress 'proj.build.launcher' `
    -EntryCommand $EntryCommand `
    -ProducerContract 'swawkit.proj-build-launcher/v1' `
    -ArtifactName 'template.proj1.exe'
$TemplatePath = Join-Path $ProjHome 'Favorites\template.proj1.exe'
$TemplateRoot = Split-Path -Path $TemplatePath -Parent
$TemplateRootItem = Get-Item `
    -LiteralPath $TemplateRoot `
    -Force `
    -ErrorAction SilentlyContinue
if ($null -eq $TemplateRootItem -or
    -not $TemplateRootItem.PSIsContainer -or
    ($TemplateRootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "The Launcher template directory is unsafe: $TemplateRoot"
}
$TemplateItem = Get-Item `
    -LiteralPath $TemplatePath `
    -Force `
    -ErrorAction SilentlyContinue
if ($null -ne $TemplateItem -and
    ($TemplateItem.PSIsContainer -or
        ($TemplateItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
    throw "The Launcher template target is unsafe: $TemplatePath"
}
$CacheRoot = Assert-ProjDevControlledRoot `
    -Root (Join-Path $ProjHome 'data\proj_cache') `
    -Description 'shared project cache root'
$PublishLock = Enter-ProjDevFileLock `
    -Path (Join-Path $CacheRoot 'locks\launcher-template-publish.lock') `
    -ControlledRoot $CacheRoot `
    -TimeoutSeconds 120
try {
    if ([IO.File]::Exists($TemplatePath) -and
        (Get-ProjDevFileSha256 -Path $TemplatePath) -ceq $Artifact.Sha256) {
        Write-Host "[CURRENT] $TemplatePath ($($Artifact.Sha256))" `
            -ForegroundColor Green
        return
    }
    $Token = [Guid]::NewGuid().ToString('N')
    $Stage = Join-Path $TemplateRoot ".template.proj1.$Token.tmp"
    $BackupRoot = Assert-ProjDevPathInsideDataRoot `
        -Path (Join-Path $CacheRoot 'retired\launcher-template') `
        -DataRoot $CacheRoot `
        -Activity 'retiring the previous Launcher template'
    [void][IO.Directory]::CreateDirectory($BackupRoot)
    $Backup = Join-Path $BackupRoot "template.proj1.$Token.exe"
    try {
        [IO.File]::Copy([string]$Artifact.Path, $Stage, $false)
        $Staged = Get-Item -LiteralPath $Stage
        if ([long]$Staged.Length -ne [long]$Artifact.Length -or
            (Get-ProjDevFileSha256 -Path $Stage) -cne $Artifact.Sha256) {
            throw 'The staged Launcher template does not match its build manifest.'
        }
        if ([IO.File]::Exists($TemplatePath)) {
            [IO.File]::Replace($Stage, $TemplatePath, $Backup, $true)
        } else {
            [IO.File]::Move($Stage, $TemplatePath)
        }
        if ((Get-ProjDevFileSha256 -Path $TemplatePath) -cne $Artifact.Sha256) {
            throw 'The published Launcher template failed SHA-256 verification.'
        }
    } finally {
        if ([IO.File]::Exists($Stage)) {
            [IO.File]::Delete($Stage)
        }
    }
    if ([IO.File]::Exists($Backup)) {
        [IO.File]::Delete($Backup)
    }
    Write-Host "[PUBLISHED] $TemplatePath ($($Artifact.Sha256))" `
        -ForegroundColor Green
} finally {
    $PublishLock.Dispose()
}
