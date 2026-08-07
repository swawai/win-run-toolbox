[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$EntryFile,

    [Parameter(Mandatory = $true)]
    [AllowEmptyString()]
    [string]$SshEntryFile,

    [string]$CommandName = 'rdp'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'entry.ps1')
. (Join-Path $PSScriptRoot 'peer-ssh.ps1')

$script:DoctorFailures = 0
$script:DoctorWarnings = 0

function Write-RdpClientDoctorCheck {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet('PASS', 'FAIL', 'WARN', 'INFO')]
        [string]$State,

        [Parameter(Mandatory = $true)][string]$Name,
        [AllowEmptyString()][string]$Detail = ''
    )

    if ($State -eq 'FAIL') {
        $script:DoctorFailures++
    } elseif ($State -eq 'WARN') {
        $script:DoctorWarnings++
    }

    $Suffix = if ($Detail.Length -gt 0) { ": $Detail" } else { '' }
    Write-Host ('  {0,-4}  {1}{2}' -f $State, $Name, $Suffix)
}

function Test-RdpClientTcpPort {
    param(
        [Parameter(Mandatory = $true)][string]$HostName,
        [Parameter(Mandatory = $true)][int]$Port,
        [int]$TimeoutMilliseconds = 1500
    )

    $Client = New-Object Net.Sockets.TcpClient
    try {
        $ConnectTask = $Client.ConnectAsync($HostName, $Port)
        $Connected = $ConnectTask.Wait($TimeoutMilliseconds) -and $Client.Connected
        return [pscustomobject]@{
            Connected = $Connected
            Error     = if ($Connected) { '' } else { 'timed out or was rejected' }
        }
    } catch {
        return [pscustomobject]@{
            Connected = $false
            Error     = $_.Exception.GetBaseException().Message
        }
    } finally {
        $Client.Dispose()
    }
}

function Write-RdpClientPortCheck {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Probe,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][int]$Port
    )

    if ($Probe.Connected) {
        Write-RdpClientDoctorCheck -State PASS -Name $Name -Detail "TCP $Port reachable"
    } else {
        Write-RdpClientDoctorCheck -State FAIL -Name $Name -Detail (
            "TCP $Port unreachable ($($Probe.Error))"
        )
    }
}

function ConvertFrom-RdpClientRemoteDoctorOutput {
    param([Parameter(Mandatory = $true)][object[]]$Output)

    $Prefix = 'RDP_SHADOW_DOCTOR_V1:'
    $Marker = @($Output | ForEach-Object { [string]$_ } | Where-Object {
        $_.StartsWith($Prefix, [StringComparison]::Ordinal)
    } | Select-Object -Last 1)
    if ($Marker.Count -ne 1) {
        throw 'SSH diagnostics returned no RDP Shadow doctor payload.'
    }

    $JsonBytes = [Convert]::FromBase64String($Marker[0].Substring($Prefix.Length))
    $Json = (New-Object Text.UTF8Encoding($false)).GetString($JsonBytes)
    return $Json | ConvertFrom-Json
}

function Write-RdpClientFirewallFamilyCheck {
    param(
        [Parameter(Mandatory = $true)][object[]]$Rules,
        [Parameter(Mandatory = $true)][string[]]$Names,
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][bool]$PortReachable
    )

    $Family = @($Rules | Where-Object { $Names -contains $_.Name })
    $Enabled = @($Family | Where-Object {
        $_.Enabled -eq 'True' -and
        [bool]$_.ActiveProfile -and
        $_.Direction -eq 'Inbound' -and
        $_.Action -eq 'Allow'
    })
    if ($Enabled.Count -gt 0) {
        $Details = @($Enabled | ForEach-Object {
            "$($_.Name) [profile=$($_.Profile); remote=$($_.RemoteAddress)]"
        }) -join ', '
        Write-RdpClientDoctorCheck -State PASS -Name $Label -Detail $Details
        return
    }

    $KnownNames = if ($Family.Count -gt 0) {
        @($Family | ForEach-Object { "$($_.Name)=$($_.Enabled)" }) -join ', '
    } else {
        'built-in rules not found'
    }
    $State = if ($PortReachable) { 'INFO' } else { 'FAIL' }
    $Prefix = if ($PortReachable) { 'port is reachable through another rule; ' } else { '' }
    Write-RdpClientDoctorCheck -State $State -Name $Label -Detail ($Prefix + $KnownNames)
}

