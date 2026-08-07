[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjEntrySmoke {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Assertion failed: $Message"
    }
}

function Invoke-ProjEntrySmoke {
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
    [pscustomobject]@{
        ExitCode = [int]$ExitCode
        Text = [string]::Join(
            [Environment]::NewLine,
            [string[]]@($Output | ForEach-Object { [string]$_ })
        )
    }
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
. (Join-Path $RepoRoot '_lib\proj\_toolchain\bootstrap-layout.ps1')
$SourceEntry = (Get-ProjBootstrapLayout).LauncherCandidatePath
$EntryName = "test-native-entry-$([Guid]::NewGuid().ToString('N'))"
$EntryPath = Join-Path $RepoRoot "$EntryName.exe"
$DataRoot = Join-Path $RepoRoot "data\proj.$EntryName"
$PoisonedEnvironment = [ordered]@{
    SWAWKIT_HOME = 'C:\foreign-home'
    SWAWKIT_PROJ_PROTOCOL = 'foreign'
    SWAWKIT_PROJ_TARGET_PROJECT_ROOT = 'C:\foreign-project'
    SWAWKIT_PROJ_ACTION_ROOT = 'C:\foreign-project\.swaw'
    SWAWKIT_PROJ_DATA_ROOT = 'C:\foreign-data'
    SWAWKIT_PROJ_ENTRY_COMMAND = 'foreign-entry'
    SWAWKIT_PROJ_ENTRY_FILE = 'C:\foreign-entry.cmd'
    SWAWKIT_PROJ_LAUNCH_MODE = 'internal-host'
    SWAWKIT_PROJ_INTERNAL_PS_ARGC = '99'
    SWAWKIT_PROJ_INTERNAL_PS_ARG_47 = 'foreign-argument'
    SWAWKIT_PROJ_INTERNAL_CMD_ENTRY_PATH = 'C:\foreign-run.cmd'
}
$SavedEnvironment = @{}

if (-not [IO.File]::Exists($SourceEntry)) {
    & (Join-Path $RepoRoot '_lib\proj\build.ps1')
}
[IO.File]::Copy($SourceEntry, $EntryPath, $false)
try {
    foreach ($Name in $PoisonedEnvironment.Keys) {
        $SavedEnvironment[$Name] = [Environment]::GetEnvironmentVariable(
            $Name,
            [EnvironmentVariableTarget]::Process
        )
        [Environment]::SetEnvironmentVariable(
            $Name,
            [string]$PoisonedEnvironment[$Name],
            [EnvironmentVariableTarget]::Process
        )
    }

    $Missing = Invoke-ProjEntrySmoke `
        -EntryPath $EntryPath `
        -Arguments @('..entry', '--json')
    Assert-ProjEntrySmoke `
        -Condition ($Missing.ExitCode -eq 0) `
        -Message "..entry --json failed: $($Missing.Text)"
    $MissingDocument = $Missing.Text | ConvertFrom-Json
    Assert-ProjEntrySmoke `
        -Condition ($MissingDocument.status -ceq 'setupRequired') `
        -Message 'a fresh Entry did not report setupRequired'

    $Saved = Invoke-ProjEntrySmoke `
        -EntryPath $EntryPath `
        -Arguments @(
            '..entry.env.project.SWAWKIT_PROJ_TARGET_PROJECT_ROOT',
            '${SWAWKIT_HOME}'
        )
    Assert-ProjEntrySmoke `
        -Condition ($Saved.ExitCode -eq 0) `
        -Message "..entry.env failed: $($Saved.Text)"
    $SavedDocument = $Saved.Text | ConvertFrom-Json
    Assert-ProjEntrySmoke `
        -Condition (
            $SavedDocument.status -ceq 'ready' -and
            $SavedDocument.profile.targetProjectRoot -ceq '${SWAWKIT_HOME}'
        ) `
        -Message 'the saved Entry Profile is not ready'

    $Help = Invoke-ProjEntrySmoke `
        -EntryPath $EntryPath `
        -Arguments @('--help')
    Assert-ProjEntrySmoke `
        -Condition (
            $Help.ExitCode -eq 0 -and
            $Help.Text.Contains('Control Plane:') -and
            $Help.Text.Contains("$EntryName ..entry")
        ) `
        -Message "root help did not expose Control Plane: $($Help.Text)"

    $DotHelp = Invoke-ProjEntrySmoke `
        -EntryPath $EntryPath `
        -Arguments @('.help')
    Assert-ProjEntrySmoke `
        -Condition (
            $DotHelp.ExitCode -eq 0 -and
            $DotHelp.Text.Contains('Control Plane:')
        ) `
        -Message ".help leaked into its fail-closed adapter: $($DotHelp.Text)"

    $LegacyWeb = Invoke-ProjEntrySmoke `
        -EntryPath $EntryPath `
        -Arguments @('.web')
    Assert-ProjEntrySmoke `
        -Condition (
            $LegacyWeb.ExitCode -eq 1 -and
            $LegacyWeb.Text.Contains('command not found: .web')
        ) `
        -Message '.web remained a public command after the ..web migration'

    Assert-ProjEntrySmoke `
        -Condition ([IO.File]::Exists((Join-Path $DataRoot '_profile.json'))) `
        -Message 'the native Entry did not publish its Profile'
} finally {
    foreach ($Name in $PoisonedEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            $SavedEnvironment[$Name],
            [EnvironmentVariableTarget]::Process
        )
    }
    if ([IO.File]::Exists($EntryPath)) {
        [IO.File]::Delete($EntryPath)
    }
    if ([IO.Directory]::Exists($DataRoot)) {
        [IO.Directory]::Delete($DataRoot, $true)
    }
}

Write-Host '[PASS] Native Proj Entry smoke test' -ForegroundColor Green
$global:LASTEXITCODE = 0
