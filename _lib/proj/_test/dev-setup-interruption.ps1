[CmdletBinding()]
param(
    [string]$LauncherPath = '',
    [string]$CorePath = '',
    [string]$HostPath = '',
    [string]$ToolchainPath = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjSetupInterruption {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Proj setup interruption test failed: $Message"
    }
}

function Invoke-ProjSetupEntry {
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
    return [pscustomobject][ordered]@{
        ExitCode = [int]$ExitCode
        Text = [string]::Join(
            [Environment]::NewLine,
            [string[]]@($Output | ForEach-Object { [string]$_ })
        )
    }
}

function New-ProjCancellationAction {
    param([Parameter(Mandatory = $true)][string]$OutputAssembly)

    $OutputAssembly = [IO.Path]::GetFullPath($OutputAssembly)
    [void][IO.Directory]::CreateDirectory(
        [IO.Path]::GetDirectoryName($OutputAssembly)
    )
    Add-Type -OutputType ConsoleApplication -OutputAssembly $OutputAssembly `
        -TypeDefinition @'
using System;
using System.Diagnostics;
using System.IO;

public static class ProjCancellationAction
{
    public static int Main()
    {
        string root = Environment.GetEnvironmentVariable(
            "SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT"
        );
        if (String.IsNullOrWhiteSpace(root)) return 64;
        Directory.CreateDirectory(root);
        ProcessStartInfo start = new ProcessStartInfo();
        start.FileName = Environment.GetEnvironmentVariable("ComSpec");
        start.Arguments = "/d /s /c \"ping -t 127.0.0.1 >nul\"";
        start.UseShellExecute = false;
        start.CreateNoWindow = true;
        using (Process child = Process.Start(start))
        {
            File.WriteAllText(
                Path.Combine(root, "descendant.pid"),
                child.Id.ToString()
            );
            child.WaitForExit();
            return child.ExitCode;
        }
    }
}
'@
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
. (Join-Path $PSScriptRoot '_lib\runtime-fixture.ps1')
. (Join-Path $PSScriptRoot '_lib\console-cancel-fixture.ps1')
$Artifacts = Resolve-ProjCandidateRuntimeArtifacts `
    -LauncherPath $LauncherPath `
    -CorePath $CorePath `
    -HostPath $HostPath `
    -ToolchainPath $ToolchainPath
$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-proj-interrupt-$([Guid]::NewGuid().ToString('N'))"
)
$EntryName = "interrupt-$([Guid]::NewGuid().ToString('N'))"
$Lock = $null

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
    $CancellationAction = Join-Path $Runtime.Home '.swaw\cancel-tree\run.exe'
    New-ProjCancellationAction -OutputAssembly $CancellationAction

    $Bound = Invoke-ProjSetupEntry `
        -EntryPath $EntryPath `
        -Arguments @(
            '..entry.project.root',
            '${SWAWKIT_HOME}'
        )
    Assert-ProjSetupInterruption `
        -Condition ($Bound.ExitCode -eq 0) `
        -Message "cannot create the isolated Entry Profile: $($Bound.Text)"
    foreach ($Tool in @('bun', 'pwsh', 'msvc', 'rust')) {
        $Disabled = Invoke-ProjSetupEntry `
            -EntryPath $EntryPath `
            -Arguments @(".dev.$Tool.mode", 'disabled')
        Assert-ProjSetupInterruption `
            -Condition ($Disabled.ExitCode -eq 0) `
            -Message "cannot disable ${Tool}: $($Disabled.Text)"
    }

    $SetupRoot = Join-Path $DataRoot 'modules\kernel\.dev\setup'
    $ProviderStatePath = Join-Path $SetupRoot '_state.json'
    $ProviderStateBefore = if ([IO.File]::Exists($ProviderStatePath)) {
        (Get-FileHash -LiteralPath $ProviderStatePath -Algorithm SHA256).Hash
    } else {
        $null
    }
    $LockRoot = Join-Path $SetupRoot 'locks'
    [void][IO.Directory]::CreateDirectory($LockRoot)
    $LockPath = Join-Path $LockRoot 'setup.lock'
    $Lock = [IO.File]::Open(
        $LockPath,
        [IO.FileMode]::OpenOrCreate,
        [IO.FileAccess]::ReadWrite,
        [IO.FileShare]::None
    )

    $DriverPath = New-ProjConsoleCancelDriver `
        -OutputAssembly (Join-Path $TemporaryRoot 'console-cancel.exe')
    $ResultPath = Join-Path $TemporaryRoot 'console-cancel.json'
    & $DriverPath $EntryPath $Runtime.Home $ResultPath
    $DriverExitCode = $LASTEXITCODE
    Assert-ProjSetupInterruption `
        -Condition ([IO.File]::Exists($ResultPath)) `
        -Message "console cancel driver produced no result (exit $DriverExitCode)"
    $Result = Get-Content -LiteralPath $ResultPath -Raw | ConvertFrom-Json
    $Lock.Dispose()
    $Lock = $null
    $LockProbe = [IO.File]::Open(
        $LockPath,
        [IO.FileMode]::Open,
        [IO.FileAccess]::ReadWrite,
        [IO.FileShare]::None
    )
    $LockProbe.Dispose()

    $RunStates = @(
        Get-ChildItem -LiteralPath (Join-Path $SetupRoot '_runs') `
            -Filter '_state.json' -File -Recurse |
            ForEach-Object {
                Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json
            }
    )
    $RunSummary = [string]::Join(
        ', ',
        [string[]]@($RunStates | ForEach-Object {
            '{0}:{1}' -f [string]$_.source, [string]$_.status
        })
    )
    Assert-ProjSetupInterruption `
        -Condition (
            $DriverExitCode -eq 0 -and
            [bool]$Result.exited -and
            [bool]$Result.treeDrained -and
            [uint32]$Result.observedProcesses -ge 3 -and
            $null -eq $Result.error
        ) `
        -Message (
            'console cancellation did not converge Launcher, Core, and Toolchain: ' +
            (Get-Content -LiteralPath $ResultPath -Raw) +
            "; journals=$RunSummary"
        )
    Assert-ProjSetupInterruption `
        -Condition (
            $RunStates.Count -eq 1 -and
            $RunStates[0].source -ceq 'cli' -and
            $RunStates[0].status -ceq 'canceled' -and
            $null -ne $RunStates[0].finishedAtUnixMs
        ) `
        -Message 'console cancellation left the CLI run journal non-terminal'
    $ProviderStateAfter = if ([IO.File]::Exists($ProviderStatePath)) {
        (Get-FileHash -LiteralPath $ProviderStatePath -Algorithm SHA256).Hash
    } else {
        $null
    }
    Assert-ProjSetupInterruption `
        -Condition ($ProviderStateBefore -ceq $ProviderStateAfter) `
        -Message 'a setup canceled before acquiring its lock changed Provider state'

    $Retry = Invoke-ProjSetupEntry `
        -EntryPath $EntryPath `
        -Arguments @('.dev.setup')
    $Provider = Get-Content -LiteralPath $ProviderStatePath `
        -Raw | ConvertFrom-Json
    Assert-ProjSetupInterruption `
        -Condition (
            $Retry.ExitCode -eq 0 -and
            $Retry.Text.Contains(
                '[OK] The base development environment is ready.'
            ) -and
            $Provider.status -ceq 'ready' -and
            [IO.File]::Exists((Join-Path $SetupRoot 'export\env.cmd'))
        ) `
        -Message "setup did not recover after console cancellation: $($Retry.Text)"

    $TreeResultPath = Join-Path $TemporaryRoot 'console-cancel-tree.json'
    & $DriverPath $EntryPath $Runtime.Home $TreeResultPath 'cancel-tree' '4'
    $TreeDriverExitCode = $LASTEXITCODE
    Assert-ProjSetupInterruption `
        -Condition ([IO.File]::Exists($TreeResultPath)) `
        -Message "process-tree cancel driver produced no result (exit $TreeDriverExitCode)"
    $TreeResult = Get-Content -LiteralPath $TreeResultPath -Raw |
        ConvertFrom-Json
    $ActionDataRoot = Join-Path $DataRoot 'modules\action\cancel-tree'
    $ActionStates = @(
        Get-ChildItem -LiteralPath (Join-Path $ActionDataRoot '_runs') `
            -Filter '_state.json' -File -Recurse |
            ForEach-Object {
                Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json
            }
    )
    Assert-ProjSetupInterruption `
        -Condition (
            $TreeDriverExitCode -eq 0 -and
            [bool]$TreeResult.exited -and
            [bool]$TreeResult.treeDrained -and
            [uint32]$TreeResult.observedProcesses -ge 4 -and
            $null -eq $TreeResult.error
        ) `
        -Message (
            'Core did not terminate the canceled Action process tree: ' +
            (Get-Content -LiteralPath $TreeResultPath -Raw)
        )
    Assert-ProjSetupInterruption `
        -Condition (
            $ActionStates.Count -eq 1 -and
            $ActionStates[0].source -ceq 'cli' -and
            $ActionStates[0].status -ceq 'canceled' -and
            $null -ne $ActionStates[0].finishedAtUnixMs
        ) `
        -Message 'the canceled Action process tree left a non-terminal journal'
} finally {
    if ($null -ne $Lock) {
        $Lock.Dispose()
    }
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        Remove-ProjCandidateRuntimeFixture -Path $TemporaryRoot
    }
}

Write-Host '[PASS] Proj CLI cancellation and process-tree lifecycle' -ForegroundColor Green
$global:LASTEXITCODE = 0
