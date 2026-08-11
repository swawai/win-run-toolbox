Set-StrictMode -Version 2.0

$script:ProjBuildReleaseManifestSchema = 'swawkit.proj-build-release-set/v1'
$script:ProjRuntimeReleaseManifestSchema = 'swawkit.proj-release-set/v1'

function New-ProjBuildReleaseDescriptor {
    param(
        [Parameter(Mandatory = $true)]
        [Collections.IDictionary]$Artifacts,
        [Parameter(Mandatory = $true)][string]$CommandDataRoot
    )

    if ($Artifacts.Count -lt 1) {
        throw 'A build Release Set must contain at least one artifact.'
    }
    $Records = [Collections.Generic.List[object]]::new()
    $IdentityLines = [Collections.Generic.List[string]]::new()
    $IdentityLines.Add($script:ProjRuntimeReleaseManifestSchema)
    foreach ($Name in [string[]]@($Artifacts.Keys | Sort-Object)) {
        if ($Name -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._+-]*$') {
            throw "Invalid Release Set artifact name: '$Name'"
        }
        $Path = Assert-ProjDevPathInsideDataRoot `
            -Path ([string]$Artifacts[$Name]) `
            -DataRoot $CommandDataRoot `
            -Activity "reading the '$Name' build candidate"
        if (-not [IO.File]::Exists($Path)) {
            throw "The Release Set build candidate is missing: $Path"
        }
        $Item = Get-Item -LiteralPath $Path
        if ($Item.Length -le 0 -or
            ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "The Release Set build candidate is invalid: $Path"
        }
        $Hash = Get-ProjDevFileSha256 -Path $Path
        $Record = [pscustomobject][ordered]@{
            name = $Name
            length = [long]$Item.Length
            sha256 = $Hash
            path = $Path
        }
        $Records.Add($Record)
        $IdentityLines.Add($Name)
        $IdentityLines.Add(([long]$Item.Length).ToString(
            [Globalization.CultureInfo]::InvariantCulture
        ))
        $IdentityLines.Add($Hash)
    }
    $ReleaseId = Get-ProjDevSha256Text `
        -Value ([string]::Join("`n", $IdentityLines))
    return [pscustomobject][ordered]@{
        ReleaseId = $ReleaseId
        InputRevision = 'sha256-' + $ReleaseId
        Artifacts = [object[]]$Records
    }
}

