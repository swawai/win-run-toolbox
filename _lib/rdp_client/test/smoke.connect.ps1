[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$OutputEncoding = New-Object Text.UTF8Encoding($false)

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$TemplateEntry = Join-Path $RepoRoot 'Favorites\template.rdp1.cmd'
$DataDirectory = Join-Path $RepoRoot 'data\rdp-client'
$EntryName = '.rdp-connect-test-' + [Guid]::NewGuid().ToString('N')
$Entry = Join-Path $RepoRoot ($EntryName + '.cmd')
$OutputPath = Join-Path $DataDirectory ($EntryName + '.rdp')
$CustomOutputPath = Join-Path $DataDirectory ($EntryName + '.custom.rdp')
$CacheDirectory = Join-Path $DataDirectory $EntryName
$ManifestPath = Join-Path $CacheDirectory 'manifest.json'

function Invoke-RdpEntry {
    param(
        [string[]]$Arguments,
        [int]$ExpectedExitCode
    )

    $PreviousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Output = (& $Entry @Arguments 2>&1 | Out-String)
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
    $FixtureText = [IO.File]::ReadAllText($Entry, [Text.Encoding]::UTF8)
    $FixtureText = [regex]::Replace(
        $FixtureText,
        '(?m)^set "RDP_HOST_ALIAS=.*"\r?$',
        'set "RDP_HOST_ALIAS=swaw-kit.administrator.rdp.home.arpa"'
    )
    $FixtureText = [regex]::Replace(
        $FixtureText,
        '(?m)^set "RDP_OUTPUT_PATH=.*"\r?$',
        "set `"RDP_OUTPUT_PATH=$OutputPath`""
    )
    $FixtureText = [regex]::Replace(
        $FixtureText,
        '(?m)^full address:s:.*\r?$',
        'full address:s:192.168.1.115:3389'
    )
    $FixtureText = [regex]::Replace(
        $FixtureText,
        '(?m)^username:s:.*\r?$',
        'username:s:administrator'
    )
    [IO.File]::WriteAllText(
        $Entry,
        $FixtureText,
        (New-Object Text.UTF8Encoding($false))
    )

    $GenerateOutput = Invoke-RdpEntry -Arguments @('.rdp', 'create') -ExpectedExitCode 0
    if (-not $GenerateOutput.Contains("[RDP] Generated: $OutputPath")) {
        throw "The generator did not report its output path.`n$GenerateOutput"
    }
    if (-not [IO.File]::Exists($OutputPath)) {
        throw "Generated RDP file not found: $OutputPath"
    }
    if (-not [IO.File]::Exists($ManifestPath)) {
        throw "RDP artifact manifest not found: $ManifestPath"
    }

    $Manifest = [IO.File]::ReadAllText(
        $ManifestPath,
        [Text.Encoding]::UTF8
    ) | ConvertFrom-Json
    if ($Manifest.version -ne 1 -or
        $Manifest.entryPath -ne $Entry -or
        $Manifest.outputPath -ne $OutputPath -or
        $Manifest.sourceHash -notmatch '^[0-9A-F]{64}$' -or
        $Manifest.outputHash -notmatch '^[0-9A-F]{64}$' -or
        [string]::IsNullOrWhiteSpace($Manifest.signingIdentity)) {
        throw "The RDP artifact manifest is incomplete.`n$($Manifest | ConvertTo-Json)"
    }

    $Bytes = [IO.File]::ReadAllBytes($OutputPath)
    if ($Bytes.Length -lt 2 -or $Bytes[0] -ne 0xFF -or $Bytes[1] -ne 0xFE) {
        throw 'Generated RDP file must be UTF-16 LE with a BOM.'
    }
    $RdpText = [IO.File]::ReadAllText($OutputPath, [Text.Encoding]::Unicode)
    foreach ($Expected in @(
        'full address:s:swaw-kit.administrator.rdp.home.arpa:3389',
        'username:s:administrator',
        'remoteapplicationmode:i:0',
        'redirectclipboard:i:1'
    )) {
        if (-not $RdpText.Contains($Expected)) {
            throw "Generated RDP file is missing '$Expected'.`n$RdpText"
        }
    }
    foreach ($Forbidden in @(
        'full address:s:192.168.1.115:3389',
        'drivestoredirect:',
        '::',
        'goto :RdpClientAfterEmbeddedRdpProperties',
        'set "RDP_HOST_ALIAS='
    )) {
        if ($RdpText.Contains($Forbidden)) {
            throw "Generated RDP file contains source-only text '$Forbidden'.`n$RdpText"
        }
    }

    [IO.File]::WriteAllText($OutputPath, 'stale', [Text.Encoding]::Unicode)
    $ExistingOutput = Invoke-RdpEntry `
        -Arguments @('.rdp', 'create') `
        -ExpectedExitCode 1
    if (-not $ExistingOutput.Contains('RDP file already exists:') -or
        -not $ExistingOutput.Contains('to overwrite it.')) {
        throw "The .rdp command should protect an existing file.`n$ExistingOutput"
    }
    if ([IO.File]::ReadAllText($OutputPath, [Text.Encoding]::Unicode) -ne 'stale') {
        throw 'A refused .rdp export changed the existing file.'
    }

    Invoke-RdpEntry -Arguments @('.rdp', 'create', '--force') -ExpectedExitCode 0 | Out-Null
    $Rebuilt = [IO.File]::ReadAllText($OutputPath, [Text.Encoding]::Unicode)
    if ($Rebuilt -eq 'stale' -or -not $Rebuilt.Contains('username:s:administrator')) {
        throw 'The generated RDP file was not deterministically rebuilt.'
    }

    $EntryText = [IO.File]::ReadAllText($Entry, [Text.Encoding]::UTF8)
    $EntryText = $EntryText.Replace(
        'set "RDP_HOST_ALIAS=swaw-kit.administrator.rdp.home.arpa"',
        'set "RDP_HOST_ALIAS="'
    )
    [IO.File]::WriteAllText($Entry, $EntryText, (New-Object Text.UTF8Encoding($false)))
    Invoke-RdpEntry -Arguments @('.rdp', 'create', '--force') -ExpectedExitCode 0 | Out-Null
    $DirectText = [IO.File]::ReadAllText($OutputPath, [Text.Encoding]::Unicode)
    if (-not $DirectText.Contains('full address:s:192.168.1.115:3389')) {
        throw 'An empty RDP_HOST_ALIAS should preserve the real full address.'
    }

    $UnresolvedText = $EntryText.Replace(
        'set "RDP_HOST_ALIAS="',
        'set "RDP_HOST_ALIAS=rdp-client-test.invalid"'
    )
    [IO.File]::WriteAllText($Entry, $UnresolvedText, (New-Object Text.UTF8Encoding($false)))
    $UnresolvedOutput = Invoke-RdpEntry -Arguments @() -ExpectedExitCode 1
    if (
        -not $UnresolvedOutput.Contains('RDP_HOST_ALIAS does not resolve:') -or
        -not $UnresolvedOutput.Contains(
            "Run `"$EntryName .hosts install --uac`"."
        ) -or
        $UnresolvedOutput.Contains('Configure DNS') -or
        -not $UnresolvedOutput.Contains('full address:s:rdp-client-test.invalid:3389') -or
        -not $UnresolvedOutput.Contains("[RDP] Generated: $OutputPath")
    ) {
        throw "An unresolved alias should stop before mstsc starts.`n$UnresolvedOutput"
    }

    $ReusedOutput = Invoke-RdpEntry -Arguments @() -ExpectedExitCode 1
    if (-not $ReusedOutput.Contains("[RDP] Reused:    $OutputPath") -or
        $ReusedOutput.Contains("[RDP] Generated: $OutputPath")) {
        throw "An unchanged RDP artifact should be reused.`n$ReusedOutput"
    }

    $CommentOnlyText = $UnresolvedText + "`r`n:: cache-neutral edit`r`n"
    [IO.File]::WriteAllText(
        $Entry,
        $CommentOnlyText,
        (New-Object Text.UTF8Encoding($false))
    )
    $CommentOnlyOutput = Invoke-RdpEntry -Arguments @() -ExpectedExitCode 1
    if (-not $CommentOnlyOutput.Contains("[RDP] Reused:    $OutputPath")) {
        throw "A source-only comment should not rebuild the RDP artifact.`n$CommentOnlyOutput"
    }

    [IO.File]::WriteAllText(
        $ManifestPath,
        '{not-json',
        (New-Object Text.UTF8Encoding($false))
    )
    $CorruptManifestOutput = Invoke-RdpEntry -Arguments @() -ExpectedExitCode 1
    if (-not $CorruptManifestOutput.Contains("[RDP] Generated: $OutputPath")) {
        throw "A corrupt manifest should cause a safe rebuild.`n$CorruptManifestOutput"
    }

    [IO.File]::WriteAllText($OutputPath, 'tampered', [Text.Encoding]::Unicode)
    $TamperedOutput = Invoke-RdpEntry -Arguments @() -ExpectedExitCode 1
    if (-not $TamperedOutput.Contains("[RDP] Generated: $OutputPath") -or
        $TamperedOutput.Contains("[RDP] Reused:    $OutputPath")) {
        throw "A modified RDP artifact should be rebuilt.`n$TamperedOutput"
    }
    [IO.File]::WriteAllText($Entry, $EntryText, (New-Object Text.UTF8Encoding($false)))

    $CustomText = $EntryText.Replace(
        "set `"RDP_OUTPUT_PATH=$OutputPath`"",
        "set `"RDP_OUTPUT_PATH=$CustomOutputPath`""
    )
    [IO.File]::WriteAllText($Entry, $CustomText, (New-Object Text.UTF8Encoding($false)))
    $CustomOutput = Invoke-RdpEntry -Arguments @('.rdp', 'create') -ExpectedExitCode 0
    if (-not $CustomOutput.Contains("[RDP] Generated: $CustomOutputPath") -or
        -not [IO.File]::Exists($CustomOutputPath)) {
        throw "RDP_OUTPUT_PATH was not used.`n$CustomOutput"
    }
    [IO.File]::WriteAllText($Entry, $EntryText, (New-Object Text.UTF8Encoding($false)))

    $DnsAliasText = $EntryText.Replace(
        'set "RDP_HOST_ALIAS="',
        'set "RDP_HOST_ALIAS=administrator.rdp.home.arpa"'
    ).Replace(
        'full address:s:192.168.1.115:3389',
        'full address:s:server.example.test:3389'
    )
    [IO.File]::WriteAllText($Entry, $DnsAliasText, (New-Object Text.UTF8Encoding($false)))
    $DnsAliasOutput = Invoke-RdpEntry `
        -Arguments @('.rdp', 'create', '--force') `
        -ExpectedExitCode 1
    if (-not $DnsAliasOutput.Contains(
        'RDP_HOST_ALIAS requires full address'
    )) {
        throw "A DNS source with an alias should fail explicitly.`n$DnsAliasOutput"
    }
    [IO.File]::WriteAllText($Entry, $EntryText, (New-Object Text.UTF8Encoding($false)))

    $EntryText = $EntryText.Replace(
        'full address:s:192.168.1.115:3389',
        "full address:s:192.168.1.115:3389`r`nfull address:s:duplicate"
    )
    [IO.File]::WriteAllText($Entry, $EntryText, (New-Object Text.UTF8Encoding($false)))
    $DuplicateOutput = Invoke-RdpEntry `
        -Arguments @('.rdp', 'create', '--force') `
        -ExpectedExitCode 1
    if (-not $DuplicateOutput.Contains('Duplicate embedded RDP property: full address')) {
        throw "Duplicate properties should fail explicitly.`n$DuplicateOutput"
    }

    $ExtraArgumentOutput = Invoke-RdpEntry `
        -Arguments @('.rdp', 'unexpected') `
        -ExpectedExitCode 1
    if (-not $ExtraArgumentOutput.Contains('RDP file usage:') -or
        -not $ExtraArgumentOutput.Contains("$EntryName .rdp create [--force]")) {
        throw "The .rdp command should reject extra arguments clearly.`n$ExtraArgumentOutput"
    }

    foreach ($RemovedArguments in @(
        [string[]]@('.rdp'),
        [string[]]@('.rdp', '--force')
    )) {
        $RemovedRdpOutput = Invoke-RdpEntry `
            -Arguments $RemovedArguments `
            -ExpectedExitCode 1
        if (-not $RemovedRdpOutput.Contains("$EntryName .rdp create [--force]")) {
            throw "A removed .rdp form should show the new usage.`n$RemovedRdpOutput"
        }
    }

    $RemovedConnectOutput = Invoke-RdpEntry `
        -Arguments @('.connect') `
        -ExpectedExitCode 1
    if (-not $RemovedConnectOutput.Contains('Unknown RDP command: .connect')) {
        throw "The removed .connect alias should fail as an unknown command.`n$RemovedConnectOutput"
    }

    Write-Host 'rdp client connection tests: PASS' -ForegroundColor Green
} finally {
    if ([IO.File]::Exists($Entry)) {
        [IO.File]::Delete($Entry)
    }
    if ([IO.File]::Exists($OutputPath)) {
        [IO.File]::Delete($OutputPath)
    }
    if ([IO.File]::Exists($CustomOutputPath)) {
        [IO.File]::Delete($CustomOutputPath)
    }
    if ([IO.Directory]::Exists($CacheDirectory)) {
        [IO.Directory]::Delete($CacheDirectory, $true)
    }
}
