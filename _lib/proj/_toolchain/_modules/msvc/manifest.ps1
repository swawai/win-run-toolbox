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
        [Parameter(Mandatory = $true)][object]$ManifestPayload,
        [Parameter(Mandatory = $true)][object]$VisualStudioManifest
    )

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
        ManifestSha256 = ''
        ToolPackageVersion = [string]$Tool[0].Text
        ToolPayloads = [object[]]$ToolPayloads.ToArray()
        SdkPackageId = [string]$SdkPackage.id
        SdkPayloads = [object[]]$SdkPayloads
        MsiPayloads = [object[]]$MsiPayloads.ToArray()
    }
}