function Publish-ProjBuildReleaseSet {
    param(
        [Parameter(Mandatory = $true)]
        [Collections.IDictionary]$Artifacts,
        [Parameter(Mandatory = $true)][string]$CommandDataRoot,
        [Parameter(Mandatory = $true)][string]$ProducerAddress,
        [Parameter(Mandatory = $true)][string]$ProducerContract
    )

    if ($ProducerAddress -cnotmatch
        '^[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*)*$') {
        throw "Invalid build producer address: '$ProducerAddress'"
    }
    if ($ProducerContract -cnotmatch '^[a-z0-9][a-z0-9._/-]{0,127}$') {
        throw "Invalid build producer contract: '$ProducerContract'"
    }
    $Descriptor = New-ProjBuildReleaseDescriptor `
        -Artifacts $Artifacts `
        -CommandDataRoot $CommandDataRoot
    $Token = [Guid]::NewGuid().ToString('N').ToLowerInvariant()
    $StateContext = [pscustomobject][ordered]@{
        DataRoot = $CommandDataRoot
        ProviderStatePath = Join-Path $CommandDataRoot '_state.json'
    }
    Write-ProjCommandProviderState `
        -Context $StateContext `
        -State (New-ProjCommandProviderUnavailableState `
            -InputRevision $Descriptor.InputRevision `
            -Token $Token)

    $ExportRoot = Assert-ProjDevPathInsideDataRoot `
        -Path (Join-Path $CommandDataRoot 'export') `
        -DataRoot $CommandDataRoot `
        -Activity 'publishing a build Release Set'
    [void][IO.Directory]::CreateDirectory($ExportRoot)
    $ReleasesRoot = Join-Path $ExportRoot 'releases'
    [void][IO.Directory]::CreateDirectory($ReleasesRoot)
    $ReleaseRoot = Join-Path $ReleasesRoot $Descriptor.ReleaseId
    if ([IO.Directory]::Exists($ReleaseRoot)) {
        [void](Read-ProjBuildReleaseDirectory `
            -ReleaseRoot $ReleaseRoot `
            -ReleaseId $Descriptor.ReleaseId `
            -ArtifactNames ([string[]]$Artifacts.Keys) `
            -ControlledRoot $CommandDataRoot)
    } else {
        $StageRoot = Join-Path $ExportRoot ".release.$Token.tmp"
        [void][IO.Directory]::CreateDirectory($StageRoot)
        $Committed = $false
        try {
            foreach ($Artifact in $Descriptor.Artifacts) {
                $Destination = Join-Path $StageRoot ([string]$Artifact.name)
                [IO.File]::Copy([string]$Artifact.path, $Destination, $false)
                if ((Get-ProjDevFileSha256 -Path $Destination) -cne
                    [string]$Artifact.sha256) {
                    throw "The staged Release Set artifact is corrupt: $Destination"
                }
            }
            $Manifest = [ordered]@{
                schema = $script:ProjBuildReleaseManifestSchema
                runtimeSchema = $script:ProjRuntimeReleaseManifestSchema
                releaseId = $Descriptor.ReleaseId
                artifacts = @($Descriptor.Artifacts | ForEach-Object {
                    [ordered]@{
                        name = [string]$_.name
                        length = [long]$_.length
                        sha256 = [string]$_.sha256
                    }
                })
            }
            [IO.File]::WriteAllText(
                (Join-Path $StageRoot 'manifest.json'),
                (ConvertTo-ProjDevJsonText -Value $Manifest),
                [Text.UTF8Encoding]::new($false)
            )
            [IO.Directory]::Move($StageRoot, $ReleaseRoot)
            $Committed = $true
        } finally {
            if (-not $Committed -and [IO.Directory]::Exists($StageRoot)) {
                [IO.Directory]::Delete($StageRoot, $true)
            }
        }
    }
    Write-ProjDevTextAtomic `
        -Path (Join-Path $ExportRoot 'current') `
        -Content ($Descriptor.ReleaseId + "`n") `
        -ControlledRoot $CommandDataRoot `
        -Encoding ([Text.UTF8Encoding]::new($false))
    Write-ProjCommandProviderState `
        -Context $StateContext `
        -State (New-ProjCommandProviderReadyState `
            -InputRevision $Descriptor.InputRevision `
            -Token $Token `
            -ProducerContract $ProducerContract)
    Write-Host "[READY] $ProducerAddress release $($Descriptor.ReleaseId)" `
        -ForegroundColor Green
    return Read-ProjBuildReleaseDirectory `
        -ReleaseRoot $ReleaseRoot `
        -ReleaseId $Descriptor.ReleaseId `
        -ArtifactNames ([string[]]$Artifacts.Keys) `
        -ControlledRoot $CommandDataRoot
}

function Get-ProjRequiredBuildReleaseSet {
    param(
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$ProviderAddress,
        [Parameter(Mandatory = $true)][string]$EntryCommand,
        [Parameter(Mandatory = $true)][string]$ProducerContract,
        [Parameter(Mandatory = $true)][string[]]$ArtifactNames
    )

    $Publication = Get-ProjReadyCommandExport `
        -DataRoot $DataRoot `
        -ProviderAddress $ProviderAddress `
        -EntryCommand $EntryCommand `
        -ProducerContract $ProducerContract `
        -ProviderSource action
    $CurrentPath = Join-Path $Publication.ExportRoot 'current'
    try {
        $ReleaseId = (Read-ProjDevReplaceableUtf8Text -Path $CurrentPath).TrimEnd("`r", "`n")
    } catch {
        $ReleaseId = ''
    }
    if ($ReleaseId -cnotmatch '^[a-f0-9]{64}$' -or
        [string]$Publication.InputRevision -cne ('sha256-' + $ReleaseId)) {
        throw "Required Release Set from '$ProviderAddress' is invalid. Run '$EntryCommand $ProviderAddress'."
    }
    $ReleaseRoot = Join-Path (Join-Path $Publication.ExportRoot 'releases') $ReleaseId
    $Release = Read-ProjBuildReleaseDirectory `
        -ReleaseRoot $ReleaseRoot `
        -ReleaseId $ReleaseId `
        -ArtifactNames $ArtifactNames `
        -ControlledRoot $DataRoot
    $CurrentState = Read-ProjCommandProviderState `
        -Path $Publication.StatePath `
        -DataRoot $DataRoot
    $CurrentId = (Read-ProjDevReplaceableUtf8Text -Path $CurrentPath).TrimEnd("`r", "`n")
    if ([string]$CurrentState.Status -cne 'ready' -or
        [string]$CurrentState.InputRevision -cne $Publication.InputRevision -or
        [string]$CurrentState.Token -cne $Publication.Token -or
        [string]$CurrentState.ProducerContract -cne $ProducerContract -or
        $CurrentId -cne $ReleaseId) {
        throw "Required Release Set from '$ProviderAddress' changed while it was read. Run '$EntryCommand $ProviderAddress'."
    }
    return $Release
}

