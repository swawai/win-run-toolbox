Set-StrictMode -Version 2.0

function Remove-ProjRetiredCoreRuntimes {
    param([Parameter(Mandatory = $true)][string]$CacheDataRoot)

    $RetiredRoot = Assert-ProjDevPathInsideDataRoot `
        -Path (Join-Path $CacheDataRoot 'retired\core') `
        -DataRoot $CacheDataRoot `
        -Activity 'cleaning retired Core runtimes'
    if (-not [IO.Directory]::Exists($RetiredRoot)) {
        return @()
    }
    foreach ($File in Get-ChildItem -LiteralPath $RetiredRoot -File -Force) {
        try {
            [IO.File]::Delete($File.FullName)
        } catch [IO.IOException] {
            # A live process can still map this retired runtime. It becomes
            # deletable after that process exits and is retried next publish.
        } catch [UnauthorizedAccessException] {
            # Treat a still-mapped executable consistently across Windows
            # versions that report sharing failures as access failures.
        }
    }
    return @(
        Get-ChildItem -LiteralPath $RetiredRoot -File -Force |
            Select-Object -ExpandProperty FullName
    )
}

function Publish-ProjCoreRuntime {
    param(
        [Parameter(Mandatory = $true)][object]$Artifact,
        [Parameter(Mandatory = $true)][string]$ProjHome,
        [Parameter(Mandatory = $true)][string]$CacheDataRoot
    )

    $ProjHome = Get-ProjDevFullPath -Path $ProjHome
    $CacheDataRoot = Assert-ProjDevControlledRoot `
        -Root $CacheDataRoot `
        -Description 'shared project cache data root'
    $RuntimePath = Join-Path $ProjHome '_lib\proj\_bin\swawkit-proj.exe'
    $RuntimeDirectory = Split-Path -Path $RuntimePath -Parent
    $RuntimeDirectoryItem = Get-Item `
        -LiteralPath $RuntimeDirectory `
        -Force `
        -ErrorAction SilentlyContinue
    if ($null -eq $RuntimeDirectoryItem -or
        -not $RuntimeDirectoryItem.PSIsContainer -or
        ($RuntimeDirectoryItem.Attributes -band
            [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "The shared Core runtime directory is unsafe: $RuntimeDirectory"
    }
    $ExpectedHash = [string]$Artifact.Sha256
    if ($ExpectedHash -cnotmatch '^[a-f0-9]{64}$') {
        throw 'The project build artifact SHA-256 is invalid.'
    }
    $SourcePath = [string]$Artifact.Path
    if (-not [IO.File]::Exists($SourcePath)) {
        throw "The project build artifact is missing: $SourcePath"
    }

    $PublishLock = Enter-ProjDevFileLock `
        -Path (Join-Path $CacheDataRoot 'locks\core-publish.lock') `
        -ControlledRoot $CacheDataRoot `
        -TimeoutSeconds 120
    try {
        [void](Remove-ProjRetiredCoreRuntimes `
            -CacheDataRoot $CacheDataRoot)
        if ([IO.File]::Exists($RuntimePath)) {
            $CurrentHash = Get-ProjDevFileSha256 -Path $RuntimePath
            if ($CurrentHash -ceq $ExpectedHash) {
                Write-Host "[CURRENT] $RuntimePath ($ExpectedHash)" `
                    -ForegroundColor Green
                return [pscustomobject][ordered]@{
                    Path = $RuntimePath
                    Sha256 = $ExpectedHash
                    Changed = $false
                }
            }
        }

        $Token = [Guid]::NewGuid().ToString('N')
        $StagedPath = Join-Path $RuntimeDirectory (
            ".swawkit-proj.$Token.tmp"
        )
        $RetiredPath = $null
        $CommitAttempted = $false
        $Published = $false
        try {
            [IO.File]::Copy($SourcePath, $StagedPath, $false)
            $Staged = Get-Item -LiteralPath $StagedPath
            if ([long]$Staged.Length -ne [long]$Artifact.Length -or
                (Get-ProjDevFileSha256 -Path $StagedPath) -cne $ExpectedHash) {
                throw 'The staged Core runtime does not match the build manifest.'
            }
            $CommitAttempted = $true
            if ([IO.File]::Exists($RuntimePath)) {
                $RetiredRoot = Assert-ProjDevPathInsideDataRoot `
                    -Path (Join-Path $CacheDataRoot 'retired\core') `
                    -DataRoot $CacheDataRoot `
                    -Activity 'retiring the previous Core runtime'
                [void][IO.Directory]::CreateDirectory($RetiredRoot)
                $RetiredPath = Join-Path $RetiredRoot (
                    "$CurrentHash-$Token.exe"
                )
                [IO.File]::Replace(
                    $StagedPath,
                    $RuntimePath,
                    $RetiredPath,
                    $true
                )
            } else {
                [IO.File]::Move($StagedPath, $RuntimePath)
            }
            if ((Get-ProjDevFileSha256 -Path $RuntimePath) -cne
                $ExpectedHash) {
                throw 'The published Core runtime failed SHA-256 verification.'
            }
            $Published = $true
        } catch {
            if ($CommitAttempted) {
                throw (
                    "Atomic Core publication failed for '$RuntimePath'. " +
                    'The verified build Export and any retired runtime were ' +
                    "preserved for recovery. $($_.Exception.Message)"
                )
            }
            throw
        } finally {
            $CleanupPaths = if ($Published -or -not $CommitAttempted) {
                @($StagedPath)
            } else {
                @()
            }
            foreach ($CleanupPath in $CleanupPaths) {
                if ([IO.File]::Exists($CleanupPath)) {
                    try {
                        [IO.File]::Delete($CleanupPath)
                    } catch {
                        Write-Warning (
                            'Core publication temporary file could not be ' +
                            "removed: $CleanupPath"
                        )
                    }
                }
            }
        }

        $Retired = @(Remove-ProjRetiredCoreRuntimes `
            -CacheDataRoot $CacheDataRoot)
        if ($Retired.Count -gt 0) {
            Write-Host (
                "[RETIRED] $($Retired.Count) old Core runtime(s) remain " +
                'mapped by live processes and will be retried next publish.'
            ) -ForegroundColor Yellow
        }
        Write-Host "[PUBLISHED] $RuntimePath ($ExpectedHash)" `
            -ForegroundColor Green
        return [pscustomobject][ordered]@{
            Path = $RuntimePath
            Sha256 = $ExpectedHash
            Changed = $true
        }
    } finally {
        $PublishLock.Dispose()
    }
}
