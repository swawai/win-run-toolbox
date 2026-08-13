[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('screenshot', 'pixel', 'click', 'script')]
    [string]$Action,

    [Parameter(Mandatory = $true)][string]$EntryFile,

    [Parameter(Mandatory = $true)]
    [AllowEmptyString()][string]$SshEntryFile,

    [Parameter(Mandatory = $true)][string]$SessionId,

    [AllowNull()][AllowEmptyString()][string]$X,

    [AllowNull()][AllowEmptyString()][string]$Y,

    [switch]$Display,

    [AllowNull()][AllowEmptyString()][string]$Timeout = '60s',

    [AllowNull()][AllowEmptyString()][string]$OutputPath = '',

    [AllowNull()][AllowEmptyString()][string]$ScriptPath = '',

    [string]$CommandName = 'rdp'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'entry.ps1')
. (Join-Path $PSScriptRoot 'process-job.ps1')
. (Join-Path $PSScriptRoot 'peer-ssh.ps1')
. (Join-Path $PSScriptRoot 'session.ps1')
. (Join-Path $PSScriptRoot 'session-connect.ps1')
. (Join-Path $PSScriptRoot 'session-display.ps1')
. (Join-Path $PSScriptRoot 'desktop-script.ps1')

function Resolve-RdpClientDesktopTimeoutSeconds {
    param([AllowNull()][AllowEmptyString()][string]$Value)

    $Text = if ([string]::IsNullOrWhiteSpace($Value)) {
        '60s'
    } else {
        $Value.Trim()
    }
    if ($Text -notmatch '^(?<Seconds>[0-9]+)(?:s)?$') {
        throw 'Desktop task timeout must use seconds, for example 60s.'
    }
    $Seconds = [int]0
    if (-not [int]::TryParse($Matches.Seconds, [ref]$Seconds) -or
        $Seconds -lt 1 -or $Seconds -gt 600) {
        throw 'Desktop task timeout must be between 1s and 600s.'
    }
    return $Seconds
}

function Resolve-RdpClientDesktopCoordinate {
    param(
        [AllowNull()][AllowEmptyString()][string]$Value,
        [Parameter(Mandatory = $true)][string]$Name
    )

    $Result = [int]0
    if ([string]::IsNullOrWhiteSpace($Value) -or
        -not [int]::TryParse($Value, [ref]$Result) -or
        $Result -lt 0) {
        throw "$Name must be a non-negative decimal integer."
    }
    return $Result
}

function Get-RdpClientDesktopExpectedPeerAddresses {
    param([Parameter(Mandatory = $true)][string]$EntryPath)

    $Document = Read-RdpClientEntryDocument -Path $EntryPath
    $HostName = [string]$Document.FullAddress.Host
    $ParsedHost = $null
    if ([Net.IPAddress]::TryParse($HostName, [ref]$ParsedHost)) {
        $Addresses = @($ParsedHost)
    } else {
        try {
            $Addresses = @([Net.Dns]::GetHostAddresses($HostName))
        } catch {
            throw "RDP peer name does not resolve: $HostName"
        }
    }
    if ($Addresses.Count -eq 0) {
        throw "RDP peer name does not resolve: $HostName"
    }
    return @($Addresses | ForEach-Object {
        if ($_.IsIPv4MappedToIPv6) {
            $_.MapToIPv4().ToString()
        } else {
            $_.ToString()
        }
    } | Sort-Object -Unique)
}

function Get-RdpClientDesktopFileSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not [IO.File]::Exists($Path)) {
        throw "Required RDP client file was not found: $Path"
    }
    $Hasher = [Security.Cryptography.SHA256]::Create()
    try {
        return [BitConverter]::ToString(
            $Hasher.ComputeHash([IO.File]::ReadAllBytes($Path))
        ).Replace('-', '')
    } finally {
        $Hasher.Dispose()
    }
}

