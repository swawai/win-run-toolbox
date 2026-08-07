[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$EntryFile,

    [Parameter(Mandatory = $true)]
    [AllowEmptyString()]
    [string]$SshEntryFile,

    [Parameter(Mandatory = $true)]
    [string]$SessionId,

    [string]$CommandName = 'rdp',

    [switch]$Control,

    [switch]$NoConsentPrompt
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'entry.ps1')
. (Join-Path $PSScriptRoot 'peer-ssh.ps1')
. (Join-Path $PSScriptRoot 'session.ps1')

try {
    $Utf8NoBom = New-Object Text.UTF8Encoding($false)
    [Console]::OutputEncoding = $Utf8NoBom
    $OutputEncoding = $Utf8NoBom

    $ResolvedEntry = [IO.Path]::GetFullPath($EntryFile)
    $ConsoleSession = $null
    if ([string]::Equals(
        $SessionId,
        'console',
        [StringComparison]::OrdinalIgnoreCase
    )) {
        $ResolvedSshEntry = Resolve-RdpClientPeerSshEntryPath `
            -Value $SshEntryFile
        Assert-RdpClientPeerSshEntryIsSeparate `
            -SshEntryPath $ResolvedSshEntry `
            -RdpEntryPath $ResolvedEntry
        $SessionState = Get-RdpClientPeerSessionState `
            -SshEntryPath $ResolvedSshEntry
        $ConsoleSession = Resolve-RdpClientShadowConsoleSession `
            -State $SessionState
        $ResolvedSessionId = [uint32]$ConsoleSession.Id
    } else {
        $ResolvedSessionId = Resolve-RdpClientShadowSessionId -Value $SessionId
        if ($null -eq $ResolvedSessionId) {
            throw 'Shadow session ID is required.'
        }
    }

    $HostAlias = Resolve-RdpClientHostAlias -Value $env:RDP_HOST_ALIAS
    $Document = Read-RdpClientEntryDocument -Path $ResolvedEntry
    $Target = Resolve-RdpClientConnectionTarget `
        -Document $Document `
        -HostAlias $HostAlias
    Assert-RdpClientHostAliasResolves `
        -HostAlias $HostAlias `
        -CommandName $CommandName

    $MstscArguments = New-RdpClientShadowMstscArgumentList `
        -Target $Target `
        -ShadowSessionId $ResolvedSessionId `
        -Control:$Control `
        -NoConsentPrompt:$NoConsentPrompt
    $Mstsc = Get-Command 'mstsc.exe' -ErrorAction Stop
    Start-Process `
        -FilePath $Mstsc.Source `
        -ArgumentList $MstscArguments |
        Out-Null

    $ShadowTarget = Resolve-RdpClientShadowConnectionTarget -Target $Target
    $Access = if ($Control) { 'control' } else { 'view only' }
    $Consent = if ($NoConsentPrompt) { 'no consent prompt' } else { 'user consent' }
    Write-Host "[RDP] Target:    $ShadowTarget"
    if ($null -ne $ConsoleSession) {
        $ConsoleUser = Get-RdpClientSessionDisplayUserName `
            -Session $ConsoleSession
        Write-Host (
            '[RDP] Console:   session {0} ({1})' -f `
                $ResolvedSessionId,
                $ConsoleUser
        )
    }
    Write-Host "[RDP] Shadow:    session $ResolvedSessionId ($Access; $Consent)"
    Write-Host '[RDP] Started mstsc.exe.'
    exit 0
} catch {
    [Console]::Error.WriteLine("[ERROR] $($_.Exception.Message)")
    exit 1
}
