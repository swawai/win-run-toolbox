Set-StrictMode -Version 2.0

function Get-ProjDevMsvcPackageMatches {
    param(
        [Parameter(Mandatory = $true)][object[]]$Packages,
        [Parameter(Mandatory = $true)][string]$Id
    )

    return @($Packages | Where-Object {
        [string]$_.id -ieq $Id
    })
}

function Get-ProjDevMsvcPackage {
    param(
        [Parameter(Mandatory = $true)][object[]]$Packages,
        [Parameter(Mandatory = $true)][string]$Id,
        [AllowNull()][string]$Language = $null
    )

    $Matches = Get-ProjDevMsvcPackageMatches `
        -Packages $Packages `
        -Id $Id
    if ([string]::IsNullOrWhiteSpace($Language)) {
        $Matches = @($Matches | Where-Object {
            $Property = $_.PSObject.Properties['language']
            $null -eq $Property -or
                [string]::IsNullOrWhiteSpace([string]$Property.Value)
        })
    } else {
        $Matches = @($Matches | Where-Object {
            $Property = $_.PSObject.Properties['language']
            $null -ne $Property -and [string]$Property.Value -ieq $Language
        })
    }
    if ($Matches.Count -ne 1) {
        $Suffix = if ([string]::IsNullOrWhiteSpace($Language)) {
            ''
        } else {
            " for language '$Language'"
        }
        throw "Expected one Microsoft package '$Id'$Suffix; found $($Matches.Count)."
    }
    return $Matches[0]
}

function Get-ProjDevMsvcPackagePayloads {
    param(
        [Parameter(Mandatory = $true)][object]$Package,
        [Parameter(Mandatory = $true)][string]$Description
    )

    $Payloads = @($Package.payloads)
    if ($Payloads.Count -eq 0) {
        throw "Microsoft package has no payloads: $Description"
    }
    return @($Payloads | ForEach-Object {
        ConvertTo-ProjDevMsvcPayload `
            -Payload $_ `
            -Description $Description
    })
}