function Invoke-RdpClientDesktopTask {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][string]$RdpEntryPath,
        [Parameter(Mandatory = $true)][uint32]$TargetSessionId,
        [Parameter(Mandatory = $true)][string]$TaskAction,
        [Parameter(Mandatory = $true)][string]$ExpectedUserName,
        [AllowEmptyString()][string]$ExpectedDomainName,
        [AllowNull()][Nullable[int]]$CoordinateX,
        [AllowNull()][Nullable[int]]$CoordinateY,
        [AllowNull()][object[]]$WorkflowSteps,
        [Parameter(Mandatory = $true)][pscustomobject]$TimeoutBudget
    )

    $TransportTimeoutSeconds = Get-RdpClientTimeoutBudgetRemainingSeconds `
        -Budget $TimeoutBudget `
        -Operation 'Desktop command'
    # Keep the SSH control path alive after the worker deadline so the peer
    # can verify process identity, stop the exact worker, and return evidence.
    $CleanupReserveSeconds = 5
    if ($TransportTimeoutSeconds -le $CleanupReserveSeconds) {
        throw (
            'Desktop command has insufficient time remaining to start a ' +
            'managed worker and verify its cleanup.'
        )
    }
    $WorkerTimeoutSeconds = $TransportTimeoutSeconds - $CleanupReserveSeconds
    $Utf8 = New-Object Text.UTF8Encoding($false)
    $TaskRequest = [ordered]@{
        Action             = $TaskAction
        SessionId          = [uint64]$TargetSessionId
        ExpectedUserName   = $ExpectedUserName
        ExpectedDomainName = $ExpectedDomainName
    }
    if ($null -ne $CoordinateX -and $null -ne $CoordinateY) {
        $TaskRequest.X = [int]$CoordinateX
        $TaskRequest.Y = [int]$CoordinateY
    }
    if ($TaskAction -eq 'script') {
        $TaskRequest.Steps = @($WorkflowSteps | ForEach-Object {
            $RemoteStep = [ordered]@{ Action = [string]$_.Action }
            if ($_.Action -in @('pixel', 'click')) {
                $RemoteStep.X = [int]$_.X
                $RemoteStep.Y = [int]$_.Y
            } elseif ($_.Action -eq 'wait') {
                $RemoteStep.Milliseconds = [int]$_.Milliseconds
            }
            [pscustomobject]$RemoteStep
        })
    }
    $TaskRequestJson = ConvertTo-Json `
        -InputObject $TaskRequest `
        -Compress `
        -Depth 4
    $TaskRequestBase64 = [Convert]::ToBase64String(
        $Utf8.GetBytes($TaskRequestJson)
    )

    $TaskScriptPath = Join-Path $PSScriptRoot 'desktop-task.remote.ps1'
    if (-not [IO.File]::Exists($TaskScriptPath)) {
        throw "RDP desktop task script was not found: $TaskScriptPath"
    }

    $HelperPath = Join-Path $PSScriptRoot 'helper.ps1'
    $OuterRequest = [ordered]@{
        Action                  = 'desktop'
        DryRun                  = $false
        Arguments               = @()
        SessionId               = [uint64]$TargetSessionId
        HelperSha256            = Get-RdpClientDesktopFileSha256 -Path $HelperPath
        HelperUploadName        = ''
        DesktopWorkerSha256     = Get-RdpClientDesktopFileSha256 -Path $TaskScriptPath
        DesktopWorkerUploadName = ''
        DesktopTimeoutSeconds   = $WorkerTimeoutSeconds
        ExpectedAddresses       = @(
            Get-RdpClientDesktopExpectedPeerAddresses -EntryPath $RdpEntryPath
        )
        DesktopRequestBase64    = $TaskRequestBase64
    }
    $OuterJson = ConvertTo-Json -InputObject $OuterRequest -Compress -Depth 4
    $OuterBase64 = [Convert]::ToBase64String($Utf8.GetBytes($OuterJson))

    $PsExecScriptPath = Join-Path $PSScriptRoot 'psexec.remote.ps1'
    if (-not [IO.File]::Exists($PsExecScriptPath)) {
        throw "RDP peer PsExec script was not found: $PsExecScriptPath"
    }
    $RemoteSource = [IO.File]::ReadAllText(
        $PsExecScriptPath,
        [Text.Encoding]::UTF8
    )
    $LibraryPath = Join-Path $PSScriptRoot 'psexec-lib.remote.ps1'
    if (-not [IO.File]::Exists($LibraryPath)) {
        throw "RDP peer PsExec library was not found: $LibraryPath"
    }
    $LibraryMarker = '__RDP_CLIENT_PSEXEC_LIBRARY__'
    if ([regex]::Matches(
        $RemoteSource,
        [regex]::Escape($LibraryMarker)
    ).Count -ne 1) {
        throw 'RDP peer PsExec script has an invalid library marker.'
    }
    $RemoteSource = $RemoteSource.Replace(
        $LibraryMarker,
        [IO.File]::ReadAllText($LibraryPath, [Text.Encoding]::UTF8)
    )
    $PayloadMarker = '__RDP_CLIENT_PSEXEC_PAYLOAD__'
    if ([regex]::Matches(
        $RemoteSource,
        [regex]::Escape($PayloadMarker)
    ).Count -ne 1) {
        throw 'RDP peer PsExec script has an invalid payload marker.'
    }
    $RemoteSource = $RemoteSource.Replace($PayloadMarker, $OuterBase64)
    $Invocation = Invoke-RdpClientPeerSshPowerShell `
        -SshEntryPath $SshEntryPath `
        -RemoteSource $RemoteSource `
        -TimeoutSeconds $TransportTimeoutSeconds

    $ResultPattern = '^RDP_CLIENT_DESKTOP_RESULT_V1:(?<Payload>[A-Za-z0-9+/=]+)$'
    $Markers = @($Invocation.Output | Where-Object { $_ -match $ResultPattern })
    if ($Markers.Count -ne 1 -or $Markers[0] -notmatch $ResultPattern) {
        $Detail = @($Invocation.Output) -join [Environment]::NewLine
        throw (
            'The peer did not return exactly one desktop task result. ' +
            "exit=$($Invocation.ExitCode)" +
            $(if ($Detail.Length -eq 0) { '' } else {
                [Environment]::NewLine + $Detail
            })
        )
    }
    try {
        $ResultJson = $Utf8.GetString(
            [Convert]::FromBase64String($Matches.Payload)
        )
        $Result = $ResultJson | ConvertFrom-Json
    } catch {
        throw "The peer returned invalid desktop task JSON: $($_.Exception.Message)"
    }
    if ($null -eq $Result -or $Result -is [Array] -or
        $null -eq $Result.PSObject.Properties['Version'] -or
        [int]$Result.Version -ne 1 -or
        $null -eq $Result.PSObject.Properties['Success']) {
        throw 'The peer returned an unsupported desktop task result.'
    }
    if (-not [bool]$Result.Success) {
        throw "$($Result.ErrorCode): $($Result.Error)"
    }
    if ($Invocation.ExitCode -ne 0) {
        throw "The desktop task succeeded but PsExec exited with $($Invocation.ExitCode)."
    }
    return $Result
}

