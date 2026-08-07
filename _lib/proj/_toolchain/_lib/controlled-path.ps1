Set-StrictMode -Version 2.0

function Assert-ProjDevControlledRoot {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Description
    )

    $FullRoot = Get-ProjDevFullPath -Path $Root
    $Item = Get-Item `
        -LiteralPath $FullRoot `
        -Force `
        -ErrorAction SilentlyContinue
    if ($null -eq $Item) {
        return $FullRoot
    }
    if (($Item.Attributes -band
        [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Description cannot be a reparse point: $FullRoot"
    }
    if (-not $Item.PSIsContainer) {
        throw "$Description must be a directory: $FullRoot"
    }
    return $FullRoot
}

function Assert-ProjDevPathInsideDataRoot {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$Activity
    )

    $FullPath = Get-ProjDevFullPath -Path $Path
    $FullRoot = Get-ProjDevFullPath -Path $DataRoot
    $RootPrefix = $FullRoot.TrimEnd('\', '/') +
        [IO.Path]::DirectorySeparatorChar
    if ($FullPath.Equals($FullRoot, [StringComparison]::OrdinalIgnoreCase) -or
        -not $FullPath.StartsWith(
            $RootPrefix,
            [StringComparison]::OrdinalIgnoreCase
        )) {
        throw "Refusing $Activity outside the controlled data root: $FullPath"
    }

    [void](Assert-ProjDevControlledRoot `
        -Root $FullRoot `
        -Description 'Controlled data root')
    $RelativePath = $FullPath.Substring($RootPrefix.Length)
    $Segments = $RelativePath.Split(
        [char[]]@(
            [IO.Path]::DirectorySeparatorChar,
            [IO.Path]::AltDirectorySeparatorChar
        ),
        [StringSplitOptions]::RemoveEmptyEntries
    )
    $Current = $FullRoot
    for ($Index = 0; $Index -lt $Segments.Length; $Index++) {
        $Current = Join-Path $Current $Segments[$Index]
        $Item = Get-Item `
            -LiteralPath $Current `
            -Force `
            -ErrorAction SilentlyContinue
        if ($null -eq $Item) {
            continue
        }
        if (($Item.Attributes -band
            [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Refusing $Activity through a reparse point: $Current"
        }
        if ($Index -lt ($Segments.Length - 1) -and
            -not $Item.PSIsContainer) {
            throw "Refusing $Activity through a non-directory path: $Current"
        }
    }
    return $FullPath
}