function Resolve-ProjDevMsvcManifest {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$ChannelData,
        [Parameter(Mandatory = $true)][object]$VisualStudioManifest
    )

    $Items = @($ChannelData.channelItems | Where-Object {
        [string]$_.id -ceq [string]$Definition.VisualStudioManifestId
    })
    if ($Items.Count -ne 1 -or @($Items[0].payloads).Count -ne 1) {
        throw (
            "VS channel must contain exactly one " +
            "'$($Definition.VisualStudioManifestId)' payload."
        )
    }
    $ManifestPayload = ConvertTo-ProjDevMsvcPayload `
        -Payload $Items[0].payloads[0] `
        -Description 'Visual Studio manifest'
    $Packages = [object[]]@($VisualStudioManifest.packages)
    if ($Packages.Count -eq 0) {
        throw 'The Visual Studio manifest contains no packages.'
    }

    $ToolCandidates = foreach ($Package in $Packages) {
        $Match = [regex]::Match(
            ([string]$Package.id).ToLowerInvariant(),
            '^microsoft\.vc\.(\d+\.\d+\.\d+\.\d+)\.' +
                'tools\.hostx64\.targetx64\.base$'
        )
        if ($Match.Success) {
            [pscustomobject]@{
                Version = [version]$Match.Groups[1].Value
                Text = $Match.Groups[1].Value
            }
        }
    }
    $Tool = @($ToolCandidates | Sort-Object Version -Descending |
        Select-Object -First 1)
    if ($Tool.Count -ne 1) {
        throw 'No x64 MSVC tool package was found in the VS manifest.'
    }

    $ToolPayloads = [Collections.Generic.List[object]]::new()
    foreach ($Template in [string[]]$Definition.ToolPackageTemplates) {
        $Id = $Template.Replace('{tool}', [string]$Tool[0].Text)
        $Language = if ($Id.EndsWith(
            '.res.base',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            [string]$Definition.ResourceLanguage
        } else {
            $null
        }
        $Package = Get-ProjDevMsvcPackage `
            -Packages $Packages `
            -Id $Id `
            -Language $Language
        foreach ($Payload in Get-ProjDevMsvcPackagePayloads `
            -Package $Package `
            -Description $Id) {
            $ToolPayloads.Add($Payload)
        }
    }

    $SdkCandidates = foreach ($Package in $Packages) {
        $Match = [regex]::Match(
            ([string]$Package.id).ToLowerInvariant(),
            '^microsoft\.visualstudio\.component\.windows1[01]sdk\.(\d+)$'
        )
        if ($Match.Success) {
            [pscustomobject]@{
                Number = [long]$Match.Groups[1].Value
                Package = $Package
            }
        }
    }
    $SdkComponent = @($SdkCandidates | Sort-Object Number -Descending |
        Select-Object -First 1)
    if ($SdkComponent.Count -ne 1) {
        throw 'No Windows 10/11 SDK component was found in the VS manifest.'
    }
    $Dependencies = @(
        $SdkComponent[0].Package.dependencies.PSObject.Properties.Name |
            Where-Object { $_ -match '^Win1[01]SDK_' }
    )
    if ($Dependencies.Count -ne 1) {
        throw (
            "SDK component '$($SdkComponent[0].Package.id)' must identify " +
            'exactly one Windows SDK package.'
        )
    }
    $SdkPackage = Get-ProjDevMsvcPackage `
        -Packages $Packages `
        -Id ([string]$Dependencies[0])
    $SdkPayloads = Get-ProjDevMsvcPackagePayloads `
        -Package $SdkPackage `
        -Description ([string]$SdkPackage.id)
    $MsiPayloads = [Collections.Generic.List[object]]::new()
    foreach ($MsiName in [string[]]$Definition.SdkMsiNames) {
        $Expected = "Installers\$MsiName"
        $Matches = @($SdkPayloads | Where-Object {
            [string]$_.FileName -ieq $Expected
        })
        if ($Matches.Count -ne 1) {
            throw "Expected one Windows SDK payload '$Expected'; found $($Matches.Count)."
        }
        $MsiPayloads.Add($Matches[0])
    }

    return [pscustomobject][ordered]@{
        ManifestUrl = [string]$ManifestPayload.Url
        ManifestIdentitySha256 = [string]$ManifestPayload.Sha256
        ManifestSha256 = ''
        ManifestSize = [long]$ManifestPayload.Size
        ToolPackageVersion = [string]$Tool[0].Text
        ToolPayloads = [object[]]$ToolPayloads.ToArray()
        SdkComponentId = [string]$SdkComponent[0].Package.id
        SdkPackageId = [string]$SdkPackage.id
        SdkPayloads = [object[]]$SdkPayloads
        MsiPayloads = [object[]]$MsiPayloads.ToArray()
    }
}

function Get-ProjDevMsvcProductManifestPath {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Payload,
        [switch]$Refresh
    )

    $Root = Join-Path (
        Join-Path (Join-Path $Context.CacheRoot 'msvc') (
            [string]$Definition.Channel
        )
    ) 'manifests'
    $Root = Assert-ProjDevPathInsideDataRoot `
        -Path $Root `
        -DataRoot $Context.CacheDataRoot `
        -Activity 'using the MSVC manifest cache'
    [void][IO.Directory]::CreateDirectory($Root)
    $ManifestIdentity = ([string]$Payload.Sha256).Trim().ToLowerInvariant()
    if ($ManifestIdentity -cnotmatch '^[a-f0-9]{64}$') {
        throw 'The Visual Studio product manifest has an invalid identity.'
    }
    $Path = Join-Path $Root (
        "VisualStudio.$($ManifestIdentity.Substring(0, 16)).vsman"
    )
    $DigestPath = "$Path.actual.sha256"
    $Lock = Enter-ProjDevFileLock `
        -Path (Join-Path $Context.ArtifactLockRoot (
            "msvc-manifest-$ManifestIdentity.lock"
        )) `
        -ControlledRoot $Context.CacheDataRoot
    try {
        if ($Refresh) {
            foreach ($RefreshPath in @($Path, $DigestPath)) {
                Remove-ProjDevControlledPath `
                    -Path $RefreshPath `
                    -DataRoot $Context.CacheDataRoot `
                    -Activity 'refreshing the Visual Studio product manifest'
            }
        }
        if ([IO.File]::Exists($Path) -and
            [IO.File]::Exists($DigestPath)) {
            $RecordedDigest = [IO.File]::ReadAllText($DigestPath).
                Trim().ToLowerInvariant()
            if ($RecordedDigest -cnotmatch '^[a-f0-9]{64}$' -or
                (Get-ProjDevFileSha256 -Path $Path) -cne $RecordedDigest) {
                foreach ($CorruptPath in @($Path, $DigestPath)) {
                    Remove-ProjDevControlledPath `
                        -Path $CorruptPath `
                        -DataRoot $Context.CacheDataRoot `
                        -Activity (
                            'removing a corrupt Visual Studio product manifest'
                        )
                }
            }
        }
        if (-not [IO.File]::Exists($Path)) {
            Invoke-ProjDevDownload `
                -Source ([string]$Payload.Url) `
                -Destination $Path `
                -ControlledRoot $Context.CacheDataRoot
        }
        if (-not [IO.File]::Exists($Path) -or
            (Get-Item -LiteralPath $Path).Length -le 0) {
            throw 'The downloaded Visual Studio product manifest is empty.'
        }
        Write-ProjDevTextAtomic `
            -Path $DigestPath `
            -Content "$(Get-ProjDevFileSha256 -Path $Path)`r`n" `
            -ControlledRoot $Context.CacheDataRoot
        return $Path
    } finally {
        $Lock.Dispose()
    }
}

function Read-ProjDevMsvcJsonFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Description
    )

    try {
        return [IO.File]::ReadAllText(
            $Path,
            [Text.Encoding]::UTF8
        ) | ConvertFrom-Json
    } catch {
        throw "Cannot parse $Description JSON: $($_.Exception.Message)"
    }
}

