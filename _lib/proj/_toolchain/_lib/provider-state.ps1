Set-StrictMode -Version 2.0

$script:ProjCommandProviderStateSchema =
    'swawkit.command-provider-state/v1'
$script:ProjDevSetupProducerContract =
    'swawkit.proj.dev-setup/v2'
$script:ProjDevCommandEnvironmentInputRevisionVariable =
    'SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION'
$script:ProjDevCommandProfileRevisionVariable =
    'SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION'
$script:ProjDevSetupPublicationTokenVariable =
    'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_PUBLICATION_TOKEN'

function Get-ProjDevSetupProducerContract {
    return $script:ProjDevSetupProducerContract
}

function Get-ProjDevSetupPublicationTokenVariable {
    return $script:ProjDevSetupPublicationTokenVariable
}

function Open-ProjDevReplaceableReadStream {
    param([Parameter(Mandatory = $true)][string]$Path)

    $Share = [IO.FileShare](
        [int][IO.FileShare]::Read -bor
        [int][IO.FileShare]::Delete
    )
    return [IO.File]::Open(
        $Path,
        [IO.FileMode]::Open,
        [IO.FileAccess]::Read,
        $Share
    )
}

function Read-ProjDevReplaceableUtf8Text {
    param([Parameter(Mandatory = $true)][string]$Path)

    $Stream = Open-ProjDevReplaceableReadStream -Path $Path
    $Reader = $null
    try {
        $Reader = [IO.StreamReader]::new(
            $Stream,
            [Text.Encoding]::UTF8,
            $true
        )
        return $Reader.ReadToEnd()
    } finally {
        if ($null -ne $Reader) {
            $Reader.Dispose()
        } else {
            $Stream.Dispose()
        }
    }
}

function Get-ProjDevCommandEnvironmentInputRevision {
    $Revision = Get-ProjDevRequiredEnvironmentValue `
        -Name $script:ProjDevCommandEnvironmentInputRevisionVariable
    if ($Revision -cnotmatch '^sha256-[a-f0-9]{64}$') {
        throw (
            'Invalid command environment input revision in ' +
            "$($script:ProjDevCommandEnvironmentInputRevisionVariable)."
        )
    }
    return $Revision
}

function Get-ProjDevCommandProfileRevision {
    $Revision = Get-ProjDevRequiredEnvironmentValue `
        -Name $script:ProjDevCommandProfileRevisionVariable
    if ($Revision -cnotmatch '^sha256-[a-f0-9]{64}$') {
        throw (
            'Invalid command Profile revision in ' +
            "$($script:ProjDevCommandProfileRevisionVariable)."
        )
    }
    return $Revision
}

function Get-ProjDevCurrentProfileRevision {
    param([Parameter(Mandatory = $true)][object]$Context)

    $ProfilePath = Assert-ProjDevPathInsideDataRoot `
        -Path ([string]$Context.ProfilePath) `
        -DataRoot ([string]$Context.DataRoot) `
        -Activity 'reading the current Entry Profile revision'
    if (-not [IO.File]::Exists($ProfilePath)) {
        throw "The current Entry Profile is missing: $ProfilePath"
    }
    $Stream = Open-ProjDevReplaceableReadStream -Path $ProfilePath
    $Algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        $Hash = [BitConverter]::ToString(
            $Algorithm.ComputeHash($Stream)
        ).Replace('-', '').ToLowerInvariant()
    } finally {
        $Algorithm.Dispose()
        $Stream.Dispose()
    }
    return 'sha256-' + $Hash
}

function Assert-ProjDevCommandProfileCurrent {
    param([Parameter(Mandatory = $true)][object]$Context)

    $Expected = [string]$Context.CommandProfileRevision
    if ($Expected -cnotmatch '^sha256-[a-f0-9]{64}$') {
        throw 'The command Profile revision is missing or invalid.'
    }
    $Current = Get-ProjDevCurrentProfileRevision -Context $Context
    if ($Current -cne $Expected) {
        throw (
            'The Entry Profile changed while this command was active. Run ' +
            'the command again from the current Entry.'
        )
    }
}

function Assert-ProjCommandProviderInputRevision {
    param([Parameter(Mandatory = $true)][string]$InputRevision)

    if ($InputRevision -cnotmatch '^sha256-[a-f0-9]{64}$') {
        throw "Invalid command provider input revision: '$InputRevision'"
    }
    return $InputRevision
}

function Assert-ProjCommandProviderToken {
    param([Parameter(Mandatory = $true)][string]$Token)

    if ($Token -cnotmatch '^[a-f0-9]{32}$') {
        throw "Invalid command provider publication token: '$Token'"
    }
    return $Token
}

function New-ProjCommandProviderUnavailableState {
    param(
        [Parameter(Mandatory = $true)][string]$InputRevision,
        [Parameter(Mandatory = $true)][string]$Token
    )

    return [ordered]@{
        schema = $script:ProjCommandProviderStateSchema
        status = 'unavailable'
        inputRevision = Assert-ProjCommandProviderInputRevision `
            -InputRevision $InputRevision
        token = Assert-ProjCommandProviderToken -Token $Token
    }
}

