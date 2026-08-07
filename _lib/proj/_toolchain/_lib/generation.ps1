Set-StrictMode -Version 2.0

$script:ProjDevelopmentEnvironmentGenerationPlaceholder =
    '__SWAWKIT_PROJ_DEV_GENERATION_ID__'

function Get-ProjDevelopmentEnvironmentGenerationPlaceholder {
    return $script:ProjDevelopmentEnvironmentGenerationPlaceholder
}

function Assert-ProjDevelopmentEnvironmentControlledRoot {
    param(
        [Parameter(Mandatory = $true)][string]$EnvironmentRoot
    )

    $FullPath = [IO.Path]::GetFullPath($EnvironmentRoot)
    $Item = Get-Item `
        -LiteralPath $FullPath `
        -Force `
        -ErrorAction SilentlyContinue
    if ($null -eq $Item) {
        return $FullPath
    }
    if (($Item.Attributes -band
        [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw (
            'The managed development environment root cannot be a reparse ' +
            "point: $FullPath"
        )
    }
    if (-not $Item.PSIsContainer) {
        throw (
            'The managed development environment root must be a directory: ' +
            $FullPath
        )
    }
    return $FullPath
}

function Get-ProjDevelopmentEnvironmentContentGenerationId {
    param(
        [Parameter(Mandatory = $true)][string]$CmdContent,
        [Parameter(Mandatory = $true)][string]$Ps1Content
    )

    $Sha = [Security.Cryptography.SHA256]::Create()
    try {
        $Bytes = [Text.Encoding]::UTF8.GetBytes(
            "$CmdContent`n---`n$Ps1Content"
        )
        return ([BitConverter]::ToString(
            $Sha.ComputeHash($Bytes)
        ).Replace('-', '').ToLowerInvariant()).Substring(0, 16)
    } finally {
        $Sha.Dispose()
    }
}

function Restore-ProjDevelopmentEnvironmentGenerationPlaceholder {
    param(
        [Parameter(Mandatory = $true)][string]$Content,
        [Parameter(Mandatory = $true)][Text.RegularExpressions.Match]$Match
    )

    $GenerationGroup = $Match.Groups[1]
    return $Content.Substring(0, $GenerationGroup.Index) +
        (Get-ProjDevelopmentEnvironmentGenerationPlaceholder) +
        $Content.Substring($GenerationGroup.Index + $GenerationGroup.Length)
}

function Get-ProjPublishedDevelopmentEnvironmentGeneration {
    param(
        [Parameter(Mandatory = $true)][string]$EnvironmentRoot,
        [Parameter(Mandatory = $true)][string]$EntryCommand
    )

    $CmdPath = Join-Path $EnvironmentRoot 'env.cmd'
    $Ps1Path = Join-Path $EnvironmentRoot 'env.ps1'
    $StatePath = Get-ProjDevelopmentEnvironmentStatePath `
        -EnvironmentRoot $EnvironmentRoot
    $HasCmd = [IO.File]::Exists($CmdPath)
    $HasPs1 = [IO.File]::Exists($Ps1Path)
    $HasState = [IO.File]::Exists($StatePath)
    if (-not $HasCmd -and -not $HasPs1 -and -not $HasState) {
        return $null
    }
    if (-not $HasCmd -or -not $HasPs1 -or -not $HasState) {
        throw (
            'The published development environment is incomplete. Run ' +
            "'$EntryCommand .dev.setup'."
        )
    }

    $CmdContent = [IO.File]::ReadAllText($CmdPath)
    $Ps1Content = [IO.File]::ReadAllText($Ps1Path)
    $CmdMatches = [regex]::Matches(
        $CmdContent,
        '(?im)^set "SWAWKIT_PROJ_DEV_GENERATION_ID=([a-f0-9]{16})"\s*$'
    )
    $Ps1Matches = [regex]::Matches(
        $Ps1Content,
        "(?im)^\`$env:SWAWKIT_PROJ_DEV_GENERATION_ID = '([a-f0-9]{16})'\s*$"
    )
    if ($CmdMatches.Count -ne 1 -or $Ps1Matches.Count -ne 1) {
        throw (
            'The published development environment files do not contain ' +
            "one canonical generation ID. Run '$EntryCommand .dev.setup'."
        )
    }
    $CmdMatch = $CmdMatches[0]
    $Ps1Match = $Ps1Matches[0]
    if (
        $CmdMatch.Groups[1].Value -cne $Ps1Match.Groups[1].Value) {
        throw (
            'The published development environment files do not match. Run ' +
            "'$EntryCommand .dev.setup'."
        )
    }
    $GenerationId = $CmdMatch.Groups[1].Value
    $UnversionedCmd = Restore-ProjDevelopmentEnvironmentGenerationPlaceholder `
        -Content $CmdContent `
        -Match $CmdMatch
    $UnversionedPs1 = Restore-ProjDevelopmentEnvironmentGenerationPlaceholder `
        -Content $Ps1Content `
        -Match $Ps1Match
    $ContentGenerationId =
        Get-ProjDevelopmentEnvironmentContentGenerationId `
            -CmdContent $UnversionedCmd `
            -Ps1Content $UnversionedPs1
    if ($ContentGenerationId -cne $GenerationId) {
        throw (
            'The published development environment files were modified or ' +
            "damaged. Run '$EntryCommand .dev.setup'."
        )
    }
    $State = Read-ProjDevelopmentEnvironmentState `
        -EnvironmentRoot $EnvironmentRoot
    if ([string]$State.GenerationId -cne $GenerationId) {
        throw (
            'The published development environment state does not match ' +
            "env.cmd and env.ps1. Run '$EntryCommand .dev.setup'."
        )
    }

    return $GenerationId
}

function Get-ProjDevelopmentEnvironmentGeneration {
    param(
        [Parameter(Mandatory = $true)][string]$EnvironmentRoot,
        [Parameter(Mandatory = $true)][string]$EntryCommand
    )

    $GenerationId = Get-ProjPublishedDevelopmentEnvironmentGeneration `
        -EnvironmentRoot $EnvironmentRoot `
        -EntryCommand $EntryCommand
    if ($null -eq $GenerationId) {
        return $null
    }
    $State = Read-ProjDevelopmentEnvironmentState `
        -EnvironmentRoot $EnvironmentRoot
    $Declared = Get-ProjDevelopmentDeclarationSnapshot
    $Differences = @(Compare-ProjDevelopmentDeclarations `
        -Applied $State.Declarations `
        -Declared $Declared)
    if ($Differences.Count -gt 0) {
        $Lines = [Collections.Generic.List[string]]::new()
        [void]$Lines.Add(
            '[DEV ENV OUTDATED] The project development declarations changed.'
        )
        foreach ($Difference in $Differences) {
            [void]$Lines.Add(
                "  $($Difference.Name): '$($Difference.Applied)' -> " +
                "'$($Difference.Declared)'"
            )
        }
        [void]$Lines.Add("Run '$EntryCommand .dev.setup'.")
        throw [string]::Join([Environment]::NewLine, $Lines)
    }
    return $GenerationId
}