function Resolve-RdpClientScreenshotOutputPath {
    param(
        [AllowNull()][AllowEmptyString()][string]$ConfiguredPath,
        [Parameter(Mandatory = $true)][string]$EntryCommand,
        [Parameter(Mandatory = $true)][uint32]$TargetSessionId
    )

    if ([string]::IsNullOrWhiteSpace($ConfiguredPath)) {
        $Root = Join-Path ([IO.Path]::GetTempPath()) 'swaw-kit\rdp-client\captures'
        $SafeCommand = $EntryCommand -replace '[^A-Za-z0-9._-]', '_'
        $Name = '{0}-session-{1}-{2}-{3}.png' -f `
            $SafeCommand,
            $TargetSessionId,
            (Get-Date -Format 'yyyyMMdd-HHmmssfff'),
            ([Guid]::NewGuid().ToString('N').Substring(0, 8))
        return Join-Path $Root $Name
    }

    $Expanded = [Environment]::ExpandEnvironmentVariables($ConfiguredPath.Trim())
    if (-not [IO.Path]::IsPathRooted($Expanded)) {
        throw 'Screenshot output path must be absolute.'
    }
    if (-not [string]::Equals(
        [IO.Path]::GetExtension($Expanded),
        '.png',
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw 'Screenshot output path must name a .png file.'
    }
    return [IO.Path]::GetFullPath($Expanded)
}

function ConvertFrom-RdpClientDesktopPngData {
    param([AllowNull()][AllowEmptyString()][string]$ImageBase64)

    try {
        [byte[]]$ImageBytes = [Convert]::FromBase64String($ImageBase64)
    } catch {
        throw 'The peer returned invalid screenshot image data.'
    }
    if ($ImageBytes.Length -lt 8 -or $ImageBytes.Length -gt 52428800 -or
        $ImageBytes[0] -ne 0x89 -or $ImageBytes[1] -ne 0x50 -or
        $ImageBytes[2] -ne 0x4E -or $ImageBytes[3] -ne 0x47) {
        throw 'The peer returned an invalid or oversized PNG screenshot.'
    }
    return (, $ImageBytes)
}

function Write-RdpClientDesktopPngFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][byte[]]$ImageBytes
    )

    $CreatedOutput = $false
    try {
        $OutputStream = [IO.File]::Open(
            $Path,
            [IO.FileMode]::CreateNew,
            [IO.FileAccess]::Write,
            [IO.FileShare]::None
        )
        $CreatedOutput = $true
        try {
            $OutputStream.Write($ImageBytes, 0, $ImageBytes.Length)
        } finally {
            $OutputStream.Dispose()
        }
    } catch {
        if ($CreatedOutput -and [IO.File]::Exists($Path)) {
            try { [IO.File]::Delete($Path) } catch { }
        }
        throw
    }
}

function Write-RdpClientDesktopOutputMarker {
    param([Parameter(Mandatory = $true)][Collections.IDictionary]$Result)

    $Utf8 = New-Object Text.UTF8Encoding($false)
    $Json = ConvertTo-Json -InputObject $Result -Compress -Depth 4
    $Payload = [Convert]::ToBase64String($Utf8.GetBytes($Json))
    Write-Output ('RDP_CLIENT_DESKTOP_OUTPUT_V1:' + $Payload)
}

$Lease = $null
try {
    $Utf8NoBom = New-Object Text.UTF8Encoding($false)
    [Console]::OutputEncoding = $Utf8NoBom
    $OutputEncoding = $Utf8NoBom

    $ResolvedSessionId = Resolve-RdpClientSessionId -Value $SessionId
    if ($null -eq $ResolvedSessionId -or $ResolvedSessionId -eq 0) {
        throw 'A positive desktop session ID is required.'
    }
    if ([uint64]$ResolvedSessionId -gt [int]::MaxValue) {
        throw 'Desktop session ID exceeds the range supported by PsExec.'
    }
    $TimeoutSeconds = Resolve-RdpClientDesktopTimeoutSeconds -Value $Timeout
    $TimeoutBudget = New-RdpClientTimeoutBudget -TimeoutSeconds $TimeoutSeconds
    $CoordinateX = $null
    $CoordinateY = $null
    $Workflow = $null
    if ($Action -in @('pixel', 'click')) {
        $CoordinateX = [Nullable[int]](
            Resolve-RdpClientDesktopCoordinate -Value $X -Name 'X'
        )
        $CoordinateY = [Nullable[int]](
            Resolve-RdpClientDesktopCoordinate -Value $Y -Name 'Y'
        )
    } elseif (-not [string]::IsNullOrWhiteSpace($X) -or
        -not [string]::IsNullOrWhiteSpace($Y)) {
        throw "$Action does not accept coordinates."
    }
    if ($Action -ne 'screenshot' -and
        -not [string]::IsNullOrWhiteSpace($OutputPath)) {
        throw '--output is only valid with screenshot.'
    }
    if ($Action -eq 'script') {
        if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
            throw 'Desktop script requires a workflow .ps1 path.'
        }
        $Workflow = Read-RdpClientDesktopScript -Path $ScriptPath
        foreach ($Step in $Workflow.Steps) {
            if ($Step.Action -eq 'screenshot') {
                [IO.Directory]::CreateDirectory(
                    [IO.Path]::GetDirectoryName([string]$Step.OutputPath)
                ) | Out-Null
            }
        }
    } elseif (-not [string]::IsNullOrWhiteSpace($ScriptPath)) {
        throw 'A workflow script path is only valid with the script action.'
    }
    $ResolvedScreenshotOutput = $null
    if ($Action -eq 'screenshot') {
        $ResolvedScreenshotOutput = Resolve-RdpClientScreenshotOutputPath `
            -ConfiguredPath $OutputPath `
            -EntryCommand $CommandName `
            -TargetSessionId $ResolvedSessionId
        if ([IO.File]::Exists($ResolvedScreenshotOutput)) {
            throw "Screenshot output already exists: $ResolvedScreenshotOutput"
        }
        [IO.Directory]::CreateDirectory(
            [IO.Path]::GetDirectoryName($ResolvedScreenshotOutput)
        ) | Out-Null
    }

    $ResolvedEntry = [IO.Path]::GetFullPath($EntryFile)
    $Document = Read-RdpClientEntryDocument -Path $ResolvedEntry
    $ResolvedSshEntry = Resolve-RdpClientPeerSshEntryPath -Value $SshEntryFile
    Assert-RdpClientPeerSshEntryIsSeparate `
        -SshEntryPath $ResolvedSshEntry `
        -RdpEntryPath $ResolvedEntry
    $InitialState = Get-RdpClientPeerSessionState `
        -SshEntryPath $ResolvedSshEntry `
        -TimeoutSeconds (Get-RdpClientTimeoutBudgetRemainingSeconds `
            -Budget $TimeoutBudget `
            -Operation 'Desktop command')
    $SelectedSession = Resolve-RdpClientSessionSelection `
        -State $InitialState `
        -EntryUserName $Document.Username `
        -SessionId $ResolvedSessionId

    $DisplaySource = 'existing'
    if (-not (Test-RdpClientSessionDisplayReady -Session $SelectedSession)) {
        $LockedProperty = $SelectedSession.PSObject.Properties['Locked']
        if ([string]::Equals(
            [string]$SelectedSession.State,
            'Active',
            [StringComparison]::OrdinalIgnoreCase
        ) -and $null -ne $LockedProperty -and
            $null -ne $LockedProperty.Value -and
            [bool]$LockedProperty.Value) {
            throw (
                "DESKTOP_NOT_INTERACTIVE: Session $ResolvedSessionId is " +
                'active but locked. Desktop actions do not automatically ' +
                'unlock or reroute an existing active session.'
            )
        }
        if (-not $Display) {
            throw (
                "DISPLAY_NOT_READY: Session $ResolvedSessionId is " +
                "$($SelectedSession.State) or locked. Run `"$CommandName " +
                ".$ResolvedSessionId $Action --display`"."
            )
        }
        $Lease = Open-RdpClientSessionDisplayLease `
            -SshEntryPath $ResolvedSshEntry `
            -EntryFile $ResolvedEntry `
            -EntryUserName $Document.Username `
            -CommandName $CommandName `
            -BeforeState $InitialState `
            -SessionId $ResolvedSessionId `
            -TimeoutBudget $TimeoutBudget
        $SelectedSession = $Lease.Session
        $DisplaySource = 'temporary-rdp'
    }

    $UserName = Get-RdpClientSessionDisplayUserName -Session $SelectedSession
    Write-Host (
        '[RDP] Session:   {0} ({1}; {2}; {3})' -f `
            $SelectedSession.Id,
            $UserName,
            $SelectedSession.State,
            $SelectedSession.Terminal
    )
    Write-Host "[RDP] Display:   $DisplaySource"

    $TaskWorkflowSteps = if ($null -eq $Workflow) {
        @()
    } else {
        @($Workflow.Steps)
    }
    $TaskResult = Invoke-RdpClientDesktopTask `
        -SshEntryPath $ResolvedSshEntry `
        -RdpEntryPath $ResolvedEntry `
        -TargetSessionId $ResolvedSessionId `
        -TaskAction $Action `
        -ExpectedUserName ([string]$SelectedSession.UserName) `
        -ExpectedDomainName ([string]$SelectedSession.DomainName) `
        -CoordinateX $CoordinateX `
        -CoordinateY $CoordinateY `
        -WorkflowSteps $TaskWorkflowSteps `
        -TimeoutBudget $TimeoutBudget

    $PublicResult = [ordered]@{
        Version       = 1
        Action        = $Action
        SessionId     = [uint64]$ResolvedSessionId
        User          = $UserName
        DisplaySource = $DisplaySource
        OriginX       = [int]$TaskResult.OriginX
        OriginY       = [int]$TaskResult.OriginY
        Width         = [int]$TaskResult.Width
        Height        = [int]$TaskResult.Height
    }

    if ($Action -eq 'screenshot') {
        $ImageBytes = ConvertFrom-RdpClientDesktopPngData `
            -ImageBase64 ([string]$TaskResult.ImageBase64)
        Write-RdpClientDesktopPngFile `
            -Path $ResolvedScreenshotOutput `
            -ImageBytes $ImageBytes
        $PublicResult.OutputPath = $ResolvedScreenshotOutput
        Write-Host "[RDP] Screenshot: $ResolvedScreenshotOutput"
        Write-Host "[RDP] Size:       $($TaskResult.Width) x $($TaskResult.Height)"
    } elseif ($Action -eq 'pixel') {
        $PublicResult.X = [int]$TaskResult.X
        $PublicResult.Y = [int]$TaskResult.Y
        $PublicResult.Color = [string]$TaskResult.Color
        Write-Host (
            '[RDP] Pixel:     ({0}, {1}) {2}' -f `
                $TaskResult.X,
                $TaskResult.Y,
                $TaskResult.Color
        )
    } elseif ($Action -eq 'click') {
        $PublicResult.X = [int]$TaskResult.X
        $PublicResult.Y = [int]$TaskResult.Y
        Write-Host "[RDP] Clicked:   ($($TaskResult.X), $($TaskResult.Y))"
    } else {
        $ReturnedSteps = @($TaskResult.Steps)
        if ($ReturnedSteps.Count -ne $Workflow.Steps.Count) {
            throw (
                'The peer returned an unexpected workflow step count: ' +
                "$($ReturnedSteps.Count), expected $($Workflow.Steps.Count)."
            )
        }
        $PublicSteps = New-Object 'Collections.Generic.List[object]'
        $CreatedWorkflowOutputs = New-Object 'Collections.Generic.List[string]'
        try {
            for ($Index = 0; $Index -lt $Workflow.Steps.Count; $Index++) {
                $ExpectedStep = $Workflow.Steps[$Index]
                $ReturnedStep = $ReturnedSteps[$Index]
                if ([int]$ReturnedStep.Index -ne ($Index + 1) -or
                    -not [string]::Equals(
                        [string]$ReturnedStep.Action,
                        [string]$ExpectedStep.Action,
                        [StringComparison]::Ordinal
                    )) {
                    throw "The peer returned mismatched workflow step $($Index + 1)."
                }
                $PublicStep = [ordered]@{
                    Index      = $Index + 1
                    Action     = [string]$ExpectedStep.Action
                    LineNumber = [int]$ExpectedStep.LineNumber
                }
                switch ([string]$ExpectedStep.Action) {
                    'screenshot' {
                        $ImageBytes = ConvertFrom-RdpClientDesktopPngData `
                            -ImageBase64 ([string]$ReturnedStep.ImageBase64)
                        Write-RdpClientDesktopPngFile `
                            -Path ([string]$ExpectedStep.OutputPath) `
                            -ImageBytes $ImageBytes
                        $CreatedWorkflowOutputs.Add(
                            [string]$ExpectedStep.OutputPath
                        )
                        $PublicStep.OutputPath = [string]$ExpectedStep.OutputPath
                        Write-Host (
                            '[RDP] Step {0}: screenshot {1}' -f `
                                ($Index + 1),
                                $ExpectedStep.OutputPath
                        )
                    }
                    'pixel' {
                        if ([int]$ReturnedStep.X -ne [int]$ExpectedStep.X -or
                            [int]$ReturnedStep.Y -ne [int]$ExpectedStep.Y -or
                            [string]$ReturnedStep.Color -notmatch
                                '^#[0-9A-Fa-f]{6}$') {
                            throw "The peer returned invalid pixel step $($Index + 1)."
                        }
                        $PublicStep.X = [int]$ExpectedStep.X
                        $PublicStep.Y = [int]$ExpectedStep.Y
                        $PublicStep.Color = [string]$ReturnedStep.Color
                        Write-Host (
                            '[RDP] Step {0}: pixel ({1}, {2}) {3}' -f `
                                ($Index + 1),
                                $ExpectedStep.X,
                                $ExpectedStep.Y,
                                $ReturnedStep.Color
                        )
                    }
                    'click' {
                        if ([int]$ReturnedStep.X -ne [int]$ExpectedStep.X -or
                            [int]$ReturnedStep.Y -ne [int]$ExpectedStep.Y) {
                            throw "The peer returned invalid click step $($Index + 1)."
                        }
                        $PublicStep.X = [int]$ExpectedStep.X
                        $PublicStep.Y = [int]$ExpectedStep.Y
                        Write-Host (
                            '[RDP] Step {0}: click ({1}, {2})' -f `
                                ($Index + 1),
                                $ExpectedStep.X,
                                $ExpectedStep.Y
                        )
                    }
                    'wait' {
                        if ([int]$ReturnedStep.Milliseconds -ne
                            [int]$ExpectedStep.Milliseconds) {
                            throw "The peer returned invalid wait step $($Index + 1)."
                        }
                        $PublicStep.Milliseconds = [int]$ExpectedStep.Milliseconds
                        Write-Host (
                            '[RDP] Step {0}: wait {1}ms' -f `
                                ($Index + 1),
                                $ExpectedStep.Milliseconds
                        )
                    }
                    default {
                        throw "Unsupported local workflow step: $($ExpectedStep.Action)"
                    }
                }
                $PublicSteps.Add([pscustomobject]$PublicStep)
            }
        } catch {
            foreach ($CreatedPath in $CreatedWorkflowOutputs) {
                if ([IO.File]::Exists($CreatedPath)) {
                    try { [IO.File]::Delete($CreatedPath) } catch { }
                }
            }
            throw
        }
        $PublicResult.ScriptPath = [string]$Workflow.Path
        $PublicResult.Steps = $PublicSteps.ToArray()
    }

    Write-RdpClientDesktopOutputMarker -Result $PublicResult
    exit 0
} catch {
    [Console]::Error.WriteLine("[ERROR] $($_.Exception.Message)")
    [Console]::Error.WriteLine(
        "[ERROR] Run `"$CommandName .help`" for desktop session usage."
    )
    exit 1
} finally {
    Close-RdpClientSessionDisplayLease -Lease $Lease
}