try {
    $Utf8NoBom = New-Object Text.UTF8Encoding($false)
    [Console]::InputEncoding = $Utf8NoBom
    [Console]::OutputEncoding = $Utf8NoBom
    $OutputEncoding = $Utf8NoBom

    $ResolvedEntry = [IO.Path]::GetFullPath($EntryFile)
    $HostAlias = Resolve-RdpClientHostAlias -Value $env:RDP_HOST_ALIAS
    $Document = Read-RdpClientEntryDocument -Path $ResolvedEntry
    $Target = Resolve-RdpClientConnectionTarget `
        -Document $Document `
        -HostAlias $HostAlias
    Assert-RdpClientHostAliasResolves `
        -HostAlias $HostAlias `
        -CommandName $CommandName

    $TargetHost = if ($HostAlias.Length -gt 0) {
        $HostAlias
    } else {
        $Document.FullAddress.Host
    }
    $RdpPort = if ($null -ne $Document.FullAddress.Port) {
        [int]$Document.FullAddress.Port
    } else {
        3389
    }

    Write-Host '[RDP] Shadow doctor'
    Write-Host "  Target: $Target"

    $RdpProbe = Test-RdpClientTcpPort -HostName $TargetHost -Port $RdpPort
    $RpcProbe = Test-RdpClientTcpPort -HostName $TargetHost -Port 135
    $SmbProbe = Test-RdpClientTcpPort -HostName $TargetHost -Port 445
    Write-RdpClientPortCheck -Probe $RdpProbe -Name 'RDP transport' -Port $RdpPort
    Write-RdpClientPortCheck -Probe $RpcProbe -Name 'RPC Endpoint Mapper' -Port 135
    Write-RdpClientPortCheck -Probe $SmbProbe -Name 'SMB / File and Printer Sharing' -Port 445

    try {
        $ResolvedSshEntry = Resolve-RdpClientPeerSshEntryPath -Value $SshEntryFile
        Assert-RdpClientPeerSshEntryIsSeparate `
            -SshEntryPath $ResolvedSshEntry `
            -RdpEntryPath $ResolvedEntry
        $RemoteScriptPath = Join-Path $PSScriptRoot 'shadow-doctor.remote.ps1'
        if (-not [IO.File]::Exists($RemoteScriptPath)) {
            throw "RDP Shadow remote doctor script not found: $RemoteScriptPath"
        }
        $RemoteSource = [IO.File]::ReadAllText($RemoteScriptPath, [Text.Encoding]::UTF8)
        $Invocation = Invoke-RdpClientPeerSshPowerShell `
            -SshEntryPath $ResolvedSshEntry `
            -RemoteSource $RemoteSource
        if ($Invocation.ExitCode -ne 0) {
            throw "SSH diagnostics failed with exit code $($Invocation.ExitCode)."
        }
        $Remote = ConvertFrom-RdpClientRemoteDoctorOutput -Output $Invocation.Output
        if ([int]$Remote.Protocol -ne 1) {
            throw "Unsupported remote doctor protocol: $($Remote.Protocol)"
        }
        Write-RdpClientDoctorCheck -State PASS -Name 'SSH remote diagnostics' -Detail (
            "$($Remote.ComputerName) via $ResolvedSshEntry"
        )
    } catch {
        Write-RdpClientDoctorCheck -State FAIL -Name 'SSH remote diagnostics' -Detail (
            $_.Exception.Message
        )
        $Remote = $null
    }

    if ($null -ne $Remote) {
        if ([bool]$Remote.IsAdministrator) {
            Write-RdpClientDoctorCheck -State PASS -Name 'SSH account' -Detail 'administrator token'
        } else {
            Write-RdpClientDoctorCheck -State WARN -Name 'SSH account' -Detail (
                'not an administrator token; some diagnostics or future setup may be unavailable'
            )
        }

        if (-not [string]::IsNullOrWhiteSpace([string]$Remote.RollbackError)) {
            Write-RdpClientDoctorCheck -State FAIL -Name 'Peer rollback' -Detail (
                [string]$Remote.RollbackError
            )
        } elseif ([bool]$Remote.RollbackPresent) {
            $Settings = @()
            if ([bool]$Remote.RollbackAllowRemoteRPC) { $Settings += 'AllowRemoteRPC' }
            if ([bool]$Remote.RollbackShadowPolicy) { $Settings += 'Shadow' }
            Write-RdpClientDoctorCheck -State PASS -Name 'Peer rollback' -Detail (
                'HKLM:\SOFTWARE\swaw-kit\rollback\rdp-client\shadow; settings=' +
                ($Settings -join ',')
            )
        } else {
            Write-RdpClientDoctorCheck -State INFO -Name 'Peer rollback' -Detail (
                'not present; no peer setting is pending restore'
            )
        }

        foreach ($ServiceName in @('RpcSs', 'LanmanServer', 'TermService')) {
            $Service = @($Remote.Services | Where-Object { $_.Name -eq $ServiceName } | Select-Object -First 1)
            if ($Service.Count -eq 1 -and $Service[0].Present -and $Service[0].Status -eq 'Running') {
                Write-RdpClientDoctorCheck -State PASS -Name "Service $ServiceName" -Detail 'Running'
            } else {
                $Status = if ($Service.Count -eq 1 -and $Service[0].Present) {
                    $Service[0].Status
                } else {
                    'not found'
                }
                Write-RdpClientDoctorCheck -State FAIL -Name "Service $ServiceName" -Detail $Status
            }
        }

        if ($Remote.AllowRemoteRPC.Present -and $Remote.AllowRemoteRPC.Value -eq '1') {
            Write-RdpClientDoctorCheck -State PASS -Name 'AllowRemoteRPC' -Detail (
                'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server = 1'
            )
        } else {
            $Value = if ($Remote.AllowRemoteRPC.Present) { $Remote.AllowRemoteRPC.Value } else { 'absent' }
            Write-RdpClientDoctorCheck -State FAIL -Name 'AllowRemoteRPC' -Detail $Value
        }

        if (-not $Remote.ShadowPolicy.Present) {
            Write-RdpClientDoctorCheck -State INFO -Name 'Shadow policy' -Detail (
                'not configured; Windows/session defaults determine consent'
            )
        } else {
            $PolicyDescriptions = @{
                '0' = 'remote control disabled'
                '1' = 'full control with consent'
                '2' = 'full control without consent'
                '3' = 'view with consent'
                '4' = 'view without consent'
            }
            $PolicyValue = [string]$Remote.ShadowPolicy.Value
            if ($PolicyValue -eq '0') {
                Write-RdpClientDoctorCheck -State FAIL -Name 'Shadow policy' -Detail (
                    '0 (remote control disabled)'
                )
            } elseif ($PolicyDescriptions.ContainsKey($PolicyValue)) {
                Write-RdpClientDoctorCheck -State PASS -Name 'Shadow policy' -Detail (
                    "$PolicyValue ($($PolicyDescriptions[$PolicyValue]))"
                )
            } else {
                Write-RdpClientDoctorCheck -State WARN -Name 'Shadow policy' -Detail (
                    "unknown value $PolicyValue"
                )
            }
        }

        if ([string]::IsNullOrWhiteSpace([string]$Remote.NetworkProfileError)) {
            foreach ($Profile in @($Remote.NetworkProfiles)) {
                $State = if ($Profile.Category -eq 'Public') { 'WARN' } else { 'INFO' }
                Write-RdpClientDoctorCheck -State $State -Name 'Network profile' -Detail (
                    "$($Profile.InterfaceAlias): $($Profile.Category)"
                )
            }
        } else {
            Write-RdpClientDoctorCheck -State WARN -Name 'Network profile query' -Detail (
                [string]$Remote.NetworkProfileError
            )
        }

        if ([string]::IsNullOrWhiteSpace([string]$Remote.FirewallError)) {
            $RemoteRules = @($Remote.FirewallRules)
            Write-RdpClientFirewallFamilyCheck `
                -Rules $RemoteRules `
                -Names @(
                    'RemoteDesktop-Shadow-In-TCP',
                    'swaw-kit-rdp-shadow-transport'
                ) `
                -Label 'RDP Shadow firewall rule' `
                -PortReachable $RdpProbe.Connected
            Write-RdpClientFirewallFamilyCheck `
                -Rules $RemoteRules `
                -Names @(
                    'FPS-RPCSS-In-TCP',
                    'FPS-RPCSS-In-TCP-V2',
                    'swaw-kit-rdp-shadow-rpc'
                ) `
                -Label 'File and Printer Sharing RPC rules' `
                -PortReachable $RpcProbe.Connected
            Write-RdpClientFirewallFamilyCheck `
                -Rules $RemoteRules `
                -Names @(
                    'FPS-SMB-In-TCP',
                    'FPS-SMB-In-TCP-V2',
                    'swaw-kit-rdp-shadow-smb'
                ) `
                -Label 'File and Printer Sharing SMB rules' `
                -PortReachable $SmbProbe.Connected
        } else {
            Write-RdpClientDoctorCheck -State FAIL -Name 'Firewall rule query' -Detail (
                [string]$Remote.FirewallError
            )
        }

        if ($Remote.QuserPresent -and [int]$Remote.QuserExitCode -eq 0) {
            Write-RdpClientDoctorCheck -State PASS -Name 'Session query' -Detail 'quser succeeded'
        } else {
            Write-RdpClientDoctorCheck -State FAIL -Name 'Session query' -Detail (
                "quser present=$($Remote.QuserPresent), exit=$($Remote.QuserExitCode)"
            )
        }
    }

    Write-RdpClientDoctorCheck -State INFO -Name 'Not preflightable' -Detail (
        'mstsc credential, target-session permission, user consent, and video compatibility'
    )

    if ($script:DoctorFailures -gt 0) {
        Write-Host (
            "[RDP] Shadow doctor: NOT READY ($($script:DoctorFailures) failure(s), " +
            "$($script:DoctorWarnings) warning(s))"
        )
        Write-Host "[RDP] Run `"$CommandName .help`" for Shadow setup guidance."
        exit 1
    }

    Write-Host "[RDP] Shadow doctor: READY ($($script:DoctorWarnings) warning(s))"
    exit 0
} catch {
    [Console]::Error.WriteLine("[ERROR] $($_.Exception.Message)")
    exit 1
}