function New-ProjCommandProviderReadyState {
    param(
        [Parameter(Mandatory = $true)][string]$InputRevision,
        [Parameter(Mandatory = $true)][string]$Token,
        [Parameter(Mandatory = $true)][string]$ProducerContract
    )

    if ($ProducerContract -cnotmatch '^[a-z0-9][a-z0-9._/-]{0,127}$') {
        throw "Invalid command provider contract: '$ProducerContract'"
    }
    return [ordered]@{
        schema = $script:ProjCommandProviderStateSchema
        status = 'ready'
        inputRevision = Assert-ProjCommandProviderInputRevision `
            -InputRevision $InputRevision
        token = Assert-ProjCommandProviderToken -Token $Token
        producerContract = $ProducerContract
    }
}

function Read-ProjCommandProviderState {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$DataRoot
    )

    $StatePath = Assert-ProjDevPathInsideDataRoot `
        -Path $Path `
        -DataRoot $DataRoot `
        -Activity 'reading command provider state'
    if (-not [IO.File]::Exists($StatePath)) {
        return $null
    }
    try {
        $StateText = Read-ProjDevReplaceableUtf8Text -Path $StatePath
        $State = $StateText | ConvertFrom-Json
    } catch {
        throw "Cannot parse command provider state '$StatePath': $($_.Exception.Message)"
    }

    $Status = [string]$State.status
    $ExpectedNames = if ($Status -ceq 'unavailable') {
        [string[]]@('schema', 'status', 'inputRevision', 'token')
    } elseif ($Status -ceq 'ready') {
        [string[]]@(
            'schema',
            'status',
            'inputRevision',
            'token',
            'producerContract'
        )
    } else {
        throw "The command provider state is invalid: $StatePath"
    }
    $ActualNames = [string[]]@($State.PSObject.Properties.Name)
    if ($ActualNames.Count -ne $ExpectedNames.Count) {
        throw "The command provider state is invalid: $StatePath"
    }
    foreach ($Name in $ExpectedNames) {
        if ($ActualNames -cnotcontains $Name) {
            throw "The command provider state is invalid: $StatePath"
        }
        if ($State.$Name -isnot [string]) {
            throw "The command provider state is invalid: $StatePath"
        }
    }
    if ([string]$State.schema -cne $script:ProjCommandProviderStateSchema) {
        throw "The command provider state is invalid: $StatePath"
    }
    try {
        $InputRevision = Assert-ProjCommandProviderInputRevision `
            -InputRevision ([string]$State.inputRevision)
        $Token = Assert-ProjCommandProviderToken `
            -Token ([string]$State.token)
    } catch {
        throw "The command provider state is invalid: $StatePath"
    }

    $Result = [ordered]@{
        Path = $StatePath
        Status = $Status
        InputRevision = $InputRevision
        Token = $Token
    }
    if ($Status -ceq 'ready') {
        $ProducerContract = [string]$State.producerContract
        try {
            if ($ProducerContract -cnotmatch '^[a-z0-9][a-z0-9._/-]{0,127}$') {
                throw 'invalid contract'
            }
        } catch {
            throw "The command provider state is invalid: $StatePath"
        }
        $Result.Add('ProducerContract', $ProducerContract)
    }
    return [pscustomobject]$Result
}

function Write-ProjCommandProviderState {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)]
        [Collections.IDictionary]$State
    )

    Write-ProjDevTextAtomic `
        -Path ([string]$Context.ProviderStatePath) `
        -Content (ConvertTo-ProjDevJsonText -Value $State) `
        -ControlledRoot ([string]$Context.DataRoot) `
        -Encoding ([Text.UTF8Encoding]::new($false))
}

function Start-ProjDevSetupProviderPublication {
    param([Parameter(Mandatory = $true)][object]$Context)

    $ExpectedInputRevision = Assert-ProjCommandProviderInputRevision `
        -InputRevision ([string]$Context.EnvironmentInputRevision)
    $Lock = Enter-ProjDevFileLock `
        -Path ([string]$Context.ProviderStateLockPath) `
        -ControlledRoot ([string]$Context.DataRoot) `
        -TimeoutSeconds 60
    try {
        Assert-ProjDevCommandProfileCurrent -Context $Context
        $Token = [Guid]::NewGuid().ToString('N').ToLowerInvariant()
        $Unavailable = New-ProjCommandProviderUnavailableState `
            -InputRevision $ExpectedInputRevision `
            -Token $Token
        Write-ProjCommandProviderState `
            -Context $Context `
            -State $Unavailable
    } finally {
        $Lock.Dispose()
    }
    return [pscustomobject][ordered]@{
        InputRevision = $ExpectedInputRevision
        Token = $Token
    }
}

function Complete-ProjDevSetupProviderPublication {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Attempt
    )

    $Lock = Enter-ProjDevFileLock `
        -Path ([string]$Context.ProviderStateLockPath) `
        -ControlledRoot ([string]$Context.DataRoot) `
        -TimeoutSeconds 60
    try {
        $Current = Read-ProjCommandProviderState `
            -Path ([string]$Context.ProviderStatePath) `
            -DataRoot ([string]$Context.DataRoot)
        if ($null -eq $Current -or
            [string]$Current.Status -cne 'unavailable' -or
            [string]$Current.InputRevision -cne [string]$Attempt.InputRevision -or
            [string]$Current.Token -cne [string]$Attempt.Token) {
            throw (
                'The project development inputs changed while .dev.setup ' +
                'was running. The stale build was not published; run ' +
                "'$($Context.EntryCommand) $($Context.EnvironmentProviderAddress)' again."
            )
        }
        $Ready = New-ProjCommandProviderReadyState `
            -InputRevision ([string]$Attempt.InputRevision) `
            -Token ([string]$Attempt.Token) `
            -ProducerContract $script:ProjDevSetupProducerContract
        Write-ProjCommandProviderState -Context $Context -State $Ready
    } finally {
        $Lock.Dispose()
    }
}
