[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('status', 'add', 'remove', 'run', 'launch')]
    [string]$Action,

    [Parameter(Mandatory = $true)]
    [AllowEmptyString()]
    [string]$SshEntryFile,

    [Parameter(Mandatory = $true)]
    [string]$RdpEntryFile,

    [Parameter(Mandatory = $true)]
    [ValidateRange(0, 32767)]
    [int]$ArgumentCount,

    [int]$SessionId = 0,

    [string]$CommandName = 'rdp',

    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'entry.ps1')
. (Join-Path $PSScriptRoot 'peer-ssh.ps1')

function Get-RdpClientPsExecArguments {
    param([Parameter(Mandatory = $true)][int]$Count)

    $Result = New-Object 'Collections.Generic.List[string]'
    for ($Index = 1; $Index -le $Count; $Index++) {
        $Name = "RDP_PSEXEC_ARG_$Index"
        $Value = [Environment]::GetEnvironmentVariable($Name, 'Process')
        if ($null -eq $Value) {
            throw "PsExec argument $Index was not forwarded by client.cmd."
        }
        $Result.Add($Value)
    }
    return $Result.ToArray()
}

function Get-RdpClientExpectedPeerAddresses {
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

function Get-RdpClientFileSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not [IO.File]::Exists($Path)) {
        throw "RDP peer managed script was not found: $Path"
    }
    $Bytes = [IO.File]::ReadAllBytes($Path)
    $Hasher = [Security.Cryptography.SHA256]::Create()
    try {
        return [BitConverter]::ToString(
            $Hasher.ComputeHash($Bytes)
        ).Replace('-', '')
    } finally {
        $Hasher.Dispose()
    }
}

