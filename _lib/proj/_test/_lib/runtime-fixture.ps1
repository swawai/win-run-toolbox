Set-StrictMode -Version 2.0

$script:ProjRuntimeFixtureRepoRoot = [IO.Path]::GetFullPath(
    (Join-Path $PSScriptRoot '..\..\..\..')
)
. (Join-Path $script:ProjRuntimeFixtureRepoRoot (
    '_lib\proj\_toolchain\bootstrap-layout.ps1'
))

function Assert-ProjCandidateRuntimeFixtureRoot {
    param([Parameter(Mandatory = $true)][string]$Path)

    $Path = [IO.Path]::GetFullPath($Path)
    $TestRoot = [IO.Path]::GetFullPath((Join-Path (
        $script:ProjRuntimeFixtureRepoRoot
    ) 'data\_test')).TrimEnd('\') + '\'
    if (-not $Path.StartsWith(
        $TestRoot,
        [StringComparison]::OrdinalIgnoreCase
    ) -or
        -not [IO.Path]::GetFileName($Path).StartsWith(
            'swawkit-proj-',
            [StringComparison]::Ordinal
        )) {
        throw "Unsafe candidate runtime fixture root: $Path"
    }
    return $Path
}

function Resolve-ProjCandidateRuntimeArtifacts {
    param(
        [string]$LauncherPath = '',
        [string]$CorePath = ''
    )

    $BuildDefaults = [string]::IsNullOrWhiteSpace($LauncherPath) -or
        [string]::IsNullOrWhiteSpace($CorePath)
    $Layout = Get-ProjBootstrapLayout
    if ([string]::IsNullOrWhiteSpace($LauncherPath)) {
        $LauncherPath = $Layout.LauncherCandidatePath
    }
    if ([string]::IsNullOrWhiteSpace($CorePath)) {
        $CorePath = Join-Path $Layout.BuildRoot 'release\swawkit-proj.exe'
    }
    if ($BuildDefaults) {
        & (Join-Path $script:ProjRuntimeFixtureRepoRoot (
            '_lib\proj\build.ps1'
        )) | Out-Host
    }

    $LauncherPath = [IO.Path]::GetFullPath($LauncherPath)
    $CorePath = [IO.Path]::GetFullPath($CorePath)
    foreach ($RequiredFile in @($LauncherPath, $CorePath)) {
        if (-not [IO.File]::Exists($RequiredFile)) {
            throw "Required built executable does not exist: $RequiredFile"
        }
    }

    return [pscustomobject][ordered]@{
        LauncherPath = $LauncherPath
        CorePath = $CorePath
    }
}

function New-ProjCandidateRuntimeFixture {
    param(
        [Parameter(Mandatory = $true)][string]$RuntimeHome,
        [Parameter(Mandatory = $true)][string]$LauncherPath,
        [Parameter(Mandatory = $true)][string]$CorePath
    )

    $RuntimeHome = [IO.Path]::GetFullPath($RuntimeHome)
    if ([IO.Path]::GetFileName($RuntimeHome) -cne 'runtime-home') {
        throw "Candidate RuntimeHome must end in 'runtime-home': $RuntimeHome"
    }
    [void](Assert-ProjCandidateRuntimeFixtureRoot `
        -Path (Split-Path -Path $RuntimeHome -Parent))
    $KernelRoot = Join-Path $RuntimeHome '_lib\proj'
    $RuntimeBin = Join-Path $KernelRoot '_bin'
    [void][IO.Directory]::CreateDirectory($RuntimeBin)
    [IO.File]::Copy(
        $CorePath,
        (Join-Path $RuntimeBin 'swawkit-proj.exe'),
        $false
    )

    foreach ($RelativeDirectory in @(
        '..entry',
        '..web',
        '.dev',
        '.help',
        '.info',
        '_help',
        '_shell',
        '_toolchain'
    )) {
        Copy-Item `
            -LiteralPath (Join-Path (
                $script:ProjRuntimeFixtureRepoRoot
            ) "_lib\proj\$RelativeDirectory") `
            -Destination $KernelRoot `
            -Recurse `
            -Force
    }

    return [pscustomobject][ordered]@{
        Home = $RuntimeHome
        KernelRoot = $KernelRoot
        RuntimeBin = $RuntimeBin
        LauncherPath = [IO.Path]::GetFullPath($LauncherPath)
    }
}

function Add-ProjCandidateRuntimeEntry {
    param(
        [Parameter(Mandatory = $true)][object]$Runtime,
        [Parameter(Mandatory = $true)][string]$RelativePath
    )

    $EntryPath = [IO.Path]::GetFullPath((Join-Path $Runtime.Home $RelativePath))
    $RuntimePrefix = $Runtime.Home.TrimEnd('\') + '\'
    if (-not $EntryPath.StartsWith(
        $RuntimePrefix,
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw "Entry path escaped the candidate runtime: $EntryPath"
    }
    [void][IO.Directory]::CreateDirectory((Split-Path -Path $EntryPath -Parent))
    [IO.File]::Copy($Runtime.LauncherPath, $EntryPath, $false)
    return $EntryPath
}

function Remove-ProjCandidateRuntimeFixture {
    param([Parameter(Mandatory = $true)][string]$Path)

    $Path = Assert-ProjCandidateRuntimeFixtureRoot -Path $Path

    $Deadline = [DateTime]::UtcNow.AddSeconds(5)
    while ([IO.Directory]::Exists($Path)) {
        try {
            [IO.Directory]::Delete($Path, $true)
            return
        } catch {
            if ([DateTime]::UtcNow -ge $Deadline) {
                throw
            }
            Start-Sleep -Milliseconds 50
        }
    }
}
