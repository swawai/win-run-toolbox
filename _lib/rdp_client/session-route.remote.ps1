$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ([string]::IsNullOrWhiteSpace($RdpClientSessionRequestBase64)) {
    throw 'RDP session route request was not provided.'
}

$RequestJson = [Text.Encoding]::UTF8.GetString(
    [Convert]::FromBase64String($RdpClientSessionRequestBase64)
)
$Request = $RequestJson | ConvertFrom-Json
$SessionId = [uint32]0
if ($null -eq $Request.PSObject.Properties['SessionId'] -or
    -not [uint32]::TryParse([string]$Request.SessionId, [ref]$SessionId)) {
    throw 'RDP session route request has an invalid session ID.'
}

$Arguments = New-Object 'Collections.Generic.List[string]'
$Executable = $null
$Outcome = 'completed'
switch ([string]$Request.Action) {
    'connect' {
        $Destination = [string]$Request.DestinationSessionName
        if ($Destination -notmatch '^[A-Za-z0-9_.#-]{1,64}$') {
            throw 'RDP session route request has an invalid destination.'
        }
        $Executable = Join-Path $env:SystemRoot 'System32\tscon.exe'
        $Arguments.Add([string]$SessionId)
        $Arguments.Add(('/dest:{0}' -f $Destination))
    }
    'connect-console-if-empty' {
        if ($null -eq (Get-Variable -Name Sessions -ErrorAction SilentlyContinue)) {
            throw 'Guarded console routing requires an RDP session snapshot.'
        }
        $ConsoleUsers = @($Sessions | Where-Object {
            [bool]$_.IsConsole -and
            -not [string]::IsNullOrWhiteSpace([string]$_.UserName)
        })
        if ($ConsoleUsers.Count -gt 0) {
            $Outcome = 'console-occupied'
            break
        }
        $Destination = [string]$Request.DestinationSessionName
        if (-not [string]::Equals(
            $Destination,
            'console',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            throw 'Guarded console routing requires destination console.'
        }
        $Executable = Join-Path $env:SystemRoot 'System32\tscon.exe'
        $Arguments.Add([string]$SessionId)
        $Arguments.Add('/dest:console')
        $Outcome = 'connected'
    }
    'disconnect' {
        $Executable = Join-Path $env:SystemRoot 'System32\tsdiscon.exe'
        $Arguments.Add([string]$SessionId)
    }
    default {
        throw "Unsupported RDP session route action: $($Request.Action)"
    }
}

$NativeExitCode = 0
$NativeOutput = @()
if ($null -ne $Executable) {
    $NativeOutput = @(& $Executable @Arguments 2>&1 | ForEach-Object {
        [string]$_
    })
    $NativeExitCode = [int]$LASTEXITCODE
}
$Result = [ordered]@{
    Version  = 1
    Action   = [string]$Request.Action
    Outcome  = $Outcome
    ExitCode = $NativeExitCode
    Output   = $NativeOutput
}
$Json = ConvertTo-Json -InputObject $Result -Depth 3 -Compress
$Payload = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Json))
Write-Output ('RDP_CLIENT_SESSION_ROUTE_V1:' + $Payload)
