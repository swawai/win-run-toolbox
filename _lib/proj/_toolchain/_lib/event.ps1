Set-StrictMode -Version 2.0

$script:ProjDevCommandEventProtocol = 'swawkit.command-event-frame/v1'
$script:ProjDevCommandEventPrefix = ([char]0x1e) + 'swawkit-event-v1 '

function Test-ProjDevCommandEventProtocol {
    return [string]$env:SWAWKIT_PROJ_CORE_COMMAND_EVENT_PROTOCOL -ceq `
        $script:ProjDevCommandEventProtocol
}

function Write-ProjDevProgressEvent {
    param(
        [Parameter(Mandatory = $true)][string]$Id,
        [Parameter(Mandatory = $true)]
        [ValidateSet('running', 'completed', 'failed')][string]$State,
        [Parameter(Mandatory = $true)]
        [ValidateSet('bytes', 'items', 'percent')][string]$Unit,
        [Parameter(Mandatory = $true)][string]$Message,
        [Nullable[long]]$Current = $null,
        [Nullable[long]]$Total = $null
    )

    if (-not (Test-ProjDevCommandEventProtocol)) {
        return
    }
    $Event = [ordered]@{
        schema = 'swawkit.command-event/v1'
        kind = 'progress'
        id = $Id
        state = $State
        current = if ($null -ne $Current) { [long]$Current } else { $null }
        total = if ($null -ne $Total) { [long]$Total } else { $null }
        unit = $Unit
        message = $Message
    }
    $Json = ConvertTo-Json -InputObject $Event -Compress
    [Console]::Error.WriteLine($script:ProjDevCommandEventPrefix + $Json)
}
