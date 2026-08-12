[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$CompilerPath,
    [Parameter(Mandatory = $true)][string]$LinkerPath,
    [Parameter(Mandatory = $true)][string]$BuildRoot,
    [Parameter(Mandatory = $true)][string]$CandidatePath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

function Resolve-ProjLauncherBuildExecutable {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Description
    )

    if (-not [IO.Path]::IsPathRooted($Path)) {
        throw "$Description path must be absolute."
    }
    $FullPath = [IO.Path]::GetFullPath($Path)
    if (-not [IO.File]::Exists($FullPath)) {
        throw "$Description does not exist: $FullPath"
    }
    return $FullPath
}

function Assert-ProjLauncherBuildPhysicalDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Description
    )

    $FullPath = [IO.Path]::GetFullPath($Path)
    if (-not [IO.Directory]::Exists($FullPath)) {
        [void][IO.Directory]::CreateDirectory($FullPath)
    }
    $Item = Get-Item -LiteralPath $FullPath -Force
    if (-not $Item.PSIsContainer -or
        ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Description is unsafe: $FullPath"
    }
    return $Item
}

function Assert-ProjLauncherBuildPathInsideRoot {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Description
    )

    if (-not [IO.Path]::IsPathRooted($Path) -or
        -not [IO.Path]::IsPathRooted($Root)) {
        throw "$Description and its build root must be absolute."
    }
    $FullRoot = [IO.Path]::GetFullPath($Root).TrimEnd('\', '/')
    $FullPath = [IO.Path]::GetFullPath($Path)
    $RootPrefix = $FullRoot + [IO.Path]::DirectorySeparatorChar
    if (-not $FullPath.StartsWith(
        $RootPrefix,
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw "$Description escaped the Launcher build root: $FullPath"
    }
    return $FullPath
}

function Assert-ProjLauncherBuildReplaceableFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Description
    )

    $Item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if ($null -ne $Item -and
        ($Item.PSIsContainer -or
            ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "$Description is unsafe: $Path"
    }
}

$CompilerPath = Resolve-ProjLauncherBuildExecutable `
    -Path $CompilerPath `
    -Description 'The injected C compiler'
$LinkerPath = Resolve-ProjLauncherBuildExecutable `
    -Path $LinkerPath `
    -Description 'The injected linker'
