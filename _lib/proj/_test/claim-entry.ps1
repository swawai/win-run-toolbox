[CmdletBinding()]
param(
    [string]$LauncherPath = '',
    [string]$CorePath = '',
    [string]$HostPath = '',
    [string]$ToolchainPath = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjClaimEntry {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Assertion failed: $Message"
    }
}

function Invoke-ProjClaimEntry {
    param(
        [Parameter(Mandatory = $true)][string]$EntryPath,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )
    $PreviousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Output = @(& $EntryPath @Arguments 2>&1)
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousPreference
    }
    return [pscustomobject]@{
        ExitCode = [int]$ExitCode
        Text = [string]::Join(
            [Environment]::NewLine,
            [string[]]@($Output | ForEach-Object { [string]$_ })
        )
    }
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
. (Join-Path $PSScriptRoot '_lib\runtime-fixture.ps1')
$Artifacts = Resolve-ProjCandidateRuntimeArtifacts `
    -LauncherPath $LauncherPath `
    -CorePath $CorePath `
    -HostPath $HostPath `
    -ToolchainPath $ToolchainPath
$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-proj-claim-$([Guid]::NewGuid().ToString('N'))"
)
$EntryName = "test-claim-$([Guid]::NewGuid().ToString('N'))"

try {
    $Runtime = New-ProjCandidateRuntimeFixture `
        -RuntimeHome (Join-Path $TemporaryRoot 'runtime-home') `
        -LauncherPath $Artifacts.LauncherPath `
        -CorePath $Artifacts.CorePath `
        -HostPath $Artifacts.HostPath `
        -ToolchainPath $Artifacts.ToolchainPath
    $EntryPath = Add-ProjCandidateRuntimeEntry `
        -Runtime $Runtime `
        -RelativePath "$EntryName.exe"
    $DataRoot = Join-Path $Runtime.Home "data\proj.$EntryName"
    $RecordPath = Join-Path $DataRoot '_entry.json'

    $Setup = Invoke-ProjClaimEntry `
        -EntryPath $EntryPath `
        -Arguments @(
            '..entry.project.root',
            '${SWAWKIT_HOME}'
        )
    Assert-ProjClaimEntry `
        -Condition ($Setup.ExitCode -eq 0) `
        -Message "initial Entry setup failed: $($Setup.Text)"
    $Before = [IO.File]::ReadAllBytes($RecordPath)

    [IO.File]::Delete($EntryPath)
    [IO.File]::Copy($Artifacts.LauncherPath, $EntryPath, $false)

    $Stopwatch = [Diagnostics.Stopwatch]::StartNew()
    $Rejected = Invoke-ProjClaimEntry -EntryPath $EntryPath -Arguments @('--help')
    $Stopwatch.Stop()
    Assert-ProjClaimEntry `
        -Condition (
            $Rejected.ExitCode -eq 1 -and
            $Stopwatch.Elapsed.TotalSeconds -lt 5 -and
            $Rejected.Text.Contains("$EntryName ..entry.claim") -and
            $Rejected.Text.Contains("$EntryName ..entry.claim --yes")
        ) `
        -Message "ordinary CLI did not reject immediately with claim guidance: $($Rejected.Text)"
    Assert-ProjClaimEntry `
        -Condition ([Linq.Enumerable]::SequenceEqual(
            [byte[]]$Before,
            [byte[]][IO.File]::ReadAllBytes($RecordPath)
        )) `
        -Message 'ordinary CLI changed the Entry identity record'

    $Preview = Invoke-ProjClaimEntry `
        -EntryPath $EntryPath `
        -Arguments @('..entry.claim', '--json')
    Assert-ProjClaimEntry `
        -Condition ($Preview.ExitCode -eq 0) `
        -Message "claim preview failed: $($Preview.Text)"
    $Document = $Preview.Text | ConvertFrom-Json
    Assert-ProjClaimEntry `
        -Condition (
            $Document.status -ceq 'claimRequired' -and
            $Document.claim.entryName -ceq $EntryName
        ) `
        -Message 'claim preview did not expose the pending Entry'
    Assert-ProjClaimEntry `
        -Condition ([Linq.Enumerable]::SequenceEqual(
            [byte[]]$Before,
            [byte[]][IO.File]::ReadAllBytes($RecordPath)
        )) `
        -Message 'claim preview changed the Entry identity record'

    $ClaimHelp = Invoke-ProjClaimEntry `
        -EntryPath $EntryPath `
        -Arguments @('..entry.claim', '--help')
    Assert-ProjClaimEntry `
        -Condition (
            $ClaimHelp.ExitCode -eq 0 -and
            $ClaimHelp.Text.Contains('..entry.claim --yes')
        ) `
        -Message "claim help was unavailable before ownership: $($ClaimHelp.Text)"

    $Applied = Invoke-ProjClaimEntry `
        -EntryPath $EntryPath `
        -Arguments @('..entry.claim', '--yes')
    Assert-ProjClaimEntry `
        -Condition ($Applied.ExitCode -eq 0 -and $Applied.Text.Contains('Status: claimed')) `
        -Message "explicit claim failed: $($Applied.Text)"
    Assert-ProjClaimEntry `
        -Condition (-not [Linq.Enumerable]::SequenceEqual(
            [byte[]]$Before,
            [byte[]][IO.File]::ReadAllBytes($RecordPath)
        )) `
        -Message 'explicit claim did not update the Entry identity record'

    $Help = Invoke-ProjClaimEntry -EntryPath $EntryPath -Arguments @('--help')
    Assert-ProjClaimEntry `
        -Condition ($Help.ExitCode -eq 0 -and $Help.Text.Contains("${EntryName}:")) `
        -Message "claimed Entry did not resume normal CLI operation: $($Help.Text)"
} finally {
    Remove-ProjCandidateRuntimeFixture -Path $TemporaryRoot
}

Write-Host '[PASS] Native Proj Entry explicit claim flow' -ForegroundColor Green
$global:LASTEXITCODE = 0
