[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$OutputEncoding = New-Object Text.UTF8Encoding($false)

$RuntimeRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $RuntimeRoot 'launch-ui.ps1')

$ExplorerSnapshot = @(
    [pscustomobject]@{ ProcessId = 100; ParentProcessId = 200; Name = 'powershell.exe' },
    [pscustomobject]@{ ProcessId = 200; ParentProcessId = 300; Name = 'cmd.exe' },
    [pscustomobject]@{ ProcessId = 300; ParentProcessId = 400; Name = 'explorer.exe' }
)
if (-not (Test-RdpClientExplorerLaunch `
    -CurrentProcessId 100 `
    -ProcessSnapshot $ExplorerSnapshot)) {
    throw 'Explorer -> cmd -> PowerShell should be recognized as a double-click launch.'
}

$TerminalSnapshot = @(
    [pscustomobject]@{ ProcessId = 100; ParentProcessId = 200; Name = 'powershell.exe' },
    [pscustomobject]@{ ProcessId = 200; ParentProcessId = 300; Name = 'cmd.exe' },
    [pscustomobject]@{ ProcessId = 300; ParentProcessId = 400; Name = 'pwsh.exe' }
)
if (Test-RdpClientExplorerLaunch `
    -CurrentProcessId 100 `
    -ProcessSnapshot $TerminalSnapshot) {
    throw 'A terminal-launched cmd entry must not be treated as an Explorer launch.'
}
if (Test-RdpClientExplorerLaunch `
    -CurrentProcessId 999 `
    -ProcessSnapshot $ExplorerSnapshot) {
    throw 'An absent current process must fail closed as a non-Explorer launch.'
}
if (Test-RdpClientExplorerLaunch) {
    throw 'The smoke test terminal must not be treated as an Explorer double click.'
}

$script:NativeResult = 6
$script:CapturedMessage = ''
$script:CapturedTitle = ''
$script:CapturedStyle = [uint32]0
function Show-RdpClientNativeMessage {
    param([string]$Message, [string]$Title, [uint32]$Style)
    $script:CapturedMessage = $Message
    $script:CapturedTitle = $Title
    $script:CapturedStyle = $Style
    return $script:NativeResult
}

$PreviousLanguage = $env:RDP_HELP_LANG
try {
    $env:RDP_HELP_LANG = 'en'
    $HostsDecision = Request-RdpClientHostsInstall `
        -HostAlias 'account.example.test' `
        -CommandName 'rdp.test'
    if ($HostsDecision -ne 'Install' -or
        -not $script:CapturedMessage.Contains('account.example.test') -or
        -not $script:CapturedMessage.Contains(
            'rdp.test .hosts install --uac'
        ) -or $script:CapturedStyle -ne 0x34) {
        throw 'The Explorer hosts prompt is missing its exact action or alias.'
    }

    $script:NativeResult = 7
    if ((Request-RdpClientHostsInstall `
        -HostAlias 'account.example.test' `
        -CommandName 'rdp.test') -ne 'Cancel') {
        throw 'Declining the hosts prompt should cancel the connection.'
    }

    $script:NativeResult = 6
    $SigningDecision = Request-RdpClientSigningSetup -CommandName 'rdp.test'
    if ($SigningDecision -ne 'Install' -or
        -not $script:CapturedMessage.Contains('rdp.test .sign install') -or
        -not $script:CapturedMessage.Contains(
            'does not require administrator privileges'
        ) -or $script:CapturedStyle -ne 0x23) {
        throw 'The signing prompt must explain its exact command and privilege scope.'
    }

    $script:NativeResult = 7
    if ((Request-RdpClientSigningSetup -CommandName 'rdp.test') -ne
        'ContinueUnsigned') {
        throw 'No should explicitly continue with an unsigned RDP file.'
    }
    $script:NativeResult = 2
    if ((Request-RdpClientSigningSetup -CommandName 'rdp.test') -ne
        'Cancel') {
        throw 'Cancel should stop signing setup and connection.'
    }

    Show-RdpClientLaunchError `
        -Message 'synthetic failure' `
        -CommandName 'rdp.test'
    if (-not $script:CapturedMessage.Contains('synthetic failure') -or
        $script:CapturedStyle -ne 0x10) {
        throw 'Explorer failures should be visible in a native error dialog.'
    }
} finally {
    if ($null -eq $PreviousLanguage) {
        Remove-Item Env:RDP_HELP_LANG -ErrorAction SilentlyContinue
    } else {
        $env:RDP_HELP_LANG = $PreviousLanguage
    }
}

Write-Host 'rdp client launch UI tests: PASS' -ForegroundColor Green
