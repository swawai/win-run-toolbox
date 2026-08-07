[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjShellTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Assertion failed: $Message"
    }
}

function Invoke-ProjShellTest {
    param(
        [Parameter(Mandatory = $true)][string]$Address,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [string[]]$InputLines
    )

    $OwnedEnvironment = @{}
    $ProcessEnvironment = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    foreach ($Name in [string[]]@($ProcessEnvironment.Keys)) {
        if ($Name.StartsWith(
            'SWAWKIT_PROJ_DEV_',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            $OwnedEnvironment[$Name] = [string]$ProcessEnvironment[$Name]
            [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
        }
    }

    $PreviousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Output = @($InputLines | & $script:ProjShellEntry $Address 2>&1)
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousErrorActionPreference
        foreach ($Pair in $OwnedEnvironment.GetEnumerator()) {
            [Environment]::SetEnvironmentVariable(
                [string]$Pair.Key,
                [string]$Pair.Value,
                [EnvironmentVariableTarget]::Process
            )
        }
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
$BootstrapRoot = Join-Path $RepoRoot '_lib\proj\_bootstrap'
. (Join-Path $BootstrapRoot '_lib\layout.ps1')
$SourceEntry = (Get-ProjBootstrapLayout).LauncherCandidatePath
$EntryName = "test-shell-$([Guid]::NewGuid().ToString('N'))"
$script:ProjShellEntry = Join-Path $RepoRoot "$EntryName.exe"
$RuntimeBin = Join-Path $RepoRoot '_lib\proj\_bin'
$DataRoot = Join-Path $RepoRoot "data\proj.$EntryName"
$UserPathBefore = [Environment]::GetEnvironmentVariable('PATH', 'User')
$MachinePathBefore = [Environment]::GetEnvironmentVariable('PATH', 'Machine')
if (-not [IO.File]::Exists($SourceEntry)) {
    & (Join-Path $RepoRoot '_lib\proj\_launcher\build.ps1')
}
[IO.File]::Copy($SourceEntry, $script:ProjShellEntry, $false)

try {
$SetupOutput = @(
    & $script:ProjShellEntry `
        '..entry.env.project.SWAWKIT_PROJ_TARGET_PROJECT_ROOT' `
        '${SWAWKIT_HOME}' `
        2>&1
)
Assert-ProjShellTest `
    -Condition ($LASTEXITCODE -eq 0) `
    -Message "Entry Profile setup failed: $SetupOutput"
$ModeVariables = [ordered]@{
    bun = 'SWAWKIT_PROJ_BUN_MODE'
    pwsh = 'SWAWKIT_PROJ_PWSH_MODE'
    msvc = 'SWAWKIT_PROJ_MSVC_MODE'
    rust = 'SWAWKIT_PROJ_RUST_MODE'
}
foreach ($Group in $ModeVariables.Keys) {
    $ModeVariable = $ModeVariables[$Group]
    $ModeOutput = @(
        & $script:ProjShellEntry `
            "..entry.env.$Group.$ModeVariable" `
            'disabled' `
            2>&1
    )
    Assert-ProjShellTest `
        -Condition ($LASTEXITCODE -eq 0) `
        -Message "Entry Profile mode setup failed for $ModeVariable`: $ModeOutput"
}

$Cmd = Invoke-ProjShellTest `
    -Address '.cmd' `
    -InputLines @(
        'echo SHELL_KIND=cmd'
        'echo ENTRY_NAME=%SWAWKIT_PROJ_ENTRY_COMMAND%'
        'echo COMMAND_ADDRESS=%SWAWKIT_PROJ_COMMAND_ADDRESS%'
        'echo PROJ_HOME=%SWAWKIT_HOME%'
        'echo DATA_ROOT=%SWAWKIT_PROJ_DATA_ROOT%'
        'echo RUNTIME_BIN=%SWAWKIT_PROJ_RUNTIME_BIN%'
        'echo PATH_VALUE=%PATH%'
        'echo WORKING_DIR=%CD%'
        'exit 31'
    )
Assert-ProjShellTest `
    -Condition ($Cmd.ExitCode -eq 31) `
    -Message ".cmd did not return the child shell exit code: $($Cmd.Text)"
foreach ($Expected in @(
    'SHELL_KIND=cmd',
    "ENTRY_NAME=$EntryName",
    'COMMAND_ADDRESS=.cmd',
    "PROJ_HOME=$RepoRoot",
    "DATA_ROOT=$DataRoot",
    "RUNTIME_BIN=$RuntimeBin",
    "PATH_VALUE=$RuntimeBin;",
    "WORKING_DIR=$RepoRoot"
)) {
    Assert-ProjShellTest `
        -Condition ($Cmd.Text.IndexOf(
            $Expected,
            [StringComparison]::OrdinalIgnoreCase
        ) -ge 0) `
        -Message ".cmd did not inherit '$Expected': $($Cmd.Text)"
}

$PowerShell = Invoke-ProjShellTest `
    -Address '.ps' `
    -InputLines @(
        'Write-Output "SHELL_KIND=ps"'
        'Write-Output "PS_MAJOR=$($PSVersionTable.PSVersion.Major)"'
        'Write-Output "PS_HOME=$PSHOME"'
        'Write-Output "ENTRY_NAME=$env:SWAWKIT_PROJ_ENTRY_COMMAND"'
        'Write-Output "COMMAND_ADDRESS=$env:SWAWKIT_PROJ_COMMAND_ADDRESS"'
        'Write-Output "PROJ_HOME=$env:SWAWKIT_HOME"'
        'Write-Output "DATA_ROOT=$env:SWAWKIT_PROJ_DATA_ROOT"'
        'Write-Output "RUNTIME_BIN=$env:SWAWKIT_PROJ_RUNTIME_BIN"'
        'Write-Output "PATH_VALUE=$env:PATH"'
        'Write-Output "WORKING_DIR=$((Get-Location).ProviderPath)"'
        'exit 32'
    )
$ExpectedPsHome = Join-Path $env:SystemRoot (
    'System32\WindowsPowerShell\v1.0'
)
Assert-ProjShellTest `
    -Condition ($PowerShell.ExitCode -eq 32) `
    -Message ".ps did not return the child shell exit code: $($PowerShell.Text)"
foreach ($Expected in @(
    'SHELL_KIND=ps',
    'PS_MAJOR=5',
    "PS_HOME=$ExpectedPsHome",
    "ENTRY_NAME=$EntryName",
    'COMMAND_ADDRESS=.ps',
    "PROJ_HOME=$RepoRoot",
    "DATA_ROOT=$DataRoot",
    "RUNTIME_BIN=$RuntimeBin",
    "PATH_VALUE=$RuntimeBin;",
    "WORKING_DIR=$RepoRoot"
)) {
    Assert-ProjShellTest `
        -Condition ($PowerShell.Text.IndexOf(
            $Expected,
            [StringComparison]::OrdinalIgnoreCase
        ) -ge 0) `
        -Message ".ps did not inherit '$Expected': $($PowerShell.Text)"
}

foreach ($Address in @('.cmd', '.ps')) {
    $PreviousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $script:ProjShellEntry $Address 'unexpected' 2>&1 | Out-Null
        $TailExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousErrorActionPreference
    }
    Assert-ProjShellTest `
        -Condition ($TailExitCode -eq 1) `
        -Message "$Address accepted a dynamic tail argument"
}

Assert-ProjShellTest `
    -Condition (
        [Environment]::GetEnvironmentVariable('PATH', 'User') -ceq
            $UserPathBefore -and
        [Environment]::GetEnvironmentVariable('PATH', 'Machine') -ceq
            $MachinePathBefore
    ) `
    -Message 'interactive project shells changed persistent PATH'
} finally {
    if ([IO.File]::Exists($script:ProjShellEntry)) {
        [IO.File]::Delete($script:ProjShellEntry)
    }
    if ([IO.Directory]::Exists($DataRoot)) {
        [IO.Directory]::Delete($DataRoot, $true)
    }
}

Write-Host '[PASS] Proj interactive shell commands' -ForegroundColor Green
$global:LASTEXITCODE = 0
