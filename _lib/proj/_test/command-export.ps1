[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjCommandExportTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "Command export test failed: $Message"
    }
}

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\_lib\runtime.ps1')
$TemporaryBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TemporaryBase)
$TemporaryRoot = Join-Path $TemporaryBase (
    "swawkit-command-export-$([Guid]::NewGuid().ToString('N'))"
)
$ModulesJunction = ''

try {
    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    $DataRoot = Join-Path $TemporaryRoot 'data'
    $CacheRoot = Join-Path $TemporaryRoot 'cache'
    [void][IO.Directory]::CreateDirectory($ProjectRoot)
    [void][IO.Directory]::CreateDirectory($DataRoot)
    $Context = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $DataRoot `
        -CacheDataRoot $CacheRoot `
        -EntryCommand 'fixture'
    $ExpectedCommandRoot = Join-Path $DataRoot 'modules\kernel\.dev\setup'
    $ExpectedExport = Join-Path $ExpectedCommandRoot 'export'
    Assert-ProjCommandExportTest `
        -Condition (
            (Get-ProjDevCanonicalPath -Path $Context.SetupCommandRoot) -ceq
            (Get-ProjDevCanonicalPath -Path $ExpectedCommandRoot) -and
            (Get-ProjDevCanonicalPath -Path $Context.EnvironmentRoot) -ceq
            (Get-ProjDevCanonicalPath -Path $ExpectedExport) -and
            (Get-ProjDevCanonicalPath -Path $Context.ProviderStatePath) -ceq
            (Get-ProjDevCanonicalPath -Path (
                Join-Path $ExpectedCommandRoot '_state.json'
            )) -and
            (Get-ProjDevCanonicalPath -Path $Context.ProviderStateLockPath) -ceq
            (Get-ProjDevCanonicalPath -Path (
                Join-Path $ExpectedCommandRoot 'locks\state.lock'
            ))
        ) `
        -Message 'provider paths do not follow the command-data layout'
    Assert-ProjCommandExportTest `
        -Condition (-not [IO.Directory]::Exists($ExpectedCommandRoot)) `
        -Message 'resolving provider paths created directories'

    $ExpectedActionRoot = Join-Path $DataRoot 'modules\action\proj\build\app'
    $ActionRoot = Get-ProjActionCommandDataRoot `
        -DataRoot $DataRoot `
        -Address 'proj.build.app'
    $ActionExport = Resolve-ProjCommandExportPath `
        -DataRoot $DataRoot `
        -ProviderAddress 'proj.build.app' `
        -ProviderSource action
    Assert-ProjCommandExportTest `
        -Condition (
            (Get-ProjDevCanonicalPath -Path $ActionRoot) -ceq
            (Get-ProjDevCanonicalPath -Path $ExpectedActionRoot) -and
            (Get-ProjDevCanonicalPath -Path $ActionExport) -ceq
            (Get-ProjDevCanonicalPath -Path (
                Join-Path $ExpectedActionRoot 'export'
            ))
        ) `
        -Message 'Action provider paths do not follow the command-data layout'

    foreach ($Address in @(
        '', '.Dev.setup', '.dev..setup', '.dev/setup', '.dev\setup',
        '..entry', 'build'
    )) {
        $Rejected = $false
        try {
            [void](Resolve-ProjCommandExportPath `
                -DataRoot $DataRoot `
                -ProviderAddress $Address)
        } catch {
            $Rejected = $true
        }
        Assert-ProjCommandExportTest `
            -Condition $Rejected `
            -Message "unsafe provider address was accepted: '$Address'"
    }

    foreach ($Address in @(
        '', 'Proj.build', 'proj..build', '.dev.setup', 'proj/build',
        'proj\build', '..entry'
    )) {
        $Rejected = $false
        try {
            [void](Resolve-ProjCommandExportPath `
                -DataRoot $DataRoot `
                -ProviderAddress $Address `
                -ProviderSource action)
        } catch {
            $Rejected = $true
        }
        Assert-ProjCommandExportTest `
            -Condition $Rejected `
            -Message "unsafe Action provider address was accepted: '$Address'"
    }

    $ExternalRoot = Join-Path $TemporaryRoot 'external'
    $ReparseDataRoot = Join-Path $TemporaryRoot 'reparse-data'
    [void][IO.Directory]::CreateDirectory($ExternalRoot)
    [void][IO.Directory]::CreateDirectory($ReparseDataRoot)
    $ExternalSentinel = Join-Path $ExternalRoot 'sentinel.txt'
    [IO.File]::WriteAllText($ExternalSentinel, 'outside')
    $ModulesJunction = Join-Path $ReparseDataRoot 'modules'
    [void](New-Item `
        -ItemType Junction `
        -Path $ModulesJunction `
        -Target $ExternalRoot)
    try {
        $ReparseRejected = $false
        try {
            [void](Resolve-ProjCommandExportPath `
                -DataRoot $ReparseDataRoot `
                -ProviderAddress '.dev.setup')
        } catch {
            $ReparseRejected = $_.Exception.Message -like '*reparse point*'
        }
        Assert-ProjCommandExportTest `
            -Condition $ReparseRejected `
            -Message 'an intermediate command-data junction was accepted'
        Assert-ProjCommandExportTest `
            -Condition (
                [IO.File]::ReadAllText($ExternalSentinel) -ceq 'outside'
            ) `
            -Message 'an external junction target was modified'
    } finally {
        if ([IO.Directory]::Exists($ModulesJunction)) {
            [IO.Directory]::Delete($ModulesJunction)
        }
        $ModulesJunction = ''
    }

    Write-Host '[PASS] Proj command export layout test' `
        -ForegroundColor Green
} finally {
    if (-not [string]::IsNullOrWhiteSpace($ModulesJunction) -and
        [IO.Directory]::Exists($ModulesJunction)) {
        [IO.Directory]::Delete($ModulesJunction)
    }
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        Remove-Item -LiteralPath $TemporaryRoot -Recurse -Force
    }
}

$global:LASTEXITCODE = 0