function Remove-RdpClientPsExecUploadsBestEffort {
    param(
        [AllowNull()][AllowEmptyString()][string]$SshEntryPath,
        [AllowNull()][object[]]$UploadNames = @()
    )

    $Names = @($UploadNames | Where-Object {
        -not [string]::IsNullOrWhiteSpace([string]$_)
    } | ForEach-Object { [string]$_ })
    if ([string]::IsNullOrWhiteSpace($SshEntryPath) -or $Names.Count -eq 0) {
        return
    }
    foreach ($Name in $Names) {
        if ($Name -notmatch
            '^\.swaw-kit-psexec-(?:helper|desktop)-[A-Fa-f0-9]{32}\.ps1$') {
            [Console]::Error.WriteLine(
                "[WARN] Refused to clean an invalid PsExec upload name: $Name"
            )
            return
        }
    }

    try {
        $Utf8 = New-Object Text.UTF8Encoding($false)
        $PayloadJson = [ordered]@{ Names = $Names } |
            ConvertTo-Json -Compress -Depth 3
        $PayloadBase64 = [Convert]::ToBase64String(
            $Utf8.GetBytes($PayloadJson)
        )
        $CleanupSource = @"
`$ErrorActionPreference = 'Stop'
`$u = New-Object Text.UTF8Encoding(`$false)
`$j = `$u.GetString([Convert]::FromBase64String('$PayloadBase64'))
`$r = `$j | ConvertFrom-Json
foreach (`$n in @(`$r.Names)) {
    if ([string]`$n -notmatch '^\.swaw-kit-psexec-(?:helper|desktop)-[A-Fa-f0-9]{32}\.ps1$') {
        throw 'Invalid PsExec upload cleanup name.'
    }
    `$p = Join-Path `$HOME ([string]`$n)
    if ([IO.File]::Exists(`$p)) {
        Remove-Item -LiteralPath `$p -Force
    }
}
"@
        $Cleanup = Invoke-RdpClientPeerSshPowerShell `
            -SshEntryPath $SshEntryPath `
            -RemoteSource $CleanupSource `
            -TimeoutSeconds 20
        if ($Cleanup.ExitCode -ne 0) {
            throw "cleanup exited with $($Cleanup.ExitCode)"
        }
    } catch {
        [Console]::Error.WriteLine(
            '[WARN] Could not clean temporary peer PsExec uploads ' +
            "($($Names -join ', ')): $($_.Exception.Message)"
        )
    }
}

$ResolvedSshEntry = ''
$HelperUploadName = ''
$DesktopWorkerUploadName = ''

try {
    if (@('run', 'launch') -notcontains $Action -and $ArgumentCount -ne 0) {
        throw 'PsExec management commands do not accept native arguments.'
    }
    if (@('run', 'launch') -contains $Action -and $ArgumentCount -eq 0) {
        throw (
            "PsExec usage: $CommandName .peer psexec <session-id> " +
            '<program-and-arguments>'
        )
    }
    if ($Action -eq 'launch' -and $SessionId -le 0) {
        throw 'PsExec session ID must be a positive integer.'
    }
    if ($Action -ne 'launch' -and $SessionId -ne 0) {
        throw 'PsExec session ID is only valid for session launch.'
    }
    if (@('run', 'launch') -contains $Action -and $DryRun) {
        throw 'PsExec process invocation does not support --dry-run.'
    }

    $Utf8 = New-Object Text.UTF8Encoding($false)
    [Console]::InputEncoding = $Utf8
    [Console]::OutputEncoding = $Utf8
    $OutputEncoding = $Utf8

    $ResolvedSshEntry = Resolve-RdpClientPeerSshEntryPath -Value $SshEntryFile
    $ResolvedRdpEntry = [IO.Path]::GetFullPath($RdpEntryFile)
    Assert-RdpClientPeerSshEntryIsSeparate `
        -SshEntryPath $ResolvedSshEntry `
        -RdpEntryPath $ResolvedRdpEntry

    $Arguments = @()
    if (@('run', 'launch') -contains $Action) {
        $Arguments = @(Get-RdpClientPsExecArguments -Count $ArgumentCount)
    }

    $HelperPath = Join-Path $PSScriptRoot 'helper.ps1'
    $DesktopWorkerPath = Join-Path $PSScriptRoot 'desktop-task.remote.ps1'
    $HelperSha256 = Get-RdpClientFileSha256 -Path $HelperPath
    $DesktopWorkerSha256 = Get-RdpClientFileSha256 -Path $DesktopWorkerPath
    if ($Action -eq 'add' -and -not $DryRun.IsPresent) {
        $HelperUploadName = (
            '.swaw-kit-psexec-helper-' +
            [Guid]::NewGuid().ToString('N') +
            '.ps1'
        )
        $Copy = Invoke-RdpClientPeerSshCopy `
            -SshEntryPath $ResolvedSshEntry `
            -SourcePath $HelperPath `
            -RemoteName $HelperUploadName
        if ($Copy.ExitCode -ne 0) {
            throw (
                'Failed to upload the PsExec session helper. ' +
                ($Copy.Output -join ' ')
            )
        }
        $DesktopWorkerUploadName = (
            '.swaw-kit-psexec-desktop-' +
            [Guid]::NewGuid().ToString('N') +
            '.ps1'
        )
        $Copy = Invoke-RdpClientPeerSshCopy `
            -SshEntryPath $ResolvedSshEntry `
            -SourcePath $DesktopWorkerPath `
            -RemoteName $DesktopWorkerUploadName
        if ($Copy.ExitCode -ne 0) {
            throw (
                'Failed to upload the PsExec desktop worker. ' +
                ($Copy.Output -join ' ')
            )
        }
    }
    $PayloadJson = [ordered]@{
        Action                  = $Action
        DryRun                  = $DryRun.IsPresent
        Arguments               = $Arguments
        SessionId               = $SessionId
        HelperSha256            = $HelperSha256
        HelperUploadName        = $HelperUploadName
        DesktopWorkerSha256     = $DesktopWorkerSha256
        DesktopWorkerUploadName = $DesktopWorkerUploadName
        ExpectedAddresses       = @(Get-RdpClientExpectedPeerAddresses `
            -EntryPath $ResolvedRdpEntry)
    } | ConvertTo-Json -Compress -Depth 4
    $PayloadBase64 = [Convert]::ToBase64String($Utf8.GetBytes($PayloadJson))

    $RemoteScriptPath = Join-Path $PSScriptRoot 'psexec.remote.ps1'
    if (-not [IO.File]::Exists($RemoteScriptPath)) {
        throw "RDP peer PsExec script was not found: $RemoteScriptPath"
    }
    $RemoteSource = [IO.File]::ReadAllText($RemoteScriptPath, [Text.Encoding]::UTF8)
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
    $Marker = '__RDP_CLIENT_PSEXEC_PAYLOAD__'
    if ([regex]::Matches($RemoteSource, [regex]::Escape($Marker)).Count -ne 1) {
        throw 'RDP peer PsExec script has an invalid payload marker.'
    }
    $RemoteSource = $RemoteSource.Replace($Marker, $PayloadBase64)

    $Invocation = Invoke-RdpClientPeerSshPowerShell `
        -SshEntryPath $ResolvedSshEntry `
        -RemoteSource $RemoteSource
    $Invocation.Output | Write-Output
    if ($Action -eq 'add' -and $Invocation.ExitCode -ne 0) {
        Remove-RdpClientPsExecUploadsBestEffort `
            -SshEntryPath $ResolvedSshEntry `
            -UploadNames @($HelperUploadName, $DesktopWorkerUploadName)
    }
    exit $Invocation.ExitCode
} catch {
    if ($Action -eq 'add') {
        Remove-RdpClientPsExecUploadsBestEffort `
            -SshEntryPath $ResolvedSshEntry `
            -UploadNames @($HelperUploadName, $DesktopWorkerUploadName)
    }
    [Console]::Error.WriteLine("[ERROR] $($_.Exception.Message)")
    [Console]::Error.WriteLine(
        "[ERROR] Run `"$CommandName .help`" for peer PsExec usage."
    )
    exit 1
}
