Set-StrictMode -Version 2.0

$script:ProjDevelopmentEnvironmentRevisionPlaceholder =
    '__SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_EXPORT_REVISION__'

function Get-ProjDevelopmentEnvironmentRevisionPlaceholder {
    return $script:ProjDevelopmentEnvironmentRevisionPlaceholder
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

function Get-ProjDevelopmentEnvironmentContentRevision {
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

function Restore-ProjDevelopmentEnvironmentRevisionPlaceholder {
    param(
        [Parameter(Mandatory = $true)][string]$Content,
        [Parameter(Mandatory = $true)][Text.RegularExpressions.Match]$Match
    )

    $RevisionGroup = $Match.Groups[1]
    return $Content.Substring(0, $RevisionGroup.Index) +
        (Get-ProjDevelopmentEnvironmentRevisionPlaceholder) +
        $Content.Substring($RevisionGroup.Index + $RevisionGroup.Length)
}

function Get-ProjPublishedDevelopmentEnvironmentRevision {
    param([Parameter(Mandatory = $true)][object]$Context)

    $EnvironmentRoot = Assert-ProjDevPathInsideDataRoot `
        -Path ([string]$Context.EnvironmentRoot) `
        -DataRoot ([string]$Context.DataRoot) `
        -Activity 'reading the development environment export'
    $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
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
            "'$Repair'."
        )
    }

    $CmdContent = [IO.File]::ReadAllText($CmdPath)
    $Ps1Content = [IO.File]::ReadAllText($Ps1Path)
    $VariableName =
        'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_EXPORT_REVISION'
    $CmdMatches = [regex]::Matches(
        $CmdContent,
        "(?im)^set `"$VariableName=([a-f0-9]{16})`"\s*$"
    )
    $Ps1Matches = [regex]::Matches(
        $Ps1Content,
        "(?im)^\`$env:$VariableName = '([a-f0-9]{16})'\s*$"
    )
    if ($CmdMatches.Count -ne 1 -or $Ps1Matches.Count -ne 1) {
        throw (
            'The published development environment files do not contain ' +
            "one canonical export revision. Run '$Repair'."
        )
    }
    $CmdMatch = $CmdMatches[0]
    $Ps1Match = $Ps1Matches[0]
    if ($CmdMatch.Groups[1].Value -cne $Ps1Match.Groups[1].Value) {
        throw (
            'The published development environment files do not match. Run ' +
            "'$Repair'."
        )
    }
    $Revision = $CmdMatch.Groups[1].Value
    $UnversionedCmd = Restore-ProjDevelopmentEnvironmentRevisionPlaceholder `
        -Content $CmdContent `
        -Match $CmdMatch
    $UnversionedPs1 = Restore-ProjDevelopmentEnvironmentRevisionPlaceholder `
        -Content $Ps1Content `
        -Match $Ps1Match
    $ContentRevision = Get-ProjDevelopmentEnvironmentContentRevision `
        -CmdContent $UnversionedCmd `
        -Ps1Content $UnversionedPs1
    if ($ContentRevision -cne $Revision) {
        throw (
            'The published development environment files were modified or ' +
            "damaged. Run '$Repair'."
        )
    }
    try {
        $State = Read-ProjDevelopmentEnvironmentState `
            -EnvironmentRoot $EnvironmentRoot
    } catch {
        throw "$($_.Exception.Message) Run '$Repair'."
    }
    if ([string]$State.Revision -cne $Revision) {
        throw (
            'The published development environment state does not match ' +
            "env.cmd and env.ps1. Run '$Repair'."
        )
    }

    try {
        $SameProject = (Get-ProjDevCanonicalPath `
            -Path ([string]$State.ProjectRoot)).Equals(
                ([string]$Context.CanonicalProjectRoot),
                [StringComparison]::OrdinalIgnoreCase
            )
        $SameEnvironment = (Get-ProjDevCanonicalPath `
            -Path ([string]$State.ExportRoot)).Equals(
                (Get-ProjDevCanonicalPath -Path $EnvironmentRoot),
                [StringComparison]::OrdinalIgnoreCase
            )
    } catch {
        throw (
            'The published development environment identity is invalid. ' +
            "Run '$Repair'."
        )
    }
    if (-not $SameProject -or -not $SameEnvironment) {
        throw (
            '[DEV ENV OUTDATED] The published development environment ' +
            "belongs to another project or data root. Run '$Repair'."
        )
    }

    return $Revision
}

function Get-ProjDevelopmentEnvironmentRevision {
    param([Parameter(Mandatory = $true)][object]$Context)

    $Revision = Get-ProjPublishedDevelopmentEnvironmentRevision `
        -Context $Context
    if ($null -eq $Revision) {
        return $null
    }
    $State = Read-ProjDevelopmentEnvironmentState `
        -EnvironmentRoot ([string]$Context.EnvironmentRoot)
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
        $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
        [void]$Lines.Add("Run '$Repair'.")
        throw [string]::Join([Environment]::NewLine, $Lines)
    }
    return $Revision
}
