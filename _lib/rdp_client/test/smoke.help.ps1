[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$OutputEncoding = New-Object Text.UTF8Encoding($false)

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$TemplateEntry = Join-Path $RepoRoot 'Favorites\template.rdp1.cmd'
if (-not [IO.File]::Exists($TemplateEntry)) {
    throw "RDP entry template not found: $TemplateEntry"
}
$HelpTemplate = Join-Path $PSScriptRoot '..\help\zh-CN.txt'
$HelpHeading = [IO.File]::ReadAllLines(
    $HelpTemplate,
    [Text.Encoding]::UTF8
)[0]
$EnglishHelpTemplate = Join-Path $PSScriptRoot '..\help\en.txt'
$EnglishHelpHeading = [IO.File]::ReadAllLines(
    $EnglishHelpTemplate,
    [Text.Encoding]::UTF8
)[0]

$EntryName = '.rdp-help-test-' + [Guid]::NewGuid().ToString('N') + '.cmd'
$Entry = Join-Path $RepoRoot $EntryName
$EntryCommand = [IO.Path]::GetFileNameWithoutExtension($Entry)

function Invoke-HelpTestCommand {
    param(
        [string]$CommandPath = $Entry,
        [AllowEmptyCollection()][string[]]$Arguments,
        [int]$ExpectedExitCode
    )

    $PreviousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Output = (& $CommandPath @Arguments 2>&1 | Out-String)
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousPreference
    }

    if ($ExitCode -ne $ExpectedExitCode) {
        throw "Unexpected exit code for '$($Arguments -join ' ')': $ExitCode`n$Output"
    }
    return $Output
}

