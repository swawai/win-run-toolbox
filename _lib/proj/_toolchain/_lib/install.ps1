Set-StrictMode -Version 2.0

function Copy-ProjDevPayload {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    [void][IO.Directory]::CreateDirectory($Destination)
    foreach ($Item in Get-ChildItem -LiteralPath $Source -Force) {
        Copy-Item `
            -LiteralPath $Item.FullName `
            -Destination $Destination `
            -Recurse `
            -Force
    }
}

function Test-ProjDevStagedPayload {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [AllowNull()][scriptblock]$Validate
    )

    if (-not (Test-ProjDevRequiredFiles `
        -Root $InstallRoot `
        -RelativePaths ([string[]]$Definition.RequiredPaths)
    )) {
        return $false
    }
    if ($null -eq $Validate) {
        return $true
    }

    $Result = @(& $Validate $Context $Definition $InstallRoot)
    if ($Result.Count -ne 1 -or $Result[0] -isnot [bool]) {
        throw "The $($Definition.Name) validator must return exactly one Boolean."
    }
    return [bool]$Result[0]
}

function Publish-ProjDevInstallDirectory {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][string]$StagedPath,
        [Parameter(Mandatory = $true)][string]$TargetPath,
        [AllowNull()][scriptblock]$ValidatePublished = $null
    )

    $TargetPath = Assert-ProjDevPathInsideDataRoot `
        -Path $TargetPath `
        -DataRoot $Context.DataRoot `
        -Activity 'publishing a development installation'
    $StagedPath = Assert-ProjDevPathInsideDataRoot `
        -Path $StagedPath `
        -DataRoot $Context.DataRoot `
        -Activity 'publishing a staged development installation'
    [void][IO.Directory]::CreateDirectory(
        (Split-Path -Path $TargetPath -Parent)
    )
    $BackupTimestamp = [DateTime]::UtcNow.ToString(
        'yyyyMMddTHHmmssfffffffZ',
        [Globalization.CultureInfo]::InvariantCulture
    )
    $BackupPath = (
        "$TargetPath.backup-$BackupTimestamp-" +
        [Guid]::NewGuid().ToString('N')
    )
    $BackupKind = ''
    $Published = $false

    try {
        if ([IO.Directory]::Exists($TargetPath)) {
            Move-ProjDevControlledPathWithRetry `
                -Source $TargetPath `
                -Destination $BackupPath `
                -DataRoot $Context.DataRoot `
                -Activity 'preserving the previous installation'
            $BackupKind = 'directory'
        } elseif ([IO.File]::Exists($TargetPath)) {
            Move-ProjDevControlledPathWithRetry `
                -Source $TargetPath `
                -Destination $BackupPath `
                -DataRoot $Context.DataRoot `
                -Activity 'preserving the previous installation'
            $BackupKind = 'file'
        }
        Move-ProjDevControlledPathWithRetry `
            -Source $StagedPath `
            -Destination $TargetPath `
            -DataRoot $Context.DataRoot `
            -Activity 'publishing the staged installation'
        $Published = $true
        if (-not (Invoke-ProjDevInstallCandidateValidation `
            -Context $Context `
            -Definition $Definition `
            -InstallRoot $TargetPath `
            -ValidateCandidate $ValidatePublished)) {
            throw "Published $($Definition.Name) installation failed validation."
        }
    } catch {
        $PublishError = $_
        $RollbackError = $null
        if ($Published -and
            ([IO.Directory]::Exists($TargetPath) -or
             [IO.File]::Exists($TargetPath))) {
            try {
                Remove-ProjDevControlledPathWithRetry `
                    -Path $TargetPath `
                    -DataRoot $Context.DataRoot `
                    -Activity 'rolling back a failed installation'
            } catch {
                $RollbackError = $_
            }
        }
        if ($null -eq $RollbackError -and
            -not [string]::IsNullOrWhiteSpace($BackupKind)) {
            try {
                if ($BackupKind -eq 'directory' -and
                    [IO.Directory]::Exists($BackupPath)) {
                    Move-ProjDevControlledPathWithRetry `
                        -Source $BackupPath `
                        -Destination $TargetPath `
                        -DataRoot $Context.DataRoot `
                        -Activity 'restoring the previous installation'
                    $BackupKind = ''
                } elseif ($BackupKind -eq 'file' -and
                    [IO.File]::Exists($BackupPath)) {
                    Move-ProjDevControlledPathWithRetry `
                        -Source $BackupPath `
                        -Destination $TargetPath `
                        -DataRoot $Context.DataRoot `
                        -Activity 'restoring the previous installation'
                    $BackupKind = ''
                }
            } catch {
                $RollbackError = $_
            }
        }
        if ($null -ne $RollbackError) {
            $TargetRemains = Test-ProjDevPathExists -Path $TargetPath
            $BackupRemains = Test-ProjDevPathExists -Path $BackupPath
            $RecoveryDetail = if ($TargetRemains -and $BackupRemains) {
                "The failed target remains at '$TargetPath', and the " +
                "previous installation backup is preserved at '$BackupPath'."
            } elseif ($BackupRemains) {
                "The previous installation backup is preserved at " +
                "'$BackupPath'."
            } elseif ($TargetRemains) {
                "No previous installation backup was available; the failed " +
                "target remains at '$TargetPath'."
            } else {
                'No recoverable installation path could be confirmed.'
            }
            throw (
                "Publishing $($Definition.Name) failed and rollback is " +
                "pending. $RecoveryDetail Release related processes and run " +
                ".dev.setup again. Original error: " +
                "$($PublishError.Exception.Message). Rollback error: " +
                $RollbackError.Exception.Message
            )
        }
        throw $PublishError
    } finally {
        if ([IO.Directory]::Exists($StagedPath)) {
            Remove-ProjDevInstallResidues `
                -Context $Context `
                -Paths @($StagedPath) `
                -Activity 'cleaning a staged installation'
        }
    }

    if (-not [string]::IsNullOrWhiteSpace($BackupKind) -and
        ([IO.Directory]::Exists($BackupPath) -or
         [IO.File]::Exists($BackupPath))) {
        Remove-ProjDevInstallResidues `
            -Context $Context `
            -Paths @($BackupPath) `
            -Activity 'cleaning a replaced installation'
    }
}

function Install-ProjDevArchiveTool {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [AllowNull()][scriptblock]$Prepare = $null,
        [AllowNull()][scriptblock]$Validate = $null
    )

    Assert-ProjDevArchiveDefinition -Definition $Definition
    $Target = Get-ProjDevInstallRoot `
        -Context $Context `
        -Definition $Definition
    $Recovery = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target
    if ($Recovery.Ready) {
        return $false
    }

    Write-Host "[STEP] Installing $($Definition.Name) $($Definition.Version)..." `
        -ForegroundColor Cyan
    $Parent = Split-Path -Path $Target -Parent
    [void][IO.Directory]::CreateDirectory($Parent)
    $InitialSha256 = [string]$Definition.Sha256
    for ($Attempt = 1; $Attempt -le 2; $Attempt++) {
        $ArchiveAcquired = $false
        $PayloadPrepared = $false
        $WorkRoot = New-ProjDevInstallWorkPath `
            -TargetPath $Target `
            -Kind 'work'
        $ExtractRoot = Join-Path $WorkRoot 'extract'
        $StagedRoot = New-ProjDevInstallWorkPath `
            -TargetPath $Target `
            -Kind 'partial'
        try {
            $ArchivePath = Get-ProjDevVerifiedArchive `
                -Context $Context `
                -Definition $Definition
            $ArchiveAcquired = $true
            Write-Host "[EXT] $([IO.Path]::GetFileName($ArchivePath))" `
                -ForegroundColor DarkGray
            Expand-ProjDevZipSafely `
                -ArchivePath $ArchivePath `
                -Destination $ExtractRoot `
                -ControlledRoot $Context.DataRoot
            $SourceRoot = if ([string]::IsNullOrWhiteSpace(
                [string]$Definition.ArchiveSubdir
            )) {
                $ExtractRoot
            } else {
                Resolve-ProjDevChildPath `
                    -Root $ExtractRoot `
                    -RelativePath ([string]$Definition.ArchiveSubdir) `
                    -Description 'archive subdirectory'
            }
            if (-not [IO.Directory]::Exists($SourceRoot)) {
                throw (
                    "Archive subdirectory is missing: " +
                    $Definition.ArchiveSubdir
                )
            }

            Copy-ProjDevPayload -Source $SourceRoot -Destination $StagedRoot
            if ($null -ne $Prepare) {
                [void](& $Prepare $StagedRoot)
            }
            if (-not (Test-ProjDevStagedPayload `
                -Context $Context `
                -Definition $Definition `
                -InstallRoot $StagedRoot `
                -Validate $Validate
            )) {
                throw "Staged $($Definition.Name) payload failed validation."
            }
            Write-ProjDevInstallMetadata `
                -Definition $Definition `
                -InstallRoot $StagedRoot
            $PayloadPrepared = $true
            Publish-ProjDevInstallDirectory `
                -Context $Context `
                -Definition $Definition `
                -StagedPath $StagedRoot `
                -TargetPath $Target
            return $true
        } catch {
            $AttemptError = $_
            if ($ArchiveAcquired -and
                -not $PayloadPrepared -and
                $Attempt -eq 1) {
                $Definition.Sha256 = $InitialSha256
                Clear-ProjDevArtifactCache `
                    -Context $Context `
                    -Definition $Definition
                Write-Warning (
                    "$($Definition.Name) staging failed; the artifact cache " +
                    'was reset and installation will retry once.'
                )
                continue
            }
            throw $AttemptError
        } finally {
            Remove-ProjDevInstallResidues `
                -Context $Context `
                -Paths @($StagedRoot, $WorkRoot) `
                -Activity 'cleaning installation work data'
        }
    }
    throw (
        "Installing $($Definition.Name) exhausted the clean retry attempt."
    )
}