if (-not [IO.Path]::IsPathRooted($BuildRoot)) {
    throw 'The Launcher build root must be absolute.'
}
$BuildRoot = [IO.Path]::GetFullPath($BuildRoot)
$CandidatePath = Assert-ProjLauncherBuildPathInsideRoot `
    -Path $CandidatePath `
    -Root $BuildRoot `
    -Description 'The Launcher candidate path'
[void](Assert-ProjLauncherBuildPhysicalDirectory `
    -Path $BuildRoot `
    -Description 'The Launcher build directory')
[void](Assert-ProjLauncherBuildPhysicalDirectory `
    -Path (Split-Path -Path $CandidatePath -Parent) `
    -Description 'The Launcher candidate directory')

$SourcePath = Join-Path $PSScriptRoot 'launcher.c'
if (-not [IO.File]::Exists($SourcePath)) {
    throw "The Launcher source is missing: $SourcePath"
}
$ContractPath = Join-Path $PSScriptRoot 'build.json'
try {
    $Contract = [IO.File]::ReadAllText(
        $ContractPath,
        [Text.Encoding]::UTF8
    ) | ConvertFrom-Json
} catch {
    throw "The Launcher build contract is invalid: $ContractPath"
}
[string[]]$ContractFields = @(
    $Contract.PSObject.Properties | ForEach-Object { [string]$_.Name }
)
if ($ContractFields.Count -ne 5 -or
    $ContractFields -cnotcontains 'schema' -or
    $ContractFields -cnotcontains 'compileArguments' -or
    $ContractFields -cnotcontains 'linkArguments' -or
    $ContractFields -cnotcontains 'libraries' -or
    $ContractFields -cnotcontains 'maximumBytes' -or
    [string]$Contract.schema -cne 'swawkit.proj-launcher-build/v1' -or
    $Contract.compileArguments -isnot [array] -or
    $Contract.linkArguments -isnot [array] -or
    $Contract.libraries -isnot [array] -or
    @($Contract.compileArguments).Count -eq 0 -or
    @($Contract.linkArguments).Count -eq 0 -or
    @($Contract.libraries).Count -eq 0 -or
    @($Contract.compileArguments | Where-Object {
        $_ -isnot [string] -or [string]::IsNullOrEmpty([string]$_)
    }).Count -ne 0 -or
    @($Contract.linkArguments | Where-Object {
        $_ -isnot [string] -or [string]::IsNullOrEmpty([string]$_)
    }).Count -ne 0 -or
    @($Contract.libraries | Where-Object {
        $_ -isnot [string] -or [string]::IsNullOrEmpty([string]$_)
    }).Count -ne 0 -or
    ($Contract.maximumBytes -isnot [int] -and
        $Contract.maximumBytes -isnot [long]) -or
    [long]$Contract.maximumBytes -le 0) {
    throw "The Launcher build contract is invalid: $ContractPath"
}
$ObjectPath = Join-Path $BuildRoot 'launcher.obj'
$StagedPath = Join-Path $BuildRoot 'template.proj1.exe'
Assert-ProjLauncherBuildReplaceableFile `
    -Path $ObjectPath `
    -Description 'The Launcher object target'
Assert-ProjLauncherBuildReplaceableFile `
    -Path $StagedPath `
    -Description 'The Launcher staged executable target'
[string[]]$CompileArguments = @(
    [string[]]@($Contract.compileArguments)
    "/Fo$ObjectPath"
    $SourcePath
)
& $CompilerPath @CompileArguments
if ($LASTEXITCODE -ne 0) {
    throw "cl.exe failed with exit code $LASTEXITCODE."
}

[string[]]$LinkArguments = @(
    [string[]]@($Contract.linkArguments)
    "/OUT:$StagedPath"
    $ObjectPath
    [string[]]@($Contract.libraries)
)
& $LinkerPath @LinkArguments
if ($LASTEXITCODE -ne 0) {
    throw "link.exe failed with exit code $LASTEXITCODE."
}

$StagedItem = Get-Item -LiteralPath $StagedPath
if ($StagedItem.Length -le 0 -or
    $StagedItem.Length -gt [long]$Contract.maximumBytes) {
    throw (
        "Unexpected launcher size $($StagedItem.Length) bytes; expected a " +
        'non-empty thin executable no larger than 64 KiB.'
    )
}

$CandidateParent = Split-Path -Path $CandidatePath -Parent
$PublishPath = Join-Path $CandidateParent (
    ".$([IO.Path]::GetFileName($CandidatePath))." +
    "$([Guid]::NewGuid().ToString('N')).tmp"
)
$BackupPath = Join-Path $CandidateParent (
    ".$([IO.Path]::GetFileName($CandidatePath))." +
    "$([Guid]::NewGuid().ToString('N')).backup"
)
try {
    [IO.File]::Copy($StagedPath, $PublishPath, $false)
    if ([IO.File]::Exists($CandidatePath)) {
        [IO.File]::Replace($PublishPath, $CandidatePath, $BackupPath, $true)
    } else {
        [IO.File]::Move($PublishPath, $CandidatePath)
    }
} finally {
    if ([IO.File]::Exists($PublishPath)) {
        [IO.File]::Delete($PublishPath)
    }
    if ([IO.File]::Exists($BackupPath)) {
        [IO.File]::Delete($BackupPath)
    }
}

$OutputItem = Get-Item -LiteralPath $CandidatePath
Write-Host (
    "[BUILT] $($OutputItem.FullName) ($($OutputItem.Length) bytes)"
) -ForegroundColor Green
$OutputItem | Select-Object FullName, Length, LastWriteTime
$global:LASTEXITCODE = 0