try {
    [IO.File]::Copy($TemplateEntry, $Entry)
    $EntryText = [IO.File]::ReadAllText($Entry, [Text.Encoding]::UTF8)
    $EntryText = [regex]::Replace(
        $EntryText,
        '(?m)^set "RDP_HELP_LANG=.*"\r?$',
        'set "RDP_HELP_LANG=zh-CN"'
    )
    [IO.File]::WriteAllText(
        $Entry,
        $EntryText,
        (New-Object Text.UTF8Encoding($false))
    )

    $TemplateOutput = Invoke-HelpTestCommand `
        -CommandPath $TemplateEntry `
        -Arguments @('--help') `
        -ExpectedExitCode 0
    if (-not $TemplateOutput.Contains('template.rdp1 .help')) {
        throw "The template should run directly from Favorites.`n$TemplateOutput"
    }
    if ($TemplateOutput.Contains('not recognized as an internal or external command')) {
        throw "The UTF-8 entry template executed documentation as commands.`n$TemplateOutput"
    }

    foreach ($Arguments in @(
        [string[]]@('.help'),
        [string[]]@('.h'),
        [string[]]@('-h'),
        [string[]]@('--help')
    )) {
        $Output = Invoke-HelpTestCommand -Arguments $Arguments -ExpectedExitCode 0
        foreach ($Expected in @(
            $HelpHeading,
            "$EntryCommand .help",
            "$EntryCommand .rdp create",
            "$EntryCommand .rdp create --force",
            "$EntryCommand .list",
            "$EntryCommand .2",
            "$EntryCommand .2 screenshot",
            "$EntryCommand .2 screenshot --display",
            "$EntryCommand .2 pixel 640 360",
            "$EntryCommand .2 click 640 360",
            "$EntryCommand .2 script workflow.ps1",
            '--timeout 60s',
            '.peer psexec add',
            'RDP_PEER_SSH_ENTRY',
            "$EntryCommand .shadow doctor",
            "$EntryCommand .shadow console",
            "$EntryCommand .shadow console --display",
            "$EntryCommand .shadow console --tscon 3",
            "$EntryCommand .shadow 2",
            "$EntryCommand .shadow 3 --control",
            "$EntryCommand .peer shadow enable",
            "$EntryCommand .peer shadow status",
            "$EntryCommand .peer shadow mode",
            "$EntryCommand .peer shadow restore",
            "$EntryCommand .peer psexec status",
            "$EntryCommand .peer psexec add",
            "$EntryCommand .peer psexec remove",
            "$EntryCommand .peer psexec 2 notepad.exe",
            "$EntryCommand .peer psexec --",
            '-accepteula',
            "$EntryCommand .hosts status",
            "$EntryCommand .hosts install",
            "$EntryCommand .hosts install --uac",
            "$EntryCommand .hosts remove",
            "$EntryCommand .hosts remove --uac",
            "$EntryCommand .hosts cleanup --dry-run",
            "$EntryCommand .hosts cleanup",
            "$EntryCommand .hosts cleanup --uac",
            "$EntryCommand .sign status",
            "$EntryCommand .sign install",
            "$EntryCommand .sign install --dry-run",
            "$EntryCommand .sign remove",
            "$EntryCommand .sign open"
        )) {
            if (-not $Output.Contains($Expected)) {
                throw "Help output is missing '$Expected'.`n$Output"
            }
        }
        if ($Output.Contains('{{COMMAND}}')) {
            throw "Help placeholder was not replaced.`n$Output"
        }
        if ($Output.Contains('not recognized as an internal or external command')) {
            throw "RDP help emitted a cmd.exe pseudo-comment error.`n$Output"
        }
        if ($Output.Contains("$EntryCommand .connect")) {
            throw "Help should not advertise the removed .connect alias.`n$Output"
        }
        if ($Output.Contains("$EntryCommand .signing")) {
            throw "Help should not advertise the removed .signing command.`n$Output"
        }
        if ($Output.Contains("$EntryCommand .console")) {
            throw "Help should not advertise ordinary-RDP .console.`n$Output"
        }
        foreach ($RemovedHelpName in @(
            'RDP_REMOTE_HOST',
            'RDP_REAL_HOST',
            'RDP_USERNAME'
        )) {
            if ($Output.Contains($RemovedHelpName)) {
                throw "Help still contains '$RemovedHelpName'.`n$Output"
            }
        }
    }

    $English = Invoke-HelpTestCommand `
        -Arguments @('.help', 'en') `
        -ExpectedExitCode 0
    if (-not $English.Contains($EnglishHelpHeading) -or
        $English.Contains($HelpHeading) -or
        -not $English.Contains('Start Remote Desktop') -or
        -not $English.Contains('Non-interactive install or repair')) {
        throw "An explicit English language should override the entry default.`n$English"
    }
    $Chinese = Invoke-HelpTestCommand `
        -Arguments @('.help', 'zh') `
        -ExpectedExitCode 0
    if (-not $Chinese.Contains($HelpHeading) -or
        $Chinese.Contains($EnglishHelpHeading)) {
        throw "An explicit Chinese language should select zh-CN.`n$Chinese"
    }
    $UnknownLanguage = Invoke-HelpTestCommand `
        -Arguments @('.help', 'fr') `
        -ExpectedExitCode 0
    if (-not $UnknownLanguage.Contains($EnglishHelpHeading)) {
        throw "An unsupported configured language should fall back to English.`n$UnknownLanguage"
    }

    $TemplateText = [IO.File]::ReadAllText(
        $TemplateEntry,
        [Text.Encoding]::UTF8
    )
    if ($TemplateText -match '(?<!\r)\n') {
        throw 'RDP entry template must use CRLF line endings for cmd.exe.'
    }

    foreach ($TemplateLine in ($TemplateText -split "`r`n")) {
        if ($TemplateLine.StartsWith('::') -and
            $TemplateLine -match '[^\x00-\x7F]' -and
            $TemplateLine -match '[^\x00-\x7F]$') {
            throw "A non-ASCII RDP template comment must end with an ASCII character: $TemplateLine"
        }
    }

    foreach ($RequiredProperty in @(
        'set "RDP_OUTPUT_PATH="',
        'set "RDP_HELP_LANG="',
        'full address:s:',
        'username:s:',
        'screen mode id:i:1',
        'winposstr:s:0,1,44,89,3000,2000',
        'desktopwidth:i:1200',
        'desktopheight:i:800',
        'use multimon:i:0',
        'keyboardhook:i:1',
        'redirectclipboard:i:1',
        ':: drivestoredirect:s:D:\;',
        'redirectprinters:i:0',
        'redirectcomports:i:0',
        'redirectwebauthn:i:0',
        'redirectsmartcards:i:0',
        'remoteapplicationmode:i:0',
        'remoteapplicationprogram:s:C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe',
        'remoteapplicationcmdline:s:https://chat.openai.com/chat',
        'remoteapplicationexpandworkingdir:i:1',
        'remoteapplicationname:s:chatGPT',
        'remoteapplicationicon:s:'
    )) {
        if (-not $TemplateText.Contains($RequiredProperty)) {
            throw "RDP template is missing '$RequiredProperty'."
        }
    }

    if ($TemplateText -notmatch '(?m)^set "RDP_PEER_SSH_ENTRY=.*"\r?$') {
        throw 'RDP template is missing the RDP_PEER_SSH_ENTRY assignment.'
    }

    if ($TemplateText -match 'RDP_SHADOW_SSH_ENTRY') {
        throw 'The retired RDP_SHADOW_SSH_ENTRY compatibility name must not remain in the template.'
    }
    foreach ($RemovedVariable in @(
        'set "RDP_REMOTE_HOST=',
        'set "RDP_REAL_HOST=',
        'set "RDP_USERNAME='
    )) {
        if ($TemplateText.Contains($RemovedVariable)) {
            throw "RDP template still contains '$RemovedVariable'."
        }
    }

    $Unknown = Invoke-HelpTestCommand `
        -Arguments @('.unknown') `
        -ExpectedExitCode 1
    if (-not $Unknown.Contains('Unknown RDP command: .unknown')) {
        throw "Unknown commands should fail explicitly.`n$Unknown"
    }

    $InvalidHosts = Invoke-HelpTestCommand `
        -Arguments @('.hosts', 'install', '--unexpected') `
        -ExpectedExitCode 1
    if (-not $InvalidHosts.Contains('Hosts usage:')) {
        throw "Invalid hosts arguments should show hosts usage.`n$InvalidHosts"
    }

    $InvalidSigning = Invoke-HelpTestCommand `
        -Arguments @('.sign', 'install', '--unexpected') `
        -ExpectedExitCode 1
    if (-not $InvalidSigning.Contains('Sign usage:')) {
        throw "Invalid signing arguments should show signing usage.`n$InvalidSigning"
    }

    $ExtraDryRunArgument = Invoke-HelpTestCommand `
        -Arguments @('.sign', 'install', '--dry-run', '--unexpected') `
        -ExpectedExitCode 1
    if (-not $ExtraDryRunArgument.Contains('Sign usage:')) {
        throw "Signing dry-run should reject extra arguments.`n$ExtraDryRunArgument"
    }

    $RejectedSignUac = Invoke-HelpTestCommand `
        -Arguments @('.sign', 'install', '--uac') `
        -ExpectedExitCode 1
    if (-not $RejectedSignUac.Contains('Sign usage:')) {
        throw "Signing commands should reject --uac.`n$RejectedSignUac"
    }

    foreach ($InvalidShadowArguments in @(
        [string[]]@('.shadow'),
        [string[]]@('.shadow', 'abc'),
        [string[]]@('.shadow', 'start', '2'),
        [string[]]@('.shadow', '2', '--unexpected'),
        [string[]]@('.shadow', '2', '--control', '--control'),
        [string[]]@('.shadow', '2', '--display'),
        [string[]]@('.shadow', '2', '--tscon', '3'),
        [string[]]@('.shadow', 'doctor', 'unexpected'),
        [string[]]@('.shadow', 'enable', '--unexpected'),
        [string[]]@('.shadow', 'enable', '--dry-run', 'unexpected'),
        [string[]]@('.shadow', 'restore', '--unexpected'),
        [string[]]@('.shadow', 'list'),
        [string[]]@('.shadow', 'console', '--unexpected'),
        [string[]]@('.shadow', 'console', '--display', '--display'),
        [string[]]@('.shadow', 'console', '--display', '--tscon', '3'),
        [string[]]@('.shadow', 'console', '--tscon'),
        [string[]]@('.shadow', 'console', '--tscon', 'abc'),
        [string[]]@('.shadow', 'console', '--tscon', '3', '--tscon', '4')
    )) {
        $InvalidShadow = Invoke-HelpTestCommand `
            -Arguments $InvalidShadowArguments `
            -ExpectedExitCode 1
        if (-not $InvalidShadow.Contains(
            "$EntryCommand .shadow <session-id>"
        )) {
            throw "Invalid Shadow arguments should show Shadow usage.`n$InvalidShadow"
        }
    }

    foreach ($InvalidSessionArguments in @(
        [string[]]@('.list', 'unexpected'),
        [string[]]@('.2', 'unexpected'),
        [string[]]@('.2', 'pixel', '640'),
        [string[]]@('.2', 'click', '640', '360', '--output', 'x.png'),
        [string[]]@('.2', 'screenshot', '--display', '--display'),
        [string[]]@('.2', 'screenshot', '--timeout')
    )) {
        $InvalidSession = Invoke-HelpTestCommand `
            -Arguments $InvalidSessionArguments `
            -ExpectedExitCode 1
        if (-not $InvalidSession.Contains("$EntryCommand .list") -or
            -not $InvalidSession.Contains("$EntryCommand .<session-id>")) {
            throw "Invalid session arguments should show session usage.`n$InvalidSession"
        }
    }

    foreach ($InvalidPeerArguments in @(
        [string[]]@('.peer'),
        [string[]]@('.peer', 'shadow'),
        [string[]]@('.peer', 'shadow', 'status', 'unexpected'),
        [string[]]@('.peer', 'shadow', 'mode'),
        [string[]]@('.peer', 'shadow', 'mode', '5'),
        [string[]]@('.peer', 'shadow', 'enable', '--unexpected'),
        [string[]]@('.peer', 'shadow', 'restore', '--unexpected'),
        [string[]]@('.peer', 'psexec'),
        [string[]]@('.peer', 'psexec', 'status', 'unexpected'),
        [string[]]@('.peer', 'psexec', 'add', '--unexpected'),
        [string[]]@('.peer', 'psexec', 'add', '--dry-run', 'unexpected'),
        [string[]]@('.peer', 'psexec', 'remove', '--unexpected'),
        [string[]]@('.peer', 'psexec', '--'),
        [string[]]@('.peer', 'psexec', '2')
    )) {
        $InvalidPeer = Invoke-HelpTestCommand `
            -Arguments $InvalidPeerArguments `
            -ExpectedExitCode 1
        if (-not $InvalidPeer.Contains(
            "$EntryCommand .peer shadow mode <0-4>"
        )) {
            throw "Invalid Peer arguments should show Peer usage.`n$InvalidPeer"
        }
    }

    $RemovedLaunchCommand = Invoke-HelpTestCommand `
        -Arguments @('.launch', '2', '--', 'notepad.exe') `
        -ExpectedExitCode 1
    if (-not $RemovedLaunchCommand.Contains('Unknown RDP command: .launch')) {
        throw "The removed .launch command should fail explicitly.`n$RemovedLaunchCommand"
    }

    $RemovedSigningCommand = Invoke-HelpTestCommand `
        -Arguments @('.signing', 'status') `
        -ExpectedExitCode 1
    if (-not $RemovedSigningCommand.Contains('Unknown RDP command: .signing')) {
        throw "The removed .signing command should fail explicitly.`n$RemovedSigningCommand"
    }

    $ExtraHelpArgument = Invoke-HelpTestCommand `
        -Arguments @('.help', 'zh', 'unexpected') `
        -ExpectedExitCode 1
    if (-not $ExtraHelpArgument.Contains('Help usage:')) {
        throw "Help should reject extra arguments.`n$ExtraHelpArgument"
    }

    Write-Host 'rdp client help tests: PASS' -ForegroundColor Green
} finally {
    if ([IO.File]::Exists($Entry)) {
        [IO.File]::Delete($Entry)
    }
}
