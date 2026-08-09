[CmdletBinding()]
param(
    [string]$LauncherPath = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjLauncherRuntimeTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Assertion failed: $Message"
    }
}

function Invoke-ProjLauncherRuntimeProcess {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Collections.IDictionary]$EnvironmentVariables = @{}
    )

    $StartInfo = [Diagnostics.ProcessStartInfo]::new()
    $StartInfo.FileName = $Executable
    $StartInfo.Arguments = $Arguments
    $StartInfo.WorkingDirectory = $WorkingDirectory
    $StartInfo.UseShellExecute = $false
    # Windows PowerShell lazily materializes this collection.
    [void]$StartInfo.EnvironmentVariables
    $StartInfo.CreateNoWindow = $true
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true
    foreach ($Pair in $EnvironmentVariables.GetEnumerator()) {
        $StartInfo.EnvironmentVariables[[string]$Pair.Key] = [string]$Pair.Value
    }
    $Process = [Diagnostics.Process]::new()
    $Process.StartInfo = $StartInfo
    try {
        if (-not $Process.Start()) {
            throw "Launcher process did not start: $Executable"
        }
        $StandardOutput = $Process.StandardOutput.ReadToEnd()
        $StandardError = $Process.StandardError.ReadToEnd()
        $Process.WaitForExit()
        return [pscustomobject][ordered]@{
            ExitCode = [int]$Process.ExitCode
            StandardOutput = $StandardOutput
            StandardError = $StandardError
        }
    } finally {
        $Process.Dispose()
    }
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
if ([string]::IsNullOrWhiteSpace($LauncherPath)) {
    . (Join-Path $RepoRoot (
        '_lib\proj\_toolchain\bootstrap-layout.ps1'
    ))
    $LauncherPath = (Get-ProjBootstrapLayout).LauncherCandidatePath
    if (-not [IO.File]::Exists($LauncherPath)) {
        & (Join-Path $RepoRoot '_lib\proj\build.ps1')
    }
}
$LauncherPath = [IO.Path]::GetFullPath($LauncherPath)
$CorePath = Join-Path $RepoRoot '_lib\proj\_bin\swawkit-proj.exe'
if (-not [IO.File]::Exists($CorePath)) {
    & (Join-Path $RepoRoot '_lib\proj\bootstrap.ps1')
}
foreach ($RequiredFile in @($LauncherPath, $CorePath)) {
    if (-not [IO.File]::Exists($RequiredFile)) {
        throw "Required built executable does not exist: $RequiredFile"
    }
}

$EntryName = "test-launcher-$([Guid]::NewGuid().ToString('N'))"
$EntryPath = Join-Path $RepoRoot "Favorites\$EntryName.exe"
$DataRoot = Join-Path $RepoRoot "data\proj.$EntryName"
$RootEntryName = "test-root-launcher-$([Guid]::NewGuid().ToString('N'))"
$RootEntryPath = Join-Path $RepoRoot "$RootEntryName.exe"
$RootDataRoot = Join-Path $RepoRoot "data\proj.$RootEntryName"
$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-proj-launcher-$([Guid]::NewGuid().ToString('N'))"
)
$TargetRoot = Join-Path $TemporaryRoot 'target'
$ActionRoot = Join-Path $TargetRoot '.swaw'
$ProbeRoot = Join-Path $ActionRoot 'probe'
$InvocationRoot = Join-Path $TemporaryRoot 'invocation'
$CapturePath = Join-Path $TemporaryRoot 'capture.json'
$NestedRoot = Join-Path $TemporaryRoot 'nested\level'
$NestedEntry = Join-Path $NestedRoot 'outside-supported-layout.exe'
$BootstrapHome = Join-Path $TemporaryRoot 'bootstrap-home'
$BootstrapEntry = Join-Path $BootstrapHome 'bootstrap-entry.exe'
$BootstrapScript = Join-Path $BootstrapHome '_lib\proj\bootstrap.ps1'
$BootstrapCore = Join-Path $BootstrapHome (
    '_lib\proj\_bin\swawkit-proj.exe'
)
$BootstrapMarker = Join-Path $BootstrapHome 'bootstrap-ran.txt'

