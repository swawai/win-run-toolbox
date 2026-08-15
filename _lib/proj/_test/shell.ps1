[CmdletBinding()]
param(
    [string]$LauncherPath = '',
    [string]$CorePath = '',
    [string]$HostPath = '',
    [string]$ToolchainPath = ''
)

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

function Set-ProjShellProcessPath {
    param([AllowEmptyString()][string]$Value)

    $Variables = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    foreach ($Name in @($Variables.Keys | Where-Object {
        [string]$_ -ieq 'PATH'
    })) {
        [Environment]::SetEnvironmentVariable(
            [string]$Name,
            $null,
            [EnvironmentVariableTarget]::Process
        )
    }
    [Environment]::SetEnvironmentVariable(
        'Path',
        $Value,
        [EnvironmentVariableTarget]::Process
    )
}

function Invoke-ProjShellTest {
    param(
        [Parameter(Mandatory = $true)][string]$Address,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [string[]]$Arguments
    )

    $OwnedEnvironment = @{}
    $ProcessEnvironment = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    foreach ($Name in [string[]]@($ProcessEnvironment.Keys)) {
        if ($Name.StartsWith(
            'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            $OwnedEnvironment[$Name] = [string]$ProcessEnvironment[$Name]
            [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
        }
    }

    $PreviousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Output = @(& $script:ProjShellEntry $Address @Arguments 2>&1)
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
. (Join-Path $PSScriptRoot '_lib\runtime-fixture.ps1')
$Artifacts = Resolve-ProjCandidateRuntimeArtifacts `
    -LauncherPath $LauncherPath `
    -CorePath $CorePath `
    -HostPath $HostPath `
    -ToolchainPath $ToolchainPath
$EntryName = "test-shell-$([Guid]::NewGuid().ToString('N'))"
$TestRoot = Join-Path $RepoRoot 'data\_test'
$TemporaryRoot = Join-Path $TestRoot (
    "swawkit-proj-shell-$([Guid]::NewGuid().ToString('N'))"
)
$UserPathBefore = [Environment]::GetEnvironmentVariable('PATH', 'User')
$MachinePathBefore = [Environment]::GetEnvironmentVariable('PATH', 'Machine')
$ProcessPathBefore = [Environment]::GetEnvironmentVariable('PATH', 'Process')
$ManagedPwshSource = Join-Path $RepoRoot (
    'data\proj.swawkit\modules\kernel\.dev\setup\export\pwsh\installs\7.6.4'
)
Assert-ProjShellTest `
    -Condition ([IO.File]::Exists((Join-Path $ManagedPwshSource 'pwsh.exe'))) `
    -Message "PowerShell 7 fixture is unavailable: $ManagedPwshSource"
$PoisonedAdapterEnvironment = [ordered]@{
    swawkit_proj_core_adapter_pwsh_arg_4095 = 'foreign-core-argument'
    SwAwKiT_PrOj_CoRe_AdApTeR_CmD_EnTrY_PaTh = 'C:\foreign-run.cmd'
    SwAwKiT_PrOj_MoDuLe_KeRnEl_DeV_PwSh_ArG_4095 = 'foreign-module-argument'
}
$SavedAdapterEnvironment = @{}
try {
    $Runtime = New-ProjCandidateRuntimeFixture `
        -RuntimeHome (Join-Path $TemporaryRoot 'runtime-home') `
        -LauncherPath $Artifacts.LauncherPath `
        -CorePath $Artifacts.CorePath `
        -HostPath $Artifacts.HostPath `
        -ToolchainPath $Artifacts.ToolchainPath
    $script:ProjShellEntry = Add-ProjCandidateRuntimeEntry `
        -Runtime $Runtime `
        -RelativePath "Favorites\$EntryName.exe"
    $RuntimeBin = $Runtime.RuntimeBin
    $DataRoot = Join-Path $Runtime.Home "data\proj.$EntryName"

    foreach ($Name in $PoisonedAdapterEnvironment.Keys) {
        $SavedAdapterEnvironment[$Name] = [Environment]::GetEnvironmentVariable(
            $Name,
            [EnvironmentVariableTarget]::Process
        )
        [Environment]::SetEnvironmentVariable(
            $Name,
            [string]$PoisonedAdapterEnvironment[$Name],
            [EnvironmentVariableTarget]::Process
        )
    }

    $SetupOutput = @(
        & $script:ProjShellEntry `
            '..entry.project.root' `
            '${SWAWKIT_HOME}' `
            2>&1
    )
    Assert-ProjShellTest `
        -Condition ($LASTEXITCODE -eq 0) `
        -Message "Entry Profile setup failed: $SetupOutput"
    foreach ($Group in @('bun', 'msvc', 'rust')) {
        $ModeOutput = @(
            & $script:ProjShellEntry `
                ".dev.$Group.mode" `
                'disabled' `
                2>&1
        )
        Assert-ProjShellTest `
            -Condition ($LASTEXITCODE -eq 0) `
            -Message "Entry Profile setup failed for $Group mode: $ModeOutput"
    }
    $PwshVersionOutput = @(
        & $script:ProjShellEntry `
            '.dev.pwsh.version' `
            '7.6.4' `
            2>&1
    )
    Assert-ProjShellTest `
        -Condition ($LASTEXITCODE -eq 0) `
        -Message "Entry Profile setup failed for managed PowerShell: $PwshVersionOutput"
    $IdentityOutput = @(
        & $script:ProjShellEntry `
            '..entry.git.name' `
            'Shell Fixture' `
            2>&1
    )
    Assert-ProjShellTest `
        -Condition ($LASTEXITCODE -eq 0) `
        -Message "Entry identity setup failed: $IdentityOutput"
    $ManagedPwshRoot = Join-Path $DataRoot (
        'modules\kernel\.dev\setup\export\pwsh\installs\7.6.4'
    )
    Copy-ProjFixtureHardLinkTree `
        -Source $ManagedPwshSource `
        -Destination $ManagedPwshRoot
    $DevelopmentSetup = Invoke-ProjShellTest `
        -Address '.dev.setup' `
        -Arguments ([string[]]@())
    Assert-ProjShellTest `
        -Condition ($DevelopmentSetup.ExitCode -eq 0) `
        -Message "Native development setup failed: $($DevelopmentSetup.Text)"

    $CmdCommand = [string]::Join(' & ', @(
        'echo SHELL_KIND=cmd'
        'echo ENTRY_NAME=%SWAWKIT_PROJ_ENTRY_COMMAND%'
        'echo COMMAND_PROTOCOL=%SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL%'
        'echo COMMAND_ADDRESS=%SWAWKIT_PROJ_CORE_COMMAND_ADDRESS%'
        'echo COMMAND_DATA_ROOT=%SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT%'
        'echo GIT_ID_NAME=%SWAWKIT_PROJ_GIT_ID_NAME%'
        'echo PROJ_HOME=%SWAWKIT_HOME%'
        'echo DATA_ROOT=%SWAWKIT_PROJ_DATA_ROOT%'
        'echo PATH_VALUE=%PATH%'
        'echo WORKING_DIR=%CD%'
        'echo CMD_SPECIAL=left^&right'
        'echo DELAYED=!SWAWKIT_PROJ_ENTRY_COMMAND!'
        'exit /b 31'
    ))
    $Cmd = Invoke-ProjShellTest `
        -Address '.dev.cmd' `
        -Arguments @('echo', 'JOINED_COMMAND=ok', '&', $CmdCommand)
    Assert-ProjShellTest `
        -Condition ($Cmd.ExitCode -eq 31) `
        -Message ".dev.cmd did not return the child exit code: $($Cmd.Text)"
    foreach ($Expected in @(
        'SHELL_KIND=cmd',
        'JOINED_COMMAND=ok',
        "ENTRY_NAME=$EntryName",
        'COMMAND_PROTOCOL=1',
        'COMMAND_ADDRESS=.dev.cmd',
        "COMMAND_DATA_ROOT=$DataRoot\modules\kernel\.dev\cmd",
        'GIT_ID_NAME=Shell Fixture',
        "PROJ_HOME=$($Runtime.Home)",
        "DATA_ROOT=$DataRoot",
        "PATH_VALUE=$RuntimeBin;",
        "WORKING_DIR=$($Runtime.Home)",
        'CMD_SPECIAL=left&right',
        'DELAYED=!SWAWKIT_PROJ_ENTRY_COMMAND!'
    )) {
        Assert-ProjShellTest `
            -Condition ($Cmd.Text.IndexOf(
                $Expected,
                [StringComparison]::OrdinalIgnoreCase
            ) -ge 0) `
            -Message ".dev.cmd did not preserve '$Expected': $($Cmd.Text)"
    }

    $PowerShellCommand = [string]::Join('; ', @(
        'Write-Output ''SHELL_KIND=ps-command'''
        'Write-Output "PS_MAJOR=$($PSVersionTable.PSVersion.Major)"'
        'Write-Output "PS_HOME=$PSHOME"'
        'Write-Output "POLICY=$((Get-ExecutionPolicy -Scope Process))"'
        'Write-Output "ENTRY_NAME=$env:SWAWKIT_PROJ_ENTRY_COMMAND"'
        'Write-Output "COMMAND_PROTOCOL=$env:SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL"'
        'Write-Output "COMMAND_ADDRESS=$env:SWAWKIT_PROJ_CORE_COMMAND_ADDRESS"'
        'Write-Output "COMMAND_DATA_ROOT=$env:SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT"'
        'Write-Output "GIT_ID_NAME=$env:SWAWKIT_PROJ_GIT_ID_NAME"'
        'Write-Output "PROJ_HOME=$env:SWAWKIT_HOME"'
        'Write-Output "DATA_ROOT=$env:SWAWKIT_PROJ_DATA_ROOT"'
        'Write-Output "PATH_VALUE=$env:PATH"'
        'Write-Output "WORKING_DIR=$((Get-Location).ProviderPath)"'
        'Write-Output ''COMMAND_TEXT=ampersand&pipe|percent%'''
        'exit 32'
    ))
    $PowerShell = Invoke-ProjShellTest `
        -Address '.dev.pwsh' `
        -Arguments @('-Command', $PowerShellCommand)
    Assert-ProjShellTest `
        -Condition ($PowerShell.ExitCode -eq 32) `
        -Message ".dev.pwsh -Command lost its exit code: $($PowerShell.Text)"
    foreach ($Expected in @(
        'SHELL_KIND=ps-command',
        'PS_MAJOR=7',
        "PS_HOME=$ManagedPwshRoot",
        'POLICY=Bypass',
        "ENTRY_NAME=$EntryName",
        'COMMAND_PROTOCOL=1',
        'COMMAND_ADDRESS=.dev.pwsh',
        "COMMAND_DATA_ROOT=$DataRoot\modules\kernel\.dev\pwsh",
        'GIT_ID_NAME=Shell Fixture',
        "PROJ_HOME=$($Runtime.Home)",
        "DATA_ROOT=$DataRoot",
        "WORKING_DIR=$($Runtime.Home)",
        'COMMAND_TEXT=ampersand&pipe|percent%'
    )) {
        Assert-ProjShellTest `
            -Condition ($PowerShell.Text.IndexOf(
                $Expected,
                [StringComparison]::OrdinalIgnoreCase
            ) -ge 0) `
            -Message ".dev.pwsh -Command did not preserve '$Expected': $($PowerShell.Text)"
    }

    $ScriptPath = Join-Path $Runtime.Home 'fixture\script with spaces.ps1'
    $ScriptSource = @'
param(
    [Parameter(Mandatory = $true)][string]$First,
    [Parameter(Mandatory = $true)][AllowEmptyString()][string]$Second,
    [Parameter(Mandatory = $true)][string]$Third
)
Write-Output ('SHELL_KIND=ps-file')
Write-Output ('FILE_ARGS=' + [string]::Join('|', @($First, $Second, $Third)))
Write-Output ('PS_MAJOR=' + $PSVersionTable.PSVersion.Major)
Write-Output ('POLICY=' + (Get-ExecutionPolicy -Scope Process))
Write-Output ('WORKING_DIR=' + (Get-Location).ProviderPath)
$ProcessEnvironmentNames = [string[]]@(
    [Environment]::GetEnvironmentVariables('Process').Keys
)
$CoreCommandAdapterNames = @(
    $ProcessEnvironmentNames | Where-Object {
        $_.StartsWith(
            'SWAWKIT_PROJ_CORE_COMMAND_ADAPTER_',
            [StringComparison]::OrdinalIgnoreCase
        )
    }
)
$ModuleNames = @(
    $ProcessEnvironmentNames | Where-Object {
        $_.StartsWith(
            'SWAWKIT_PROJ_MODULE_KERNEL_DEV_PWSH_',
            [StringComparison]::OrdinalIgnoreCase
        )
    }
)
Write-Output (
    'CORE_COMMAND_ADAPTER_INTERNAL_COUNT=' + $CoreCommandAdapterNames.Count
)
Write-Output ('MODULE_INTERNAL_COUNT=' + $ModuleNames.Count)
Write-Output ('UNDEFINED=' + [string]$ProjUndefinedVariable)
exit 33
'@
    [void][IO.Directory]::CreateDirectory(
        (Split-Path -Path $ScriptPath -Parent)
    )
    [IO.File]::WriteAllText(
        $ScriptPath,
        $ScriptSource,
        [Text.UTF8Encoding]::new($false)
    )
    $RelativeScriptPath = $ScriptPath.Substring(
        $Runtime.Home.TrimEnd('\').Length + 1
    )
    $PowerShellFile = Invoke-ProjShellTest `
        -Address '.dev.pwsh' `
        -Arguments @(
            '-File',
            $RelativeScriptPath,
            'hello world',
            'ampersand&value',
            'pipe|percent%'
        )
    Assert-ProjShellTest `
        -Condition ($PowerShellFile.ExitCode -eq 33) `
        -Message ".dev.pwsh -File lost its exit code: $($PowerShellFile.Text)"
    foreach ($Expected in @(
        'SHELL_KIND=ps-file',
        'FILE_ARGS=hello world|ampersand&value|pipe|percent%',
        'PS_MAJOR=7',
        'POLICY=Bypass',
        "WORKING_DIR=$($Runtime.Home)",
        'CORE_COMMAND_ADAPTER_INTERNAL_COUNT=0',
        'MODULE_INTERNAL_COUNT=0',
        'UNDEFINED='
    )) {
        Assert-ProjShellTest `
            -Condition ($PowerShellFile.Text.IndexOf(
                $Expected,
                [StringComparison]::OrdinalIgnoreCase
            ) -ge 0) `
            -Message ".dev.pwsh -File did not preserve '$Expected': $($PowerShellFile.Text)"
    }

    $NestedCmd = Invoke-ProjShellTest `
        -Address '.dev.cmd' `
        -Arguments @("`"$script:ProjShellEntry`"", '--help')
    Assert-ProjShellTest `
        -Condition (
            $NestedCmd.ExitCode -eq 1 -and
            $NestedCmd.Text.Contains('inside another Entry command')
        ) `
        -Message ".dev.cmd allowed a nested Entry: $($NestedCmd.Text)"

    $NestedEntryLiteral = $script:ProjShellEntry.Replace("'", "''")
    $NestedPowerShell = Invoke-ProjShellTest `
        -Address '.dev.pwsh' `
        -Arguments @(
            '-Command',
            "& '$NestedEntryLiteral' --help; exit `$LASTEXITCODE"
        )
    Assert-ProjShellTest `
        -Condition (
            $NestedPowerShell.ExitCode -eq 1 -and
            $NestedPowerShell.Text.Contains('inside another Entry command')
        ) `
        -Message ".dev.pwsh allowed a nested Entry: $($NestedPowerShell.Text)"

    $InvalidInvocations = @(
        @{ Address = '.dev.cmd'; Arguments = [string[]]@(); Name = 'no command' },
        @{ Address = '.dev.cmd'; Arguments = @(' '); Name = 'blank command' },
        @{ Address = '.dev.pwsh'; Arguments = [string[]]@(); Name = 'no mode' },
        @{ Address = '.dev.pwsh'; Arguments = @('-Command'); Name = 'missing command' },
        @{ Address = '.dev.pwsh'; Arguments = @('-Command', ' '); Name = 'blank command' },
        @{ Address = '.dev.pwsh'; Arguments = @('-File'); Name = 'missing file' },
        @{
            Address = '.dev.pwsh'
            Arguments = @('-File', 'missing.ps1')
            Name = 'absent file'
        },
        @{
            Address = '.dev.pwsh'
            Arguments = @('-File', 'not-a-script.txt')
            Name = 'non-ps1 file'
        },
        @{
            Address = '.dev.pwsh'
            Arguments = @('-Unknown', 'value')
            Name = 'unknown mode'
        }
    )
    foreach ($Invocation in $InvalidInvocations) {
        $Rejected = Invoke-ProjShellTest `
            -Address $Invocation.Address `
            -Arguments ([string[]]$Invocation.Arguments)
        Assert-ProjShellTest `
            -Condition ($Rejected.ExitCode -eq 1) `
            -Message "$($Invocation.Address) accepted $($Invocation.Name)"
    }

    $SystemPwshRoot = Join-Path $TemporaryRoot 'system-pwsh'
    Copy-ProjFixtureHardLinkTree `
        -Source $ManagedPwshSource `
        -Destination $SystemPwshRoot
    Set-ProjShellProcessPath `
        -Value ($SystemPwshRoot + ';' + $ProcessPathBefore)
    $SystemMode = @(
        & $script:ProjShellEntry `
            '.dev.pwsh.mode' `
            'system' `
            2>&1
    )
    Assert-ProjShellTest `
        -Condition ($LASTEXITCODE -eq 0) `
        -Message "failed to select system PowerShell: $SystemMode"
    $SystemSetup = Invoke-ProjShellTest `
        -Address '.dev.setup' `
        -Arguments ([string[]]@())
    Assert-ProjShellTest `
        -Condition (
            $SystemSetup.ExitCode -eq 0 -and
            $SystemSetup.Text.Contains('PowerShell 7.6.4 system runtime is ready')
        ) `
        -Message "system PowerShell setup failed: $($SystemSetup.Text)"
    $SystemPowerShell = Invoke-ProjShellTest `
        -Address '.dev.pwsh' `
        -Arguments @(
            '-Command',
            'Write-Output (''SYSTEM_PS_HOME='' + $PSHOME); exit 34'
        )
    Assert-ProjShellTest `
        -Condition (
            $SystemPowerShell.ExitCode -eq 34 -and
            $SystemPowerShell.Text.Contains("SYSTEM_PS_HOME=$SystemPwshRoot")
        ) `
        -Message "system PowerShell execution failed: $($SystemPowerShell.Text)"

    $DisabledMode = @(
        & $script:ProjShellEntry `
            '.dev.pwsh.mode' `
            'disabled' `
            2>&1
    )
    Assert-ProjShellTest `
        -Condition ($LASTEXITCODE -eq 0) `
        -Message "failed to disable PowerShell: $DisabledMode"
    $DisabledSetup = Invoke-ProjShellTest `
        -Address '.dev.setup' `
        -Arguments ([string[]]@())
    Assert-ProjShellTest `
        -Condition ($DisabledSetup.ExitCode -eq 0) `
        -Message "disabled PowerShell setup failed: $($DisabledSetup.Text)"
    $DisabledPowerShell = Invoke-ProjShellTest `
        -Address '.dev.pwsh' `
        -Arguments @('-Command', 'exit 0')
    Assert-ProjShellTest `
        -Condition (
            $DisabledPowerShell.ExitCode -eq 1 -and
            $DisabledPowerShell.Text.Contains('run.ps1 is disabled')
        ) `
        -Message "disabled PowerShell remained runnable: $($DisabledPowerShell.Text)"

    Assert-ProjShellTest `
        -Condition (
            [Environment]::GetEnvironmentVariable('PATH', 'User') -ceq
                $UserPathBefore -and
            [Environment]::GetEnvironmentVariable('PATH', 'Machine') -ceq
                $MachinePathBefore
        ) `
        -Message 'project shell commands changed persistent PATH'
} finally {
    Set-ProjShellProcessPath -Value $ProcessPathBefore
    foreach ($Name in $SavedAdapterEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            $SavedAdapterEnvironment[$Name],
            [EnvironmentVariableTarget]::Process
        )
    }
    Remove-ProjCandidateRuntimeFixture -Path $TemporaryRoot
}

Write-Host '[PASS] Proj one-shot shell commands' -ForegroundColor Green
$global:LASTEXITCODE = 0
