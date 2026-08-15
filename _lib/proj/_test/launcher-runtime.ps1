[CmdletBinding()]
param(
    [string]$LauncherPath = '',
    [string]$CorePath = '',
    [string]$HostPath = '',
    [string]$ToolchainPath = ''
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

. (Join-Path $PSScriptRoot '_lib\windows-job-fixture.ps1')
. (Join-Path $PSScriptRoot '_lib\runtime-fixture.ps1')

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
    # Windows PowerShell 5.1 can inherit duplicate-cased names such as Path
    # and PATH. Rebuild one case-insensitive child block before adding probes;
    # lazy materialization can otherwise silently drop an unrelated variable.
    $InheritedEnvironment = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    [void]$StartInfo.EnvironmentVariables
    $StartInfo.EnvironmentVariables.Clear()
    foreach ($Name in [string[]]@($InheritedEnvironment.Keys)) {
        $StartInfo.EnvironmentVariables[$Name] = [string]$InheritedEnvironment[$Name]
    }
    $StartInfo.CreateNoWindow = $true
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true
    $StartInfo.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $StartInfo.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
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
if ([string]::IsNullOrWhiteSpace($LauncherPath) -or
    [string]::IsNullOrWhiteSpace($CorePath) -or
    [string]::IsNullOrWhiteSpace($HostPath) -or
    [string]::IsNullOrWhiteSpace($ToolchainPath)) {
    . (Join-Path $RepoRoot (
        '_lib\proj\_toolchain\bootstrap-layout.ps1'
    ))
    $Layout = Get-ProjBootstrapLayout
    if ([string]::IsNullOrWhiteSpace($LauncherPath)) {
        $LauncherPath = $Layout.LauncherCandidatePath
    }
    if ([string]::IsNullOrWhiteSpace($CorePath)) {
        $CorePath = Join-Path $Layout.BuildRoot 'release\swawkit-proj.exe'
    }
    if ([string]::IsNullOrWhiteSpace($HostPath)) {
        $HostPath = Join-Path $Layout.BuildRoot 'release\swawkit-proj-host.exe'
    }
    if ([string]::IsNullOrWhiteSpace($ToolchainPath)) {
        $ToolchainPath = Join-Path $Layout.BuildRoot (
            'release\swawkit-proj-toolchain.exe'
        )
    }
    & (Join-Path $RepoRoot '_lib\proj\build.ps1') | Out-Host
}
$LauncherPath = [IO.Path]::GetFullPath($LauncherPath)
$CorePath = [IO.Path]::GetFullPath($CorePath)
$HostPath = [IO.Path]::GetFullPath($HostPath)
$ToolchainPath = [IO.Path]::GetFullPath($ToolchainPath)
foreach ($RequiredFile in @(
    $LauncherPath,
    $CorePath,
    $HostPath,
    $ToolchainPath
)) {
    if (-not [IO.File]::Exists($RequiredFile)) {
        throw "Required built executable does not exist: $RequiredFile"
    }
}

$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-proj-launcher-$([Guid]::NewGuid().ToString('N'))"
)
$RuntimeHome = Join-Path $TemporaryRoot 'runtime-home'
$RuntimeKernelRoot = Join-Path $RuntimeHome '_lib\proj'
$RuntimeReleaseId = 'a' * 64
$RuntimeBin = Join-Path $RuntimeKernelRoot '_bin'
$RuntimeRelease = Join-Path (
    Join-Path $RuntimeBin 'releases'
) $RuntimeReleaseId
$RuntimeCorePath = Join-Path $RuntimeRelease 'swawkit-proj.exe'
$RuntimeHostPath = Join-Path $RuntimeRelease 'swawkit-proj-host.exe'
$RuntimeToolchainPath = Join-Path $RuntimeRelease 'swawkit-proj-toolchain.exe'
$EntryName = "test-launcher-$([Guid]::NewGuid().ToString('N'))"
$EntryPath = Join-Path $RuntimeHome "Favorites\$EntryName.exe"
$DataRoot = Join-Path $RuntimeHome "data\proj.$EntryName"
$RootEntryName = "test-root-launcher-$([Guid]::NewGuid().ToString('N'))"
$RootEntryPath = Join-Path $RuntimeHome "$RootEntryName.exe"
$RootDataRoot = Join-Path $RuntimeHome "data\proj.$RootEntryName"
$TargetRoot = Join-Path $TemporaryRoot 'target'
$ActionRoot = Join-Path $TargetRoot '.swaw'
$ProbeRoot = Join-Path $ActionRoot 'probe'
$HangRoot = Join-Path $ActionRoot 'hang'
$InvocationRoot = Join-Path $TemporaryRoot 'invocation'
$CapturePath = Join-Path $DataRoot 'modules\action\probe\capture.json'
$HangMarkerPath = Join-Path $DataRoot 'modules\action\hang\started.txt'
$NestedRoot = Join-Path $TemporaryRoot 'nested\level'
$NestedEntry = Join-Path $NestedRoot 'outside-supported-layout.exe'
$BootstrapHome = Join-Path $TemporaryRoot 'bootstrap-home'
$BootstrapEntry = Join-Path $BootstrapHome 'bootstrap-entry.exe'
$BootstrapScript = Join-Path $BootstrapHome '_lib\proj\bootstrap.ps1'
$BootstrapCore = Join-Path $BootstrapHome (
    ('_lib\proj\_bin\releases\' + ('b' * 64) + '\swawkit-proj.exe')
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
    'SWAWKIT_PROJ_CORE_LAUNCH_PROTOCOL',
    'SWAWKIT_PROJ_CORE_LAUNCH_ENTRY_FILE',
    'SWAWKIT_PROJ_CORE_LAUNCH_MODE',
    'SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL',
    'SWAWKIT_PROJ_CORE_COMMAND_ENTRY_FILE',
    'SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT',
    'SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION',
    'SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION',
    'SWAWKIT_PROJ_BUN_VERSION',
    'SwAwKiT_PrOj_UnKnOwN',
    'swawkit_proj_module_kernel_dev_setup_inherited_test',
    'SwAwKiT_PrOj_CoRe_AdApTeR_PoWeRsHeLl_ArG_47',
    'sWaWkIt_pRoJ_CoRe_cOmMaNd_aDaPtEr_pWsH_ArG_47'
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
        (Split-Path -Path $RuntimeCorePath -Parent),
        (Join-Path $RuntimeKernelRoot '_help'),
        (Split-Path -Path $EntryPath -Parent),
        $ProbeRoot,
        $HangRoot,
        $InvocationRoot,
        $NestedRoot,
        (Split-Path -Path $BootstrapScript -Parent)
    )) {
        [void][IO.Directory]::CreateDirectory($Directory)
    }
    [IO.File]::Copy($CorePath, $RuntimeCorePath, $false)
    [IO.File]::Copy($HostPath, $RuntimeHostPath, $false)
    [IO.File]::Copy($ToolchainPath, $RuntimeToolchainPath, $false)
    [IO.File]::WriteAllText(
        (Join-Path $RuntimeBin 'current'),
        ($RuntimeReleaseId + "`n"),
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::Copy(
        (Join-Path $RepoRoot '_lib\proj\_help\zh-CN.txt'),
        (Join-Path $RuntimeKernelRoot '_help\zh-CN.txt'),
        $false
    )
    Copy-Item `
        -LiteralPath (Join-Path $RepoRoot '_lib\proj\.dev') `
        -Destination $RuntimeKernelRoot `
        -Recurse `
        -Force
    [IO.File]::Copy($LauncherPath, $EntryPath, $false)
    [IO.File]::Copy($LauncherPath, $RootEntryPath, $false)
    [IO.File]::Copy($LauncherPath, $NestedEntry, $false)
    [IO.File]::Copy($LauncherPath, $BootstrapEntry, $false)

    $BootstrapFixture = @'
$ErrorActionPreference = 'Stop'
$HomeRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$ReleaseId = 'b' * 64
$RuntimeRoot = Join-Path $HomeRoot '_lib\proj\_bin'
$RuntimePath = Join-Path (
    Join-Path (Join-Path $RuntimeRoot 'releases') $ReleaseId
) 'swawkit-proj.exe'
$CmdPath = Join-Path ([Environment]::SystemDirectory) 'cmd.exe'
[void][IO.Directory]::CreateDirectory((Split-Path -Path $RuntimePath -Parent))
[IO.File]::Copy($CmdPath, $RuntimePath, $false)
[IO.File]::WriteAllText(
    (Join-Path $RuntimeRoot 'current'),
    ($ReleaseId + "`n"),
    [Text.UTF8Encoding]::new($false)
)
[IO.File]::WriteAllText(
    (Join-Path $HomeRoot 'bootstrap-ran.txt'),
    ((@(
        [string]$env:SWAWKIT_PROJ_CORE_LAUNCH_WORKER_PROTOCOL
        [string]$env:SWAWKIT_PROJ_CORE_LAUNCH_WORKER_JOB_NAME
        [string]$env:SWAWKIT_PROJ_CORE_LAUNCH_WORKER_READY_EVENT_NAME
    )) -join '|'),
    [Text.UTF8Encoding]::new($false)
)
'@
    [IO.File]::WriteAllText(
        $BootstrapScript,
        $BootstrapFixture,
        [Text.UTF8Encoding]::new($false)
    )

    $Bootstrapped = Invoke-ProjLauncherJobTestProcess `
        -Executable $BootstrapEntry `
        -Arguments '/d /c exit 0' `
        -WorkingDirectory $InvocationRoot
    Assert-ProjLauncherRuntimeTest `
        -Condition (
            $Bootstrapped.ExitCode -eq 0 -and
            $Bootstrapped.InJob -and
            [IO.File]::Exists($BootstrapCore) -and
            [IO.File]::Exists($BootstrapMarker) -and
            [IO.File]::ReadAllText($BootstrapMarker) -ceq '||'
        ) `
        -Message (
            'Launcher did not Bootstrap a missing shared Core: ' +
            "exit=$($Bootstrapped.ExitCode); " +
            "job=$($Bootstrapped.InJob); " +
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
        schema = 'swawkit.entry-profile/v2'
        targetProjectRoot = $TargetRoot
        language = 'zh-CN'
        development = [ordered]@{
            bun = [ordered]@{
                mode = 'disabled'; version = '1.2.15'; sha256 = ''
            }
            pwsh = [ordered]@{
                mode = 'managed'; version = '7.6.4'; sha256 = ''
            }
            msvc = [ordered]@{ mode = 'disabled'; channel = '17' }
            rust = [ordered]@{
                mode = 'disabled'
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
    swawkitHome = [string]$env:SWAWKIT_HOME
    entryFile = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_ENTRY_FILE
    launchEntryFile = [string]$env:SWAWKIT_PROJ_CORE_LAUNCH_ENTRY_FILE
    launchProtocol = [string]$env:SWAWKIT_PROJ_CORE_LAUNCH_PROTOCOL
    workerProtocol = [string]$env:SWAWKIT_PROJ_CORE_LAUNCH_WORKER_PROTOCOL
    workerJobName = [string]$env:SWAWKIT_PROJ_CORE_LAUNCH_WORKER_JOB_NAME
    workerReadyEventName = [string]$env:SWAWKIT_PROJ_CORE_LAUNCH_WORKER_READY_EVENT_NAME
    legacyEntryFile = [string]$env:SWAWKIT_PROJ_ENTRY_FILE
    entryName = [string]$env:SWAWKIT_PROJ_ENTRY_COMMAND
    targetProjectRoot = [string]$env:SWAWKIT_PROJ_TARGET_PROJECT_ROOT
    actionRoot = [string]$env:SWAWKIT_PROJ_ACTION_ROOT
    dataRoot = [string]$env:SWAWKIT_PROJ_DATA_ROOT
    commandProtocol = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL
    commandDataRoot = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT
    legacyCommandProtocol = [string]$env:SWAWKIT_PROJ_COMMAND_PROTOCOL
    legacyCommandDataRoot = [string]$env:SWAWKIT_PROJ_COMMAND_DATA_ROOT
    invocationDirectory = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR
    launchMode = [string]$env:SWAWKIT_PROJ_CORE_LAUNCH_MODE
    legacyLaunchMode = [string]$env:SWAWKIT_PROJ_LAUNCH_MODE
    bunVersion = [string]$env:SWAWKIT_PROJ_BUN_VERSION
    environmentInputRevision = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION
    profileRevision = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION
    unknownState = [string]$env:SWAWKIT_PROJ_UNKNOWN
    moduleState = [string]$env:SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_INHERITED_TEST
    legacyAdapterState = [string]$env:SWAWKIT_PROJ_CORE_ADAPTER_POWERSHELL_ARG_47
    commandAdapterState = [string]$env:SWAWKIT_PROJ_CORE_COMMAND_ADAPTER_PWSH_ARG_47
}
$CapturePath = Join-Path $env:SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT 'capture.json'
[void][IO.Directory]::CreateDirectory((Split-Path -Path $CapturePath -Parent))
[IO.File]::WriteAllText(
    $CapturePath,
    (($Payload | ConvertTo-Json -Depth 5) + "`n"),
    [Text.UTF8Encoding]::new($false)
)
Write-Output 'worker-stdout-sentinel'
[Console]::Error.WriteLine('worker-stderr-sentinel')
exit 37
'@,
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::WriteAllText(
        (Join-Path $HangRoot 'run.ps1'),
        @'
$ErrorActionPreference = 'Stop'
$MarkerPath = Join-Path $env:SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT 'started.txt'
[void][IO.Directory]::CreateDirectory((Split-Path -Path $MarkerPath -Parent))
[IO.File]::WriteAllText(
    $MarkerPath,
    [string]$PID,
    [Text.UTF8Encoding]::new($false)
)
Start-Sleep -Seconds 60
'@,
        [Text.UTF8Encoding]::new($false)
    )

    $ManagedPwshSource = Join-Path $RepoRoot (
        'data\proj.swawkit\modules\kernel\.dev\setup\export\pwsh\installs\7.6.4'
    )
    Copy-ProjFixtureHardLinkTree `
        -Source $ManagedPwshSource `
        -Destination (Join-Path $DataRoot (
            'modules\kernel\.dev\setup\export\pwsh\installs\7.6.4'
        ))
    $DevelopmentSetup = Invoke-ProjLauncherRuntimeProcess `
        -Executable $EntryPath `
        -Arguments '.dev.setup' `
        -WorkingDirectory $InvocationRoot
    Assert-ProjLauncherRuntimeTest `
        -Condition ($DevelopmentSetup.ExitCode -eq 0) `
        -Message "native development setup failed: $($DevelopmentSetup.StandardError)"

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
    $env:SWAWKIT_PROJ_CORE_LAUNCH_PROTOCOL = 'foreign'
    $env:SWAWKIT_PROJ_CORE_LAUNCH_ENTRY_FILE = 'C:\foreign-core-entry.exe'
    $env:SWAWKIT_PROJ_CORE_LAUNCH_MODE = 'internal-host'
    $env:SWAWKIT_PROJ_CORE_COMMAND_ENTRY_FILE = 'C:\foreign-command-entry.exe'
    $env:SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT = 'C:\foreign-core-command-data'
    $env:SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION = 'foreign-input-revision'
    $env:SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION = 'foreign-profile-revision'
    $env:SWAWKIT_PROJ_BUN_VERSION = 'foreign-version'
    $env:SwAwKiT_PrOj_UnKnOwN = 'foreign-unknown'
    $env:swawkit_proj_module_kernel_dev_setup_inherited_test = 'foreign-module'
    $env:SwAwKiT_PrOj_CoRe_AdApTeR_PoWeRsHeLl_ArG_47 = 'foreign-legacy-adapter'
    $env:sWaWkIt_pRoJ_CoRe_cOmMaNd_aDaPtEr_pWsH_ArG_47 = 'foreign-command-adapter'

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
        swawkitHome = $RuntimeHome
        entryFile = $EntryPath
        launchEntryFile = ''
        launchProtocol = ''
        workerProtocol = ''
        workerJobName = ''
        workerReadyEventName = ''
        legacyEntryFile = ''
        entryName = $EntryName
        targetProjectRoot = $TargetRoot
        actionRoot = $ActionRoot
        dataRoot = $DataRoot
        commandProtocol = '1'
        commandDataRoot = (Join-Path $DataRoot 'modules\action\probe')
        legacyCommandProtocol = ''
        legacyCommandDataRoot = ''
        invocationDirectory = $InvocationRoot
        launchMode = ''
        legacyLaunchMode = ''
        bunVersion = '1.2.15'
        unknownState = ''
        moduleState = ''
        legacyAdapterState = ''
        commandAdapterState = ''
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
    foreach ($RevisionName in @(
        'environmentInputRevision',
        'profileRevision'
    )) {
        Assert-ProjLauncherRuntimeTest `
            -Condition (
                [string]$Capture.($RevisionName) -cmatch
                    '^sha256-[a-f0-9]{64}$'
            ) `
            -Message (
                "unexpected ${RevisionName}: " +
                "'$([string]$Capture.($RevisionName))'"
            )
    }

    $WorkerRun = Invoke-ProjLauncherJobTestProcess `
        -Executable $EntryPath `
        -Arguments 'probe worker-boundary' `
        -WorkingDirectory $InvocationRoot
    Assert-ProjLauncherRuntimeTest `
        -Condition (
            $WorkerRun.ExitCode -eq 37 -and
            $WorkerRun.InJob -and
            $WorkerRun.StandardOutput.Contains('worker-stdout-sentinel') -and
            $WorkerRun.StandardError.Contains('worker-stderr-sentinel')
        ) `
        -Message (
            'Launcher did not execute inside the declared Web worker job: ' +
            "exit=$($WorkerRun.ExitCode); job=$($WorkerRun.InJob); " +
            "stderr=$($WorkerRun.StandardError)"
        )
    $WorkerCapture = [IO.File]::ReadAllText($CapturePath) |
        ConvertFrom-Json
    Assert-ProjLauncherRuntimeTest `
        -Condition (
            @($WorkerCapture.arguments).Count -eq 1 -and
            [string]$WorkerCapture.arguments[0] -ceq 'worker-boundary' -and
            [string]::IsNullOrEmpty([string]$WorkerCapture.workerProtocol) -and
            [string]::IsNullOrEmpty([string]$WorkerCapture.workerJobName) -and
            [string]::IsNullOrEmpty(
                [string]$WorkerCapture.workerReadyEventName
            )
        ) `
        -Message 'Web worker launch declarations leaked into the command'

    $RejectedWorkerDeclaration = Invoke-ProjLauncherRuntimeProcess `
        -Executable $EntryPath `
        -Arguments '--help' `
        -WorkingDirectory $InvocationRoot `
        -EnvironmentVariables @{
            SWAWKIT_PROJ_CORE_LAUNCH_WORKER_PROTOCOL = '1'
        }
    Assert-ProjLauncherRuntimeTest `
        -Condition (
            $RejectedWorkerDeclaration.ExitCode -eq 1 -and
            $RejectedWorkerDeclaration.StandardError.Contains(
                'Web worker process boundary'
            )
        ) `
        -Message 'Launcher accepted an incomplete Web worker declaration'

    $WorkerIdentity = [Guid]::NewGuid().ToString('N')
    $WorkerJobName = "Local\SwawKit.Proj.Test.Job.$WorkerIdentity"
    $WorkerReadyName = "Local\SwawKit.Proj.Test.Ready.$WorkerIdentity"
    $WorkerJob = [IntPtr]::Zero
    $WorkerReady = [IntPtr]::Zero
    $WorkerProcess = $null
    $CommandProcess = $null
    try {
        $WorkerJob = [ProjLauncherJobTest]::CreateKillOnCloseJob(
            $WorkerJobName
        )
        $WorkerReady = [ProjLauncherJobTest]::CreateReadyEvent(
            $WorkerReadyName
        )
        $WorkerProcess = Start-ProjLauncherRuntimeProcess `
            -Executable $EntryPath `
            -Arguments 'hang' `
            -WorkingDirectory $InvocationRoot `
            -EnvironmentVariables @{
                SWAWKIT_PROJ_CORE_LAUNCH_WORKER_PROTOCOL = '1'
                SWAWKIT_PROJ_CORE_LAUNCH_WORKER_JOB_NAME = $WorkerJobName
                SWAWKIT_PROJ_CORE_LAUNCH_WORKER_READY_EVENT_NAME = $WorkerReadyName
            }
        $WorkerOutput = $WorkerProcess.StandardOutput.ReadToEndAsync()
        $WorkerError = $WorkerProcess.StandardError.ReadToEndAsync()
        Assert-ProjLauncherRuntimeTest `
            -Condition ([ProjLauncherJobTest]::WaitReady($WorkerReady, 5000)) `
            -Message 'cancellable Launcher did not signal worker readiness'
        $Deadline = [DateTime]::UtcNow.AddSeconds(5)
        while (-not [IO.File]::Exists($HangMarkerPath) -and
            [DateTime]::UtcNow -lt $Deadline) {
            Start-Sleep -Milliseconds 25
        }
        Assert-ProjLauncherRuntimeTest `
            -Condition ([IO.File]::Exists($HangMarkerPath)) `
            -Message 'cancellable command did not start its descendant process'
        $CommandProcess = [Diagnostics.Process]::GetProcessById(
            [int][IO.File]::ReadAllText($HangMarkerPath)
        )
        Assert-ProjLauncherRuntimeTest `
            -Condition ([ProjLauncherJobTest]::CloseHandle($WorkerJob)) `
            -Message 'test could not close the worker Job handle'
        $WorkerJob = [IntPtr]::Zero
        Assert-ProjLauncherRuntimeTest `
            -Condition (
                $WorkerProcess.WaitForExit(5000) -and
                $CommandProcess.WaitForExit(5000)
            ) `
            -Message 'closing the worker Job did not terminate the complete tree'
        [void]$WorkerOutput.Result
        [void]$WorkerError.Result
    } finally {
        if ($WorkerJob -ne [IntPtr]::Zero) {
            [void][ProjLauncherJobTest]::CloseHandle($WorkerJob)
        }
        if ($null -ne $WorkerProcess) {
            if (-not $WorkerProcess.HasExited) {
                $WorkerProcess.Kill()
                [void]$WorkerProcess.WaitForExit(5000)
            }
            $WorkerProcess.Dispose()
        }
        if ($null -ne $CommandProcess) {
            $CommandProcess.Dispose()
        }
        if ($WorkerReady -ne [IntPtr]::Zero) {
            [void][ProjLauncherJobTest]::CloseHandle($WorkerReady)
        }
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