$PoisonedVariables = @(
    'SWAWKIT_HOME',
    'SWAWKIT_PROJ_PROTOCOL',
    'SWAWKIT_PROJ_TARGET_PROJECT_ROOT',
    'SWAWKIT_PROJ_ACTION_ROOT',
    'SWAWKIT_PROJ_DATA_ROOT',
    'SWAWKIT_PROJ_ENTRY_COMMAND',
    'SWAWKIT_PROJ_ENTRY_FILE',
    'SWAWKIT_PROJ_LAUNCH_MODE',
    'SWAWKIT_PROJ_COMMAND_PROTOCOL',
    'SWAWKIT_PROJ_COMMAND_DATA_ROOT',
    'SWAWKIT_PROJ_CORE_LAUNCH_ENTRY_FILE',
    'SWAWKIT_PROJ_CORE_LAUNCH_MODE',
    'SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL',
    'SWAWKIT_PROJ_CORE_COMMAND_ENTRY_FILE',
    'SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT',
    'SWAWKIT_PROJ_TEST_LAUNCHER_CAPTURE'
)
$SavedEnvironment = @{}
foreach ($Name in $PoisonedVariables) {
    $SavedEnvironment[$Name] = [Environment]::GetEnvironmentVariable(
        $Name,
        [EnvironmentVariableTarget]::Process
    )
}
[Environment]::SetEnvironmentVariable(
    'SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL',
    $null,
    [EnvironmentVariableTarget]::Process
)

