Set-StrictMode -Version 2.0

function Publish-ProjBuildCandidate {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePath,
        [Parameter(Mandatory = $true)][string]$ExportPath,
        [Parameter(Mandatory = $true)][string]$CommandDataRoot
    )

    $SourcePath = Assert-ProjDevPathInsideDataRoot `
        -Path $SourcePath `
        -DataRoot $CommandDataRoot `
        -Activity 'reading a project build candidate'
    $ExportPath = Assert-ProjDevPathInsideDataRoot `
        -Path $ExportPath `
        -DataRoot $CommandDataRoot `
        -Activity 'publishing a project build export'
    if (-not [IO.File]::Exists($SourcePath) -or
        (Get-Item -LiteralPath $SourcePath).Length -le 0) {
        throw "The project build candidate is missing or empty: $SourcePath"
    }

    $ExportRoot = Split-Path -Path $ExportPath -Parent
    [void][IO.Directory]::CreateDirectory($ExportRoot)
    $ExportRoot = Assert-ProjDevPathInsideDataRoot `
        -Path $ExportRoot `
        -DataRoot $CommandDataRoot `
        -Activity 'using the project build export directory'
    $FileName = [IO.Path]::GetFileName($ExportPath)
    $TemporaryPath = Join-Path $ExportRoot (
        ".$FileName.$([Guid]::NewGuid().ToString('N')).tmp"
    )
    $BackupPath = Join-Path $ExportRoot (
        ".$FileName.$([Guid]::NewGuid().ToString('N')).backup"
    )
    $CommitAttempted = $false
    $Published = $false
    try {
        [IO.File]::Copy($SourcePath, $TemporaryPath, $false)
        if ((Get-Item -LiteralPath $TemporaryPath).Length -ne
            (Get-Item -LiteralPath $SourcePath).Length) {
            throw "The staged project build export is incomplete: $TemporaryPath"
        }
        $CommitAttempted = $true
        if ([IO.File]::Exists($ExportPath)) {
            [IO.File]::Replace(
                $TemporaryPath,
                $ExportPath,
                $BackupPath,
                $true
            )
        } else {
            [IO.File]::Move($TemporaryPath, $ExportPath)
        }
        $Published = $true
    } catch {
        if ($CommitAttempted) {
            throw (
                "Atomic project build export failed for '$ExportPath'. " +
                'Recovery files were preserved when present: ' +
                "'$TemporaryPath', '$BackupPath'. $($_.Exception.Message)"
            )
        }
        throw
    } finally {
        $CleanupPaths = if ($Published) {
            @($TemporaryPath, $BackupPath)
        } elseif (-not $CommitAttempted) {
            @($TemporaryPath)
        } else {
            @()
        }
        foreach ($CleanupPath in $CleanupPaths) {
            if ([IO.File]::Exists($CleanupPath)) {
                try {
                    [IO.File]::Delete($CleanupPath)
                } catch {
                    Write-Warning (
                        'Project build export temporary file could not be ' +
                        "removed: $CleanupPath"
                    )
                }
            }
        }
    }

    $Export = Get-Item -LiteralPath $ExportPath
    Write-Host (
        "[EXPORTED] $($Export.FullName) ($($Export.Length) bytes)"
    ) -ForegroundColor Green
    Write-Output $Export.FullName
}
