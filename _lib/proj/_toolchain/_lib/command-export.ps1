Set-StrictMode -Version 2.0

function Get-ProjKernelCommandDataRoot {
    param(
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$Address
    )

    if ($Address -cnotmatch '^\.[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*)*$') {
        throw "Invalid Kernel command provider address: '$Address'"
    }
    $Segments = $Address.Substring(1).Split('.')
    $Path = Join-Path (Join-Path $DataRoot 'modules\kernel') (
        ".$($Segments[0])"
    )
    for ($Index = 1; $Index -lt $Segments.Length; $Index++) {
        $Path = Join-Path $Path $Segments[$Index]
    }
    return Assert-ProjDevPathInsideDataRoot `
        -Path $Path `
        -DataRoot $DataRoot `
        -Activity "resolving command data for '$Address'"
}

function Get-ProjActionCommandDataRoot {
    param(
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$Address
    )

    if ($Address -cnotmatch '^[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*)*$') {
        throw "Invalid Action command provider address: '$Address'"
    }
    $Path = Join-Path $DataRoot 'modules\action'
    foreach ($Segment in $Address.Split('.')) {
        $Path = Join-Path $Path $Segment
    }
    return Assert-ProjDevPathInsideDataRoot `
        -Path $Path `
        -DataRoot $DataRoot `
        -Activity "resolving command data for '$Address'"
}

function Get-ProjCommandProviderDataRoot {
    param(
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$ProviderAddress,
        [ValidateSet('kernel', 'action')]
        [string]$ProviderSource = 'kernel'
    )

    if ($ProviderSource -ceq 'action') {
        return Get-ProjActionCommandDataRoot `
            -DataRoot $DataRoot `
            -Address $ProviderAddress
    }
    return Get-ProjKernelCommandDataRoot `
        -DataRoot $DataRoot `
        -Address $ProviderAddress
}

function Resolve-ProjCommandExportPath {
    param(
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$ProviderAddress,
        [ValidateSet('kernel', 'action')]
        [string]$ProviderSource = 'kernel'
    )

    $CommandRoot = Get-ProjCommandProviderDataRoot `
        -DataRoot $DataRoot `
        -ProviderAddress $ProviderAddress `
        -ProviderSource $ProviderSource
    return Assert-ProjDevPathInsideDataRoot `
        -Path (Join-Path $CommandRoot 'export') `
        -DataRoot $DataRoot `
        -Activity "resolving the '$ProviderAddress' command export"
}

function Get-ProjReadyCommandExport {
    param(
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$ProviderAddress,
        [Parameter(Mandatory = $true)][string]$EntryCommand,
        [Parameter(Mandatory = $true)][string]$ProducerContract,
        [ValidateSet('kernel', 'action')]
        [string]$ProviderSource = 'kernel'
    )

    if ($ProducerContract -cnotmatch '^[a-z0-9][a-z0-9._/-]{0,127}$') {
        throw "Invalid command provider contract: '$ProducerContract'"
    }
    $CommandRoot = Get-ProjCommandProviderDataRoot `
        -DataRoot $DataRoot `
        -ProviderAddress $ProviderAddress `
        -ProviderSource $ProviderSource
    $ExportRoot = Resolve-ProjCommandExportPath `
        -DataRoot $DataRoot `
        -ProviderAddress $ProviderAddress `
        -ProviderSource $ProviderSource
    $StatePath = Assert-ProjDevPathInsideDataRoot `
        -Path (Join-Path $CommandRoot '_state.json') `
        -DataRoot $DataRoot `
        -Activity "resolving the '$ProviderAddress' provider state"
    try {
        $State = Read-ProjCommandProviderState `
            -Path $StatePath `
            -DataRoot $DataRoot
    } catch {
        $State = $null
    }
    if ($null -eq $State -or
        [string]$State.Status -cne 'ready' -or
        [string]$State.ProducerContract -cne $ProducerContract -or
        -not [IO.Directory]::Exists($ExportRoot)) {
        throw (
            "Required export from '$ProviderAddress' is unavailable or " +
            "outdated. Run '$EntryCommand $ProviderAddress'."
        )
    }
    return [pscustomobject][ordered]@{
        ProviderAddress = $ProviderAddress
        ProviderSource = $ProviderSource
        CommandRoot = $CommandRoot
        ExportRoot = $ExportRoot
        StatePath = $StatePath
        InputRevision = [string]$State.InputRevision
        Token = [string]$State.Token
        ProducerContract = [string]$State.ProducerContract
    }
}

function Get-ProjRequiredCommandExport {
    param(
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$ProviderAddress,
        [Parameter(Mandatory = $true)][string]$EntryCommand,
        [Parameter(Mandatory = $true)][string]$InputRevision,
        [Parameter(Mandatory = $true)][string]$ProducerContract,
        [ValidateSet('kernel', 'action')]
        [string]$ProviderSource = 'kernel'
    )

    [void](Assert-ProjCommandProviderInputRevision `
        -InputRevision $InputRevision)
    $Publication = Get-ProjReadyCommandExport `
        -DataRoot $DataRoot `
        -ProviderAddress $ProviderAddress `
        -EntryCommand $EntryCommand `
        -ProducerContract $ProducerContract `
        -ProviderSource $ProviderSource
    if ([string]$Publication.InputRevision -cne $InputRevision) {
        throw (
            "Required export from '$ProviderAddress' is unavailable or " +
            'outdated. Run ' +
            "'$EntryCommand $ProviderAddress'."
        )
    }
    return $Publication
}

function Assert-ProjCommandProviderPublicationCurrent {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Publication
    )

    try {
        $State = Read-ProjCommandProviderState `
            -Path ([string]$Context.ProviderStatePath) `
            -DataRoot ([string]$Context.DataRoot)
    } catch {
        $State = $null
    }
    if ($null -eq $State -or
        [string]$State.Status -cne 'ready' -or
        [string]$State.InputRevision -cne [string]$Publication.InputRevision -or
        [string]$State.Token -cne [string]$Publication.Token -or
        [string]$State.ProducerContract -cne [string]$Publication.ProducerContract) {
        $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
        throw (
            'The development environment publication changed while it was ' +
            "being loaded. Run '$Repair'."
        )
    }
}

function Get-ProjProviderInvocation {
    param(
        [Parameter(Mandatory = $true)][string]$EntryCommand,
        [Parameter(Mandatory = $true)][string]$ProviderAddress
    )

    return "$EntryCommand $ProviderAddress"
}

function Get-ProjEnvironmentRepairInvocation {
    param([Parameter(Mandatory = $true)][object]$Context)

    $Override = $Context.PSObject.Properties['EnvironmentRepairInvocation']
    if ($null -ne $Override -and
        -not [string]::IsNullOrWhiteSpace([string]$Override.Value)) {
        return [string]$Override.Value
    }
    $Provider = $Context.PSObject.Properties['EnvironmentProviderAddress']
    if ($null -eq $Provider -or
        [string]::IsNullOrWhiteSpace([string]$Provider.Value)) {
        throw 'The development environment provider address is missing.'
    }
    return Get-ProjProviderInvocation `
        -EntryCommand ([string]$Context.EntryCommand) `
        -ProviderAddress ([string]$Provider.Value)
}