try {
    foreach ($Directory in @(
        $ProbeRoot,
        $InvocationRoot,
        $NestedRoot,
        (Split-Path -Path $BootstrapScript -Parent)
    )) {
        [void][IO.Directory]::CreateDirectory($Directory)
    }
    [IO.File]::Copy($LauncherPath, $EntryPath, $false)
    [IO.File]::Copy($LauncherPath, $RootEntryPath, $false)
    [IO.File]::Copy($LauncherPath, $NestedEntry, $false)
    [IO.File]::Copy($LauncherPath, $BootstrapEntry, $false)

    $BootstrapFixture = @'
$ErrorActionPreference = 'Stop'
$HomeRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$RuntimePath = Join-Path $HomeRoot '_lib\proj\_bin\swawkit-proj.exe'
$CmdPath = Join-Path ([Environment]::SystemDirectory) 'cmd.exe'
[void][IO.Directory]::CreateDirectory((Split-Path -Path $RuntimePath -Parent))
[IO.File]::Copy($CmdPath, $RuntimePath, $false)
[IO.File]::WriteAllText(
    (Join-Path $HomeRoot 'bootstrap-ran.txt'),
    'ok',
    [Text.UTF8Encoding]::new($false)
)
'@
    [IO.File]::WriteAllText(
        $BootstrapScript,
        $BootstrapFixture,
        [Text.UTF8Encoding]::new($false)
    )

    $Bootstrapped = Invoke-ProjLauncherRuntimeProcess `
        -Executable $BootstrapEntry `
        -Arguments '/d /c exit 0' `
        -WorkingDirectory $InvocationRoot
    Assert-ProjLauncherRuntimeTest `
        -Condition (
            $Bootstrapped.ExitCode -eq 0 -and
            [IO.File]::Exists($BootstrapCore) -and
            [IO.File]::Exists($BootstrapMarker)
        ) `
        -Message (
            'Launcher did not Bootstrap a missing shared Core: ' +
            "exit=$($Bootstrapped.ExitCode); " +
            "core=$([IO.File]::Exists($BootstrapCore)); " +
            "marker=$([IO.File]::Exists($BootstrapMarker)); " +
            "stdout=$($Bootstrapped.StandardOutput); " +
            "stderr=$($Bootstrapped.StandardError)"
        )

    $Help = Invoke-ProjLauncherRuntimeProcess `
        -Executable $EntryPath `
        -Arguments '--help' `
        -WorkingDirectory $InvocationRoot
    Assert-ProjLauncherRuntimeTest `
        -Condition ($Help.ExitCode -eq 0) `
        -Message "native Launcher help failed: $($Help.StandardError)"
    Assert-ProjLauncherRuntimeTest `
        -Condition ($Help.StandardOutput.Contains($EntryName)) `
        -Message 'shared Core did not derive the copied Launcher entry name'
    Assert-ProjLauncherRuntimeTest `
        -Condition ([IO.File]::Exists((Join-Path $DataRoot '_entry.json'))) `
        -Message 'copied Launcher did not create its entry-owned DataRoot'

    $RootHelp = Invoke-ProjLauncherRuntimeProcess `
        -Executable $RootEntryPath `
        -Arguments '--help' `
        -WorkingDirectory $InvocationRoot
    Assert-ProjLauncherRuntimeTest `
        -Condition (
            $RootHelp.ExitCode -eq 0 -and
            $RootHelp.StandardOutput.Contains($RootEntryName) -and
            [IO.File]::Exists((Join-Path $RootDataRoot '_entry.json'))
        ) `
        -Message 'Launcher did not support the SWAWKIT_HOME root layout'

    $Profile = [ordered]@{
        schema = 'swawkit.entry-profile/v1'
        targetProjectRoot = $TargetRoot
        preferences = [ordered]@{
            defaultShell = 'pwsh'
            defaultIde = 'code'
            helpLanguage = ''
        }
        development = [ordered]@{
            bun = [ordered]@{
                mode = 'managed'; version = '1.2.15'; sha256 = ''
            }
            pwsh = [ordered]@{
                mode = 'managed'; version = 'latest'; sha256 = ''
            }
            msvc = [ordered]@{ mode = 'managed'; channel = '17' }
            rust = [ordered]@{
                mode = 'rustup'
                toolchain = 'stable'
                profile = 'minimal'
                host = 'x86_64-pc-windows-msvc'
            }
            uv = [ordered]@{
                mode = 'disabled'; version = '0.10.2'; sha256 = ''
            }
            python = [ordered]@{
                mode = 'disabled'; version = '3.13'; sha256 = ''
            }
            go = [ordered]@{
                mode = 'disabled'; version = ''; sha256 = ''
            }
            gh = [ordered]@{ mode = 'system' }
            vscode = [ordered]@{ mode = 'system' }
            cursor = [ordered]@{ mode = 'system' }
        }
        git = [ordered]@{ name = ''; email = ''; access = '' }
        repository = [ordered]@{ remote = '' }
    }
    [IO.File]::WriteAllText(
        (Join-Path $DataRoot '_profile.json'),
        (($Profile | ConvertTo-Json -Depth 8) + "`n"),
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::WriteAllText(
        (Join-Path $ProbeRoot 'run.ps1'),
        @'
$ErrorActionPreference = 'Stop'
$Payload = [ordered]@{
    arguments = [string[]]@($args)
    entryFile = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_ENTRY_FILE
    launchEntryFile = [string]$env:SWAWKIT_PROJ_CORE_LAUNCH_ENTRY_FILE
    legacyEntryFile = [string]$env:SWAWKIT_PROJ_ENTRY_FILE
    entryName = [string]$env:SWAWKIT_PROJ_ENTRY_COMMAND
    targetProjectRoot = [string]$env:SWAWKIT_PROJ_TARGET_PROJECT_ROOT
    dataRoot = [string]$env:SWAWKIT_PROJ_DATA_ROOT
    commandProtocol = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL
    commandDataRoot = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT
    legacyCommandProtocol = [string]$env:SWAWKIT_PROJ_COMMAND_PROTOCOL
    legacyCommandDataRoot = [string]$env:SWAWKIT_PROJ_COMMAND_DATA_ROOT
    invocationDirectory = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR
    launchMode = [string]$env:SWAWKIT_PROJ_CORE_LAUNCH_MODE
    legacyLaunchMode = [string]$env:SWAWKIT_PROJ_LAUNCH_MODE
    bunVersion = [string]$env:SWAWKIT_PROJ_BUN_VERSION
}
[IO.File]::WriteAllText(
    $env:SWAWKIT_PROJ_TEST_LAUNCHER_CAPTURE,
    (($Payload | ConvertTo-Json -Depth 5) + "`n"),
    [Text.UTF8Encoding]::new($false)
)
exit 37
'@,
        [Text.UTF8Encoding]::new($false)
    )

    $env:SWAWKIT_HOME = 'C:\foreign-home'
    $env:SWAWKIT_PROJ_PROTOCOL = 'foreign'
    $env:SWAWKIT_PROJ_TARGET_PROJECT_ROOT = 'C:\foreign-project'
    $env:SWAWKIT_PROJ_ACTION_ROOT = 'C:\foreign-project\.swaw'
    $env:SWAWKIT_PROJ_DATA_ROOT = 'C:\foreign-data'
    $env:SWAWKIT_PROJ_ENTRY_COMMAND = 'foreign-entry'
    $env:SWAWKIT_PROJ_ENTRY_FILE = 'C:\foreign-entry.exe'
    $env:SWAWKIT_PROJ_LAUNCH_MODE = 'internal-host'
    $env:SWAWKIT_PROJ_COMMAND_PROTOCOL = 'foreign'
    $env:SWAWKIT_PROJ_COMMAND_DATA_ROOT = 'C:\foreign-command-data'
    $env:SWAWKIT_PROJ_CORE_LAUNCH_ENTRY_FILE = 'C:\foreign-core-entry.exe'
    $env:SWAWKIT_PROJ_CORE_LAUNCH_MODE = 'internal-host'
    $env:SWAWKIT_PROJ_CORE_COMMAND_ENTRY_FILE = 'C:\foreign-command-entry.exe'
    $env:SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT = 'C:\foreign-core-command-data'
    $env:SWAWKIT_PROJ_TEST_LAUNCHER_CAPTURE = $CapturePath

    $Run = Invoke-ProjLauncherRuntimeProcess `
        -Executable $EntryPath `
        -Arguments 'probe "" "a&b|c" "你好 世界"' `
        -WorkingDirectory $InvocationRoot
    Assert-ProjLauncherRuntimeTest `
        -Condition ($Run.ExitCode -eq 37) `
        -Message (
            "Launcher did not return the exact Core exit code: " +
            "$($Run.ExitCode); stderr=$($Run.StandardError)"
        )
    $Capture = [IO.File]::ReadAllText($CapturePath) | ConvertFrom-Json
    Assert-ProjLauncherRuntimeTest `
        -Condition (
            @($Capture.arguments).Count -eq 3 -and
            [string]$Capture.arguments[0] -ceq '' -and
            [string]$Capture.arguments[1] -ceq 'a&b|c' -and
            [string]$Capture.arguments[2] -ceq '你好 世界'
        ) `
        -Message 'Launcher did not preserve empty, metacharacter, and Unicode argv'
    $Expectations = [ordered]@{
        entryFile = $EntryPath
        launchEntryFile = ''
        legacyEntryFile = ''
        entryName = $EntryName
        targetProjectRoot = $TargetRoot
        dataRoot = $DataRoot
        commandProtocol = '1'
        commandDataRoot = (Join-Path $DataRoot 'modules\action\probe')
        legacyCommandProtocol = ''
        legacyCommandDataRoot = ''
        invocationDirectory = $InvocationRoot
        launchMode = ''
        legacyLaunchMode = ''
        bunVersion = '1.2.15'
    }
    foreach ($Expectation in $Expectations.GetEnumerator()) {
        Assert-ProjLauncherRuntimeTest `
            -Condition (
                [string]$Capture.($Expectation.Key) -ceq
                    [string]$Expectation.Value
            ) `
            -Message (
                "unexpected $($Expectation.Key): " +
                "'$([string]$Capture.($Expectation.Key))'"
            )
    }

    foreach ($ProtocolValue in @('', 'foreign')) {
        $RejectedNestedEntry = Invoke-ProjLauncherRuntimeProcess `
            -Executable $EntryPath `
            -Arguments '--help' `
            -WorkingDirectory $InvocationRoot `
            -EnvironmentVariables @{
                SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL = $ProtocolValue
            }
        Assert-ProjLauncherRuntimeTest `
            -Condition (
                $RejectedNestedEntry.ExitCode -eq 1 -and
                $RejectedNestedEntry.StandardError.Contains(
                    'inside another Entry command'
                )
            ) `
            -Message (
                'Launcher did not reject nested Entry startup when the ' +
                'command protocol variable existed: ' +
                "value='$ProtocolValue'; " +
                "exit=$($RejectedNestedEntry.ExitCode); " +
                "stderr=$($RejectedNestedEntry.StandardError)"
            )
    }

    $RejectedLayout = Invoke-ProjLauncherRuntimeProcess `
        -Executable $NestedEntry `
        -Arguments '--help' `
        -WorkingDirectory $InvocationRoot
    Assert-ProjLauncherRuntimeTest `
        -Condition (
            $RejectedLayout.ExitCode -eq 1 -and
            $RejectedLayout.StandardError.Contains('Cannot locate')
        ) `
        -Message 'Launcher accepted a path deeper than the supported layout'
} finally {
    foreach ($Name in $PoisonedVariables) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            $SavedEnvironment[$Name],
            [EnvironmentVariableTarget]::Process
        )
    }
    if ([IO.File]::Exists($EntryPath)) {
        [IO.File]::Delete($EntryPath)
    }
    if ([IO.File]::Exists($RootEntryPath)) {
        [IO.File]::Delete($RootEntryPath)
    }
    if ([IO.Directory]::Exists($DataRoot) -and
        [IO.Path]::GetFileName($DataRoot) -ceq "proj.$EntryName") {
        [IO.Directory]::Delete($DataRoot, $true)
    }
    if ([IO.Directory]::Exists($RootDataRoot) -and
        [IO.Path]::GetFileName($RootDataRoot) -ceq "proj.$RootEntryName") {
        [IO.Directory]::Delete($RootDataRoot, $true)
    }
    if ([IO.Directory]::Exists($TemporaryRoot) -and
        $TemporaryRoot.StartsWith(
            (Join-Path $RepoRoot 'data\_test') + '\',
            [StringComparison]::OrdinalIgnoreCase
        )) {
        [IO.Directory]::Delete($TemporaryRoot, $true)
    }
}

Write-Host '[PASS] Proj native Launcher runtime' -ForegroundColor Green
$global:LASTEXITCODE = 0
