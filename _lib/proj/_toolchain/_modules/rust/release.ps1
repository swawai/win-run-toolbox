Set-StrictMode -Version 2.0

function Get-ProjDevRustupCacheRoot {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $SourceKey = (Get-ProjDevSha256Text -Value ([string]::Join("`n", @(
        'swawkit.proj-dev.rustup-source.v0'
        [string]$Definition.RecipeVersion
        [string]$Definition.RustupInitUrl
        [string]$Definition.RustupInitChecksumUrl
    )))).Substring(0, 16)
    $HostRoot = Join-Path (
        Join-Path (Join-Path $Context.CacheRoot 'rust') 'rustup-init'
    ) ([string]$Definition.Host)
    return Join-Path $HostRoot (
        "$($Definition.RecipeVersion)-$SourceKey"
    )
}

function Read-ProjDevRustupChecksum {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not [IO.File]::Exists($Path)) {
        return ''
    }
    $Text = [IO.File]::ReadAllText(
        $Path,
        [Text.Encoding]::ASCII
    ).Trim()
    $Match = [regex]::Match($Text, '(?i)^([a-f0-9]{64})(?:\s|$)')
    if (-not $Match.Success) {
        return ''
    }
    return $Match.Groups[1].Value.ToLowerInvariant()
}

function Receive-ProjDevRustupChecksum {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][string]$ChecksumPath
    )

    if ([IO.File]::Exists($ChecksumPath) -or
        [IO.Directory]::Exists($ChecksumPath)) {
        Remove-ProjDevControlledPath `
            -Path $ChecksumPath `
            -DataRoot $Context.CacheDataRoot `
            -Activity 'refreshing the rustup checksum cache'
    }
    Invoke-ProjDevDownload `
        -Source ([string]$Definition.RustupInitChecksumUrl) `
        -Destination $ChecksumPath `
        -ControlledRoot $Context.CacheDataRoot
    $Expected = Read-ProjDevRustupChecksum -Path $ChecksumPath
    if ([string]::IsNullOrWhiteSpace($Expected)) {
        Remove-ProjDevControlledPath `
            -Path $ChecksumPath `
            -DataRoot $Context.CacheDataRoot `
            -Activity 'removing an invalid rustup checksum'
        throw 'The official rustup-init SHA-256 sidecar is invalid.'
    }
    return $Expected
}

function Get-ProjDevVerifiedRustupInstaller {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $CacheRoot = Get-ProjDevRustupCacheRoot `
        -Context $Context `
        -Definition $Definition
    $CacheRoot = Assert-ProjDevPathInsideDataRoot `
        -Path $CacheRoot `
        -DataRoot $Context.CacheDataRoot `
        -Activity 'using the rustup artifact cache'
    $ChecksumPath = Join-Path $CacheRoot 'rustup-init.exe.sha256'
    $InstallerPath = Join-Path $CacheRoot 'rustup-init.exe'
    $LockKey = Get-ProjDevSha256Text -Value ([string]::Join("`n", @(
        [string]$Definition.RustupInitUrl
        [string]$Definition.RustupInitChecksumUrl
    )))
    $Lock = Enter-ProjDevFileLock `
        -Path (Join-Path $Context.ArtifactLockRoot "rustup-$LockKey.lock") `
        -ControlledRoot $Context.CacheDataRoot
    try {
        if ([IO.File]::Exists($CacheRoot)) {
            Remove-ProjDevControlledPath `
                -Path $CacheRoot `
                -DataRoot $Context.CacheDataRoot `
                -Activity 'repairing the rustup cache root'
        }
        [void][IO.Directory]::CreateDirectory($CacheRoot)

        $Expected = Read-ProjDevRustupChecksum -Path $ChecksumPath
        if ([string]::IsNullOrWhiteSpace($Expected)) {
            $Expected = Receive-ProjDevRustupChecksum `
                -Context $Context `
                -Definition $Definition `
                -ChecksumPath $ChecksumPath
        }

        if ([IO.Directory]::Exists($InstallerPath)) {
            Remove-ProjDevControlledPath `
                -Path $InstallerPath `
                -DataRoot $Context.CacheDataRoot `
                -Activity 'repairing the rustup installer cache'
        }
        if ([IO.File]::Exists($InstallerPath) -and
            (Get-ProjDevFileSha256 -Path $InstallerPath) -cne $Expected) {
            Remove-ProjDevControlledPath `
                -Path $InstallerPath `
                -DataRoot $Context.CacheDataRoot `
                -Activity 'removing an unverified rustup installer'
        }
        if (-not [IO.File]::Exists($InstallerPath)) {
            Invoke-ProjDevDownload `
                -Source ([string]$Definition.RustupInitUrl) `
                -Destination $InstallerPath `
                -ControlledRoot $Context.CacheDataRoot
        }
        $Actual = Get-ProjDevFileSha256 -Path $InstallerPath
        if ($Actual -cne $Expected) {
            $Expected = Receive-ProjDevRustupChecksum `
                -Context $Context `
                -Definition $Definition `
                -ChecksumPath $ChecksumPath
        }
        if ($Actual -cne $Expected) {
            Remove-ProjDevControlledPath `
                -Path $InstallerPath `
                -DataRoot $Context.CacheDataRoot `
                -Activity 'removing a rustup installer with the wrong checksum'
            throw 'SHA-256 verification failed for rustup-init.exe.'
        }
        return [pscustomobject][ordered]@{
            Path = $InstallerPath
            Sha256 = $Expected
        }
    } finally {
        $Lock.Dispose()
    }
}
