Set-StrictMode -Version 2.0

function ConvertTo-ProjDevMsvcPayload {
    param(
        [Parameter(Mandatory = $true)][object]$Payload,
        [Parameter(Mandatory = $true)][string]$Description
    )

    $FileName = [string]$Payload.fileName
    $Sha256 = ([string]$Payload.sha256).Trim().ToLowerInvariant()
    $UrlText = [string]$Payload.url
    $Uri = $null
    if ([string]::IsNullOrWhiteSpace($FileName) -or
        $Sha256 -cnotmatch '^[a-f0-9]{64}$' -or
        -not [Uri]::TryCreate($UrlText, [UriKind]::Absolute, [ref]$Uri) -or
        $Uri.Scheme -cne 'https' -or
        $Uri.Host -cne 'download.visualstudio.microsoft.com') {
        throw "Invalid Microsoft payload for $Description."
    }
    $Size = 0L
    if ($null -ne $Payload.PSObject.Properties['size']) {
        $Size = [long]$Payload.size
        if ($Size -le 0) {
            throw "Microsoft payload has an invalid size for $Description."
        }
    }
    return [pscustomobject][ordered]@{
        FileName = $FileName
        LeafName = [IO.Path]::GetFileName($FileName)
        Sha256 = $Sha256
        Size = $Size
        Url = $Uri.AbsoluteUri
    }
}

function Get-ProjDevMsvcPayloadCacheRoot {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    return Join-Path (
        Join-Path (Join-Path $Context.CacheRoot 'msvc') (
            [string]$Definition.Channel
        )
    ) 'payloads'
}

function Get-ProjDevMsvcVerifiedPayload {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Payload
    )

    $CacheRoot = Get-ProjDevMsvcPayloadCacheRoot `
        -Context $Context `
        -Definition $Definition
    $SafeName = [string]$Payload.LeafName
    if ([string]::IsNullOrWhiteSpace($SafeName) -or
        $SafeName -in @('.', '..') -or
        $SafeName.IndexOfAny([IO.Path]::GetInvalidFileNameChars()) -ge 0) {
        throw "Invalid Microsoft payload file name: $SafeName"
    }
    $PayloadSha256 = ([string]$Payload.Sha256).Trim().ToLowerInvariant()
    if ($PayloadSha256 -cnotmatch '^[a-f0-9]{64}$') {
        throw "Invalid Microsoft payload SHA-256 for: $SafeName"
    }
    $PayloadRoot = Join-Path $CacheRoot $PayloadSha256
    $PayloadRoot = Assert-ProjDevPathInsideDataRoot `
        -Path $PayloadRoot `
        -DataRoot $Context.CacheDataRoot `
        -Activity 'using the MSVC payload cache'
    $Lock = Enter-ProjDevFileLock `
        -Path (Join-Path `
            $Context.ArtifactLockRoot `
            "msvc-$PayloadSha256.lock") `
        -ControlledRoot $Context.CacheDataRoot
    try {
        if ([IO.File]::Exists($PayloadRoot)) {
            Remove-ProjDevControlledPath `
                -Path $PayloadRoot `
                -DataRoot $Context.CacheDataRoot `
                -Activity 'repairing an invalid MSVC payload cache root'
        }
        [void][IO.Directory]::CreateDirectory($PayloadRoot)
        $Path = Join-Path $PayloadRoot $SafeName
        if ([IO.Directory]::Exists($Path)) {
            Remove-ProjDevControlledPath `
                -Path $Path `
                -DataRoot $Context.CacheDataRoot `
                -Activity 'repairing an invalid MSVC payload cache'
        }
        if ([IO.File]::Exists($Path)) {
            $Valid = (Get-ProjDevFileSha256 -Path $Path) -ceq
                $PayloadSha256
            if (-not $Valid) {
                Remove-ProjDevControlledPath `
                    -Path $Path `
                    -DataRoot $Context.CacheDataRoot `
                    -Activity 'removing a corrupt MSVC payload'
            }
        }
        if (-not [IO.File]::Exists($Path)) {
            Invoke-ProjDevDownload `
                -Source ([string]$Payload.Url) `
                -Destination $Path `
                -ControlledRoot $Context.CacheDataRoot
        }
        if ((Get-ProjDevFileSha256 -Path $Path) -cne
            $PayloadSha256) {
            Remove-ProjDevControlledPath `
                -Path $Path `
                -DataRoot $Context.CacheDataRoot `
                -Activity 'removing an unverified MSVC payload'
            throw "Microsoft payload verification failed: $SafeName"
        }
        return $Path
    } finally {
        $Lock.Dispose()
    }
}