function Remove-ProjDevMsvcChannelRefreshResidues {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$ControlledRoot
    )

    $Root = Assert-ProjDevPathInsideDataRoot `
        -Path $Root `
        -DataRoot $ControlledRoot `
        -Activity 'cleaning MSVC channel refresh residues'
    if (-not [IO.Directory]::Exists($Root)) {
        return
    }
    foreach ($Item in Get-ChildItem -LiteralPath $Root -File -Force) {
        if ([string]$Item.Name -cnotmatch
            '^\.channel-[a-f0-9]{32}\.json$') {
            continue
        }
        Remove-ProjDevControlledPath `
            -Path $Item.FullName `
            -DataRoot $ControlledRoot `
            -Activity 'cleaning an interrupted MSVC channel refresh'
    }
}

function Get-ProjDevMsvcChannelData {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $Root = Join-Path (
        Join-Path (Join-Path $Context.CacheRoot 'msvc') (
            [string]$Definition.Channel
        )
    ) 'manifests'
    $Root = Assert-ProjDevPathInsideDataRoot `
        -Path $Root `
        -DataRoot $Context.CacheDataRoot `
        -Activity 'using the MSVC channel cache'
    [void][IO.Directory]::CreateDirectory($Root)
    $CachedPath = Join-Path $Root 'channel.json'
    $RefreshPath = Join-Path $Root (
        ".channel-$([Guid]::NewGuid().ToString('N')).json"
    )
    $Lock = Enter-ProjDevFileLock `
        -Path (Join-Path $Context.ArtifactLockRoot (
            "msvc-channel-$($Definition.Channel).lock"
        )) `
        -ControlledRoot $Context.CacheDataRoot
    try {
        Remove-ProjDevMsvcChannelRefreshResidues `
            -Root $Root `
            -ControlledRoot $Context.CacheDataRoot
        try {
            Invoke-ProjDevDownload `
                -Source ([string]$Definition.ChannelUrl) `
                -Destination $RefreshPath `
                -ControlledRoot $Context.CacheDataRoot
            $Data = Read-ProjDevMsvcJsonFile `
                -Path $RefreshPath `
                -Description 'Visual Studio channel'
            Write-ProjDevTextAtomic `
                -Path $CachedPath `
                -Content (ConvertTo-ProjDevJsonText -Value $Data) `
                -ControlledRoot $Context.CacheDataRoot
            return $Data
        } catch {
            if (-not [IO.File]::Exists($CachedPath)) {
                throw (
                    'Cannot refresh the Visual Studio channel and no cached ' +
                    "channel is available: $($_.Exception.Message)"
                )
            }
            Write-Warning (
                'Using the cached Visual Studio channel because refresh ' +
                "failed: $($_.Exception.Message)"
            )
            return Read-ProjDevMsvcJsonFile `
                -Path $CachedPath `
                -Description 'cached Visual Studio channel'
        }
    } finally {
        try {
            if ([IO.File]::Exists($RefreshPath)) {
                Remove-ProjDevControlledPath `
                    -Path $RefreshPath `
                    -DataRoot $Context.CacheDataRoot `
                    -Activity 'cleaning an MSVC channel refresh'
            }
        } finally {
            $Lock.Dispose()
        }
    }
}

function Resolve-ProjDevMsvcRelease {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $ChannelData = Get-ProjDevMsvcChannelData `
        -Context $Context `
        -Definition $Definition
    $Items = @($ChannelData.channelItems | Where-Object {
        [string]$_.id -ceq [string]$Definition.VisualStudioManifestId
    })
    if ($Items.Count -ne 1 -or @($Items[0].payloads).Count -ne 1) {
        throw 'The Visual Studio channel does not declare one product manifest.'
    }
    $ManifestPayload = ConvertTo-ProjDevMsvcPayload `
        -Payload $Items[0].payloads[0] `
        -Description 'Visual Studio manifest'
    $ManifestPath = Get-ProjDevMsvcProductManifestPath `
        -Context $Context `
        -Definition $Definition `
        -Payload $ManifestPayload
    try {
        $VisualStudioManifest = Read-ProjDevMsvcJsonFile `
            -Path $ManifestPath `
            -Description 'Visual Studio manifest'
    } catch {
        $ManifestPath = Get-ProjDevMsvcProductManifestPath `
            -Context $Context `
            -Definition $Definition `
            -Payload $ManifestPayload `
            -Refresh
        $VisualStudioManifest = Read-ProjDevMsvcJsonFile `
            -Path $ManifestPath `
            -Description 'Visual Studio manifest'
    }
    $Recipe = Resolve-ProjDevMsvcManifest `
        -Definition $Definition `
        -ChannelData $ChannelData `
        -VisualStudioManifest $VisualStudioManifest
    $Recipe.ManifestSha256 = Get-ProjDevFileSha256 -Path $ManifestPath
    return $Recipe
}
