Set-StrictMode -Version 2.0

function Add-ProjShellRuntimePath {
    param([Parameter(Mandatory = $true)][string]$KernelRoot)

    $RuntimeBin = [IO.Path]::GetFullPath(
        (Join-Path $KernelRoot '_bin')
    )
    if (-not [IO.Directory]::Exists($RuntimeBin)) {
        throw "Proj runtime bin directory is missing: $RuntimeBin"
    }
    $RuntimeBinItem = Get-Item -LiteralPath $RuntimeBin -Force
    if (($RuntimeBinItem.Attributes -band
        [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Proj runtime bin directory cannot be a reparse point: $RuntimeBin"
    }

    $AlreadyPresent = $false
    foreach ($Entry in ([string]$env:PATH).Split(
        [IO.Path]::PathSeparator,
        [StringSplitOptions]::RemoveEmptyEntries
    )) {
        try {
            $Candidate = [IO.Path]::GetFullPath($Entry.Trim().Trim('"'))
        } catch {
            continue
        }
        if ($Candidate.Equals(
            $RuntimeBin,
            [StringComparison]::OrdinalIgnoreCase
        )) {
            $AlreadyPresent = $true
            break
        }
    }
    if (-not $AlreadyPresent) {
        $env:PATH = (
            "$RuntimeBin$([IO.Path]::PathSeparator)$([string]$env:PATH)"
        )
    }
}

function Get-ProjSystemCmdPath {
    if ([string]::IsNullOrWhiteSpace([string]$env:SystemRoot)) {
        throw 'SystemRoot is unavailable; cannot locate Windows CMD.'
    }
    $Path = [IO.Path]::GetFullPath(
        (Join-Path $env:SystemRoot 'System32\cmd.exe')
    )
    if (-not [IO.File]::Exists($Path)) {
        throw "Windows CMD is unavailable: $Path"
    }
    return $Path
}

function Get-ProjWindowsPowerShellPath {
    if ([string]::IsNullOrWhiteSpace([string]$env:SystemRoot)) {
        throw 'SystemRoot is unavailable; cannot locate Windows PowerShell.'
    }
    $Path = [IO.Path]::GetFullPath(
        (Join-Path $env:SystemRoot (
            'System32\WindowsPowerShell\v1.0\powershell.exe'
        ))
    )
    if (-not [IO.File]::Exists($Path)) {
        throw "Windows PowerShell is unavailable: $Path"
    }
    return $Path
}
