[CmdletBinding()]
param([string]$ToolchainPath = '')

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjToolchainTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Proj Toolchain test failed: $Message"
    }
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
if ([string]::IsNullOrWhiteSpace($ToolchainPath)) {
    $Current = [IO.File]::ReadAllText(
        (Join-Path $RepoRoot '_lib\proj\_bin\current'),
        [Text.Encoding]::UTF8
    ).TrimEnd("`r", "`n")
    $ToolchainPath = Join-Path $RepoRoot (
        "_lib\proj\_bin\releases\$Current\swawkit-proj-toolchain.exe"
    )
}
$ToolchainPath = [IO.Path]::GetFullPath($ToolchainPath)
if (-not [IO.File]::Exists($ToolchainPath)) {
    throw "Proj Toolchain candidate is missing: $ToolchainPath"
}
. (Join-Path $RepoRoot '_lib\proj\_toolchain\runtime.ps1')
. (Join-Path $RepoRoot '_lib\proj\_toolchain\_lib\event.ps1')
. (Join-Path $RepoRoot '_lib\proj\_toolchain\_lib\artifact.ps1')

$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-toolchain-$([Guid]::NewGuid().ToString('N'))"
)
$SourceRoot = Join-Path $TemporaryRoot 'source'
$ControlledRoot = Join-Path $TemporaryRoot 'controlled'
$SourceArchive = Join-Path $TemporaryRoot 'fixture.zip'
$DownloadedArchive = Join-Path $ControlledRoot 'cache\fixture.zip'
$ExtractRoot = Join-Path $ControlledRoot 'extract'

try {
    [void][IO.Directory]::CreateDirectory($SourceRoot)
    [void][IO.Directory]::CreateDirectory(
        (Split-Path $DownloadedArchive -Parent)
    )
    [void][IO.Directory]::CreateDirectory($ExtractRoot)
    [IO.File]::WriteAllText(
        (Join-Path $SourceRoot 'fixture.txt'),
        'native-toolchain',
        [Text.UTF8Encoding]::new($false)
    )
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::CreateFromDirectory(
        $SourceRoot,
        $SourceArchive,
        [IO.Compression.CompressionLevel]::Optimal,
        $false
    )

    Invoke-ProjDevDownload `
        -Source $SourceArchive `
        -Destination $DownloadedArchive `
        -ControlledRoot $ControlledRoot `
        -ToolchainExecutable $ToolchainPath
    Assert-ProjToolchainTest `
        -Condition (Test-ProjDevZipArchive `
            -Path $DownloadedArchive `
            -ToolchainExecutable $ToolchainPath) `
        -Message 'the native ZIP validator rejected a valid archive'
    Expand-ProjDevZipSafely `
        -ArchivePath $DownloadedArchive `
        -Destination $ExtractRoot `
        -ControlledRoot $ControlledRoot `
        -ToolchainExecutable $ToolchainPath
    Assert-ProjToolchainTest `
        -Condition ([IO.File]::ReadAllText(
            (Join-Path $ExtractRoot 'fixture.txt'),
            [Text.Encoding]::UTF8
        ) -ceq 'native-toolchain') `
        -Message 'native download/extraction did not preserve the payload'

    $PreviousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $EscapedOutput = @(& $ToolchainPath `
            'zip-extract-v1' `
            $ControlledRoot `
            $DownloadedArchive `
            (Join-Path $TemporaryRoot 'escaped') 2>&1)
        $EscapedExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousPreference
    }
    Assert-ProjToolchainTest `
        -Condition (
            $EscapedExitCode -ne 0 -and
            [string]::Join("`n", [string[]]$EscapedOutput).Contains(
                'escapes the controlled root'
            )
        ) `
        -Message 'the native Toolchain accepted an escaped destination'
} finally {
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        [IO.Directory]::Delete($TemporaryRoot, $true)
    }
}

Write-Host '[PASS] Proj native Toolchain protocol' -ForegroundColor Green
$global:LASTEXITCODE = 0
