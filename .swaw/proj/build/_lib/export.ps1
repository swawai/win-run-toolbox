Set-StrictMode -Version 2.0

$script:ProjBuildArtifactManifestSchema = 'swawkit.proj-build-artifact/v1'

function Publish-ProjBuildFile {
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

function Publish-ProjBuildArtifact {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePath,
        [Parameter(Mandatory = $true)][string]$ExportPath,
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
    $SourcePath = Assert-ProjDevPathInsideDataRoot `
        -Path $SourcePath `
        -DataRoot $CommandDataRoot `
        -Activity 'reading a project build artifact'
    if (-not [IO.File]::Exists($SourcePath) -or
        (Get-Item -LiteralPath $SourcePath).Length -le 0) {
        throw "The project build candidate is missing or empty: $SourcePath"
    }
    $ArtifactHash = Get-ProjDevFileSha256 -Path $SourcePath
    $InputRevision = 'sha256-' + $ArtifactHash
    $Token = [Guid]::NewGuid().ToString('N').ToLowerInvariant()
    $StateContext = [pscustomobject][ordered]@{
        DataRoot = $CommandDataRoot
        ProviderStatePath = Join-Path $CommandDataRoot '_state.json'
    }
    Write-ProjCommandProviderState `
        -Context $StateContext `
        -State (New-ProjCommandProviderUnavailableState `
            -InputRevision $InputRevision `
            -Token $Token)

    $PublishedPath = Publish-ProjBuildFile `
        -SourcePath $SourcePath `
        -ExportPath $ExportPath `
        -CommandDataRoot $CommandDataRoot
    $Published = Get-Item -LiteralPath $PublishedPath
    $PublishedHash = Get-ProjDevFileSha256 -Path $PublishedPath
    if ($PublishedHash -cne $ArtifactHash) {
        throw "The published project build artifact is corrupt: $PublishedPath"
    }
    $Manifest = [ordered]@{
        schema = $script:ProjBuildArtifactManifestSchema
        producerAddress = $ProducerAddress
        producerContract = $ProducerContract
        inputRevision = $InputRevision
        token = $Token
        artifact = [ordered]@{
            name = [IO.Path]::GetFileName($PublishedPath)
            length = [long]$Published.Length
            sha256 = $PublishedHash
        }
    }
    Write-ProjDevTextAtomic `
        -Path (Join-Path $Published.DirectoryName 'manifest.json') `
        -Content (ConvertTo-ProjDevJsonText -Value $Manifest) `
        -ControlledRoot $CommandDataRoot `
        -Encoding ([Text.UTF8Encoding]::new($false))
    Write-ProjCommandProviderState `
        -Context $StateContext `
        -State (New-ProjCommandProviderReadyState `
            -InputRevision $InputRevision `
            -Token $Token `
            -ProducerContract $ProducerContract)

    Write-Host "[READY] $ProducerAddress ($InputRevision)" `
        -ForegroundColor Green
    Write-Output $PublishedPath
}

function Assert-ProjBuildManifestFields {
    param(
        [Parameter(Mandatory = $true)][object]$Value,
        [Parameter(Mandatory = $true)][string[]]$Names,
        [Parameter(Mandatory = $true)][string]$Path
    )

    if ($Value -isnot [psobject]) {
        throw "The project build artifact manifest is invalid: $Path"
    }
    $ActualNames = [string[]]@($Value.PSObject.Properties.Name)
    if ($ActualNames.Count -ne $Names.Count) {
        throw "The project build artifact manifest is invalid: $Path"
    }
    foreach ($Name in $Names) {
        if ($ActualNames -cnotcontains $Name) {
            throw "The project build artifact manifest is invalid: $Path"
        }
    }
}

function Get-ProjRequiredBuildArtifact {
    param(
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$ProviderAddress,
        [Parameter(Mandatory = $true)][string]$EntryCommand,
        [Parameter(Mandatory = $true)][string]$ProducerContract,
        [Parameter(Mandatory = $true)][string]$ArtifactName
    )

    if ($ArtifactName -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._+-]*$') {
        throw "Invalid project build artifact name: '$ArtifactName'"
    }
    $Publication = Get-ProjReadyCommandExport `
        -DataRoot $DataRoot `
        -ProviderAddress $ProviderAddress `
        -EntryCommand $EntryCommand `
        -ProducerContract $ProducerContract `
        -ProviderSource action
    $ManifestPath = Assert-ProjDevPathInsideDataRoot `
        -Path (Join-Path $Publication.ExportRoot 'manifest.json') `
        -DataRoot $DataRoot `
        -Activity "reading the '$ProviderAddress' artifact manifest"
    try {
        $Manifest = (Read-ProjDevReplaceableUtf8Text -Path $ManifestPath) |
            ConvertFrom-Json
        Assert-ProjBuildManifestFields `
            -Value $Manifest `
            -Names @(
                'schema',
                'producerAddress',
                'producerContract',
                'inputRevision',
                'token',
                'artifact'
            ) `
            -Path $ManifestPath
        Assert-ProjBuildManifestFields `
            -Value $Manifest.artifact `
            -Names @('name', 'length', 'sha256') `
            -Path $ManifestPath
    } catch {
        throw (
            "Required export from '$ProviderAddress' has an invalid " +
            "artifact manifest. Run '$EntryCommand $ProviderAddress'."
        )
    }
    $LengthValue = $Manifest.artifact.length
    $LengthIsInteger = $LengthValue -is [int] -or
        $LengthValue -is [long]
    $ManifestValid =
        [string]$Manifest.schema -ceq $script:ProjBuildArtifactManifestSchema -and
        $Manifest.schema -is [string] -and
        [string]$Manifest.producerAddress -ceq $ProviderAddress -and
        $Manifest.producerAddress -is [string] -and
        [string]$Manifest.producerContract -ceq $ProducerContract -and
        $Manifest.producerContract -is [string] -and
        [string]$Manifest.inputRevision -ceq $Publication.InputRevision -and
        $Manifest.inputRevision -is [string] -and
        [string]$Manifest.token -ceq $Publication.Token -and
        $Manifest.token -is [string] -and
        [string]$Manifest.artifact.name -ceq $ArtifactName -and
        $Manifest.artifact.name -is [string] -and
        $LengthIsInteger -and [long]$LengthValue -gt 0 -and
        $Manifest.artifact.sha256 -is [string] -and
        [string]$Manifest.artifact.sha256 -cmatch '^[a-f0-9]{64}$' -and
        [string]$Manifest.inputRevision -ceq (
            'sha256-' + [string]$Manifest.artifact.sha256
        )
    if (-not $ManifestValid) {
        throw (
            "Required export from '$ProviderAddress' has an invalid " +
            "artifact manifest. Run '$EntryCommand $ProviderAddress'."
        )
    }

    $ArtifactPath = Assert-ProjDevPathInsideDataRoot `
        -Path (Join-Path $Publication.ExportRoot $ArtifactName) `
        -DataRoot $DataRoot `
        -Activity "reading the '$ProviderAddress' build artifact"
    if (-not [IO.File]::Exists($ArtifactPath)) {
        throw (
            "Required artifact from '$ProviderAddress' is missing. " +
            "Run '$EntryCommand $ProviderAddress'."
        )
    }
    $Artifact = Get-Item -LiteralPath $ArtifactPath
    $ArtifactHash = Get-ProjDevFileSha256 -Path $ArtifactPath
    if ([long]$Artifact.Length -ne [long]$LengthValue -or
        $ArtifactHash -cne [string]$Manifest.artifact.sha256) {
        throw (
            "Required artifact from '$ProviderAddress' does not match its " +
            "manifest. Run '$EntryCommand $ProviderAddress'."
        )
    }
    try {
        $Current = Read-ProjCommandProviderState `
            -Path $Publication.StatePath `
            -DataRoot $DataRoot
    } catch {
        $Current = $null
    }
    if ($null -eq $Current -or
        [string]$Current.Status -cne 'ready' -or
        [string]$Current.InputRevision -cne $Publication.InputRevision -or
        [string]$Current.Token -cne $Publication.Token -or
        [string]$Current.ProducerContract -cne $ProducerContract) {
        throw (
            "Required export from '$ProviderAddress' changed while it was " +
            "being read. Run '$EntryCommand $ProviderAddress'."
        )
    }
    return [pscustomobject][ordered]@{
        Path = $ArtifactPath
        Length = [long]$Artifact.Length
        Sha256 = $ArtifactHash
        Publication = $Publication
    }
}