function Copy-ProjDevMsvcPayloadToSourceRoot {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Payload,
        [Parameter(Mandatory = $true)][string]$VerifiedPath,
        [Parameter(Mandatory = $true)][string]$SourceRoot
    )

    $SourceRoot = Assert-ProjDevPathInsideDataRoot `
        -Path $SourceRoot `
        -DataRoot $Context.DataRoot `
        -Activity 'staging MSVC installer sources'
    [void][IO.Directory]::CreateDirectory($SourceRoot)
    $Destination = Resolve-ProjDevChildPath `
        -Root $SourceRoot `
        -RelativePath ([string]$Payload.LeafName) `
        -Description 'MSVC installer source'
    $ExpectedSha256 = ([string]$Payload.Sha256).Trim().ToLowerInvariant()
    if ([IO.File]::Exists($Destination)) {
        if ((Get-ProjDevFileSha256 -Path $Destination) -cne
            $ExpectedSha256) {
            throw (
                'Conflicting MSVC installer source file: ' +
                [string]$Payload.LeafName
            )
        }
        return $Destination
    }
    [IO.File]::Copy($VerifiedPath, $Destination, $false)
    if ((Get-ProjDevFileSha256 -Path $Destination) -cne $ExpectedSha256) {
        Remove-ProjDevControlledPath `
            -Path $Destination `
            -DataRoot $Context.DataRoot `
            -Activity 'removing an invalid MSVC installer source copy'
        throw "MSVC installer source copy verification failed: $Destination"
    }
    return $Destination
}

function Expand-ProjDevMsvcVsix {
    param(
        [Parameter(Mandatory = $true)][string]$ArchivePath,
        [Parameter(Mandatory = $true)][string]$Destination,
        [Parameter(Mandatory = $true)][string]$ControlledRoot
    )

    $Destination = Assert-ProjDevPathInsideDataRoot `
        -Path $Destination `
        -DataRoot $ControlledRoot `
        -Activity 'extracting an MSVC VSIX'
    $Archive = [IO.Compression.ZipFile]::OpenRead($ArchivePath)
    $EntryCount = 0
    [long]$TotalBytes = 0
    try {
        foreach ($Entry in $Archive.Entries) {
            if (-not $Entry.FullName.StartsWith(
                'Contents/',
                [StringComparison]::OrdinalIgnoreCase
            )) {
                continue
            }
            $EntryCount++
            $TotalBytes += [long]$Entry.Length
            if ($EntryCount -gt 200000 -or
                $Entry.Length -gt 4GB -or
                $TotalBytes -gt 12GB) {
                throw 'MSVC VSIX exceeds the extraction safety limits.'
            }
            $Encoded = $Entry.FullName.Substring('Contents/'.Length)
            if ([string]::IsNullOrWhiteSpace($Encoded)) {
                continue
            }
            $RelativePath = [Uri]::UnescapeDataString($Encoded).Replace(
                '/',
                [IO.Path]::DirectorySeparatorChar
            )
            $Target = Resolve-ProjDevChildPath `
                -Root $Destination `
                -RelativePath $RelativePath `
                -Description 'MSVC VSIX entry'
            if ($Entry.FullName.EndsWith('/')) {
                [void][IO.Directory]::CreateDirectory($Target)
                continue
            }
            [void][IO.Directory]::CreateDirectory(
                (Split-Path -Path $Target -Parent)
            )
            $Input = $Entry.Open()
            try {
                $Output = [IO.File]::Open(
                    $Target,
                    [IO.FileMode]::Create,
                    [IO.FileAccess]::Write,
                    [IO.FileShare]::None
                )
                try {
                    $Input.CopyTo($Output)
                } finally {
                    $Output.Dispose()
                }
            } finally {
                $Input.Dispose()
            }
        }
    } finally {
        $Archive.Dispose()
    }
    if ($EntryCount -eq 0) {
        throw "VSIX has no Contents payload: $ArchivePath"
    }
}

function Get-ProjDevMsvcCabNames {
    param(
        [Parameter(Mandatory = $true)][string]$MsiPath,
        [Parameter(Mandatory = $true)][string[]]$CandidateNames
    )

    $Bytes = [IO.File]::ReadAllBytes($MsiPath)
    $Text = [Text.Encoding]::ASCII.GetString($Bytes)
    $Names = foreach ($Candidate in $CandidateNames) {
        if ([string]::IsNullOrWhiteSpace($Candidate) -or
            [IO.Path]::GetExtension($Candidate) -ine '.cab' -or
            [IO.Path]::GetFileName($Candidate) -cne $Candidate) {
            throw "Invalid Windows SDK CAB candidate: $Candidate"
        }
        if ($Text.IndexOf(
            $Candidate,
            [StringComparison]::OrdinalIgnoreCase
        ) -ge 0) {
            $Candidate
        }
    }
    return [string[]]@($Names | Sort-Object -Unique)
}

function Invoke-ProjDevMsvcAdministrativeInstall {
    param(
        [Parameter(Mandatory = $true)][string]$MsiPath,
        [Parameter(Mandatory = $true)][string]$Destination,
        [Parameter(Mandatory = $true)][string]$LogPath,
        [Parameter(Mandatory = $true)][string]$ControlledRoot
    )

    $Destination = Assert-ProjDevPathInsideDataRoot `
        -Path $Destination `
        -DataRoot $ControlledRoot `
        -Activity 'installing an MSVC payload'
    $LogPath = Assert-ProjDevPathInsideDataRoot `
        -Path $LogPath `
        -DataRoot $ControlledRoot `
        -Activity 'writing an MSVC installation log'
    $MsiExec = Join-Path $env:SystemRoot 'System32\msiexec.exe'
    if (-not [IO.File]::Exists($MsiExec)) {
        throw "Windows Installer is unavailable: $MsiExec"
    }
    [void][IO.Directory]::CreateDirectory($Destination)
    [void][IO.Directory]::CreateDirectory(
        (Split-Path -Path $LogPath -Parent)
    )
    if ([IO.File]::Exists($LogPath)) {
        Remove-ProjDevControlledPath `
            -Path $LogPath `
            -DataRoot $ControlledRoot `
            -Activity 'replacing an MSVC installation log'
    }
    $Info = [Diagnostics.ProcessStartInfo]::new()
    $Info.FileName = $MsiExec
    $Info.Arguments = (
        "/a `"$MsiPath`" /quiet /qn TARGETDIR=`"$Destination`" " +
        "/l*v `"$LogPath`""
    )
    $Info.UseShellExecute = $false
    $Info.CreateNoWindow = $true
    $Process = [Diagnostics.Process]::Start($Info)
    if ($null -eq $Process) {
        throw "Failed to start Windows Installer for: $MsiPath"
    }
    try {
        if (-not $Process.WaitForExit(600000)) {
            try { $Process.Kill() } catch {}
            try { [void]$Process.WaitForExit(5000) } catch {}
            throw "Windows Installer timed out for: $MsiPath"
        }
        if ($Process.ExitCode -ne 0) {
            throw (
                "Windows Installer exited with code $($Process.ExitCode) " +
                "for: $MsiPath. Diagnostic log: $LogPath"
            )
        }
    } finally {
        $Process.Dispose()
    }
    if ([IO.File]::Exists($LogPath)) {
        Remove-ProjDevControlledPath `
            -Path $LogPath `
            -DataRoot $ControlledRoot `
            -Activity 'cleaning an MSVC installation log'
    }
}
