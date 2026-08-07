[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [AllowEmptyString()]
    [string]$SshEntryFile,

    [Parameter(Mandatory = $true)]
    [string]$RdpEntryFile,

    [string]$CommandName = 'rdp'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'peer-ssh.ps1')
. (Join-Path $PSScriptRoot 'session.ps1')

try {
    $Utf8NoBom = New-Object Text.UTF8Encoding($false)
    [Console]::InputEncoding = $Utf8NoBom
    [Console]::OutputEncoding = $Utf8NoBom
    $OutputEncoding = $Utf8NoBom

    $ResolvedSshEntry = Resolve-RdpClientPeerSshEntryPath -Value $SshEntryFile
    $ResolvedRdpEntry = [IO.Path]::GetFullPath($RdpEntryFile)
    Assert-RdpClientPeerSshEntryIsSeparate `
        -SshEntryPath $ResolvedSshEntry `
        -RdpEntryPath $ResolvedRdpEntry

    $State = Get-RdpClientPeerSessionState -SshEntryPath $ResolvedSshEntry
    $Sessions = @($State.Sessions | Sort-Object { [int]$_.Id })

    Write-Host '[RDP] Peer sessions'
    Write-Host ("  Peer: {0}" -f $State.ComputerName)
    if ($Sessions.Count -eq 0) {
        Write-Host '  No interactive sessions.'
        exit 0
    }

    Write-Output ''
    Write-Output (' {0,4}  {1,-28} {2,-14} {3,-12} {4,-8} {5}' -f `
        'ID', 'USERNAME', 'SESSIONNAME', 'STATE', 'LOCK', 'TERMINAL')
    foreach ($Session in $Sessions) {
        $UserName = Get-RdpClientSessionDisplayUserName -Session $Session
        $Lock = if ($null -eq $Session.Locked) {
            '-'
        } elseif ([bool]$Session.Locked) {
            'Locked'
        } else {
            'Unlocked'
        }
        Write-Output (' {0,4}  {1,-28} {2,-14} {3,-12} {4,-8} {5}' -f `
            $Session.Id,
            $UserName,
            $Session.SessionName,
            $Session.State,
            $Lock,
            $Session.Terminal)
    }
    exit 0
} catch {
    [Console]::Error.WriteLine("[ERROR] $($_.Exception.Message)")
    [Console]::Error.WriteLine(
        "[ERROR] Run `"$CommandName .help`" for session setup guidance."
    )
    exit 1
}