function Read-ProjBuildReleaseDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$ReleaseRoot,
        [Parameter(Mandatory = $true)][string]$ReleaseId,
        [Parameter(Mandatory = $true)][string[]]$ArtifactNames,
        [Parameter(Mandatory = $true)][string]$ControlledRoot
    )

    $ReleaseRoot = Assert-ProjDevPathInsideDataRoot `
        -Path $ReleaseRoot `
        -DataRoot $ControlledRoot `
        -Activity 'reading a build Release Set'
    $ManifestPath = Join-Path $ReleaseRoot 'manifest.json'
    try {
        $Manifest = (Read-ProjDevReplaceableUtf8Text -Path $ManifestPath) |
            ConvertFrom-Json
    } catch {
        throw "The build Release Set manifest is invalid: $ManifestPath"
    }
    $Names = [string[]]@($Manifest.PSObject.Properties.Name)
    if ($Names.Count -ne 4 -or
        $Names -cnotcontains 'schema' -or
        $Names -cnotcontains 'runtimeSchema' -or
        $Names -cnotcontains 'releaseId' -or
        $Names -cnotcontains 'artifacts' -or
        [string]$Manifest.schema -cne $script:ProjBuildReleaseManifestSchema -or
        [string]$Manifest.runtimeSchema -cne $script:ProjRuntimeReleaseManifestSchema -or
        [string]$Manifest.releaseId -cne $ReleaseId) {
        throw "The build Release Set manifest is invalid: $ManifestPath"
    }
    $ExpectedNames = [string[]]@($ArtifactNames | Sort-Object)
    $Records = [Collections.Generic.List[object]]::new()
    foreach ($Artifact in @($Manifest.artifacts)) {
        $Fields = [string[]]@($Artifact.PSObject.Properties.Name)
        if ($Fields.Count -ne 3 -or
            $Fields -cnotcontains 'name' -or
            $Fields -cnotcontains 'length' -or
            $Fields -cnotcontains 'sha256') {
            throw "The build Release Set manifest is invalid: $ManifestPath"
        }
        $Name = [string]$Artifact.name
        $Path = Join-Path $ReleaseRoot $Name
        if ($Name -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._+-]*$' -or
            -not [IO.File]::Exists($Path)) {
            throw "The build Release Set artifact is missing: $Path"
        }
        $Item = Get-Item -LiteralPath $Path
        $Hash = Get-ProjDevFileSha256 -Path $Path
        if (($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
            [long]$Artifact.length -le 0 -or
            [long]$Artifact.length -ne [long]$Item.Length -or
            [string]$Artifact.sha256 -cnotmatch '^[a-f0-9]{64}$' -or
            [string]$Artifact.sha256 -cne $Hash) {
            throw "The build Release Set artifact is corrupt: $Path"
        }
        $Records.Add([pscustomobject][ordered]@{
            Name = $Name
            Path = $Path
            Length = [long]$Item.Length
            Sha256 = $Hash
        })
    }
    $ActualNames = [string[]]@($Records | ForEach-Object Name | Sort-Object)
    if ([string]::Join("`n", $ActualNames) -cne
        [string]::Join("`n", $ExpectedNames)) {
        throw "The build Release Set has the wrong artifact membership: $ManifestPath"
    }
    $Identity = [Collections.Generic.List[string]]::new()
    $Identity.Add($script:ProjRuntimeReleaseManifestSchema)
    foreach ($Record in @($Records | Sort-Object Name)) {
        $Identity.Add([string]$Record.Name)
        $Identity.Add(([long]$Record.Length).ToString(
            [Globalization.CultureInfo]::InvariantCulture
        ))
        $Identity.Add([string]$Record.Sha256)
    }
    $ComputedId = Get-ProjDevSha256Text `
        -Value ([string]::Join("`n", $Identity))
    if ($ComputedId -cne $ReleaseId) {
        throw "The build Release Set ID does not match its artifacts: $ManifestPath"
    }
    return [pscustomobject][ordered]@{
        ReleaseId = $ReleaseId
        Root = $ReleaseRoot
        ManifestPath = $ManifestPath
        Artifacts = [object[]]$Records
    }
}
