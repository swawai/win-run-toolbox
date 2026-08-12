[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$ToolchainPath)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjNativeSetup {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Proj native setup test failed: $Message"
    }
}

function Invoke-ProjNativeSetup {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][hashtable]$Environment,
        [string[]]$Arguments = @()
    )
    $Info = [Diagnostics.ProcessStartInfo]::new()
    $Info.FileName = $Executable
    $Info.Arguments = [string]::Join(' ', @('command-v1', 'dev.setup') + $Arguments)
    $Info.UseShellExecute = $false
    $Info.CreateNoWindow = $true
    $Info.RedirectStandardOutput = $true
    $Info.RedirectStandardError = $true
    # Windows PowerShell 5.1 initializes this dictionary lazily.
    $null = $Info.EnvironmentVariables
    foreach ($Pair in $Environment.GetEnumerator()) {
        $Info.EnvironmentVariables[[string]$Pair.Key] = [string]$Pair.Value
    }
    $Process = [Diagnostics.Process]::Start($Info)
    try {
        $OutputTask = $Process.StandardOutput.ReadToEndAsync()
        $ErrorOutputTask = $Process.StandardError.ReadToEndAsync()
        if (-not $Process.WaitForExit(30000)) {
            $Process.Kill()
            $Process.WaitForExit()
            throw 'native setup handler timed out'
        }
        $Output = $OutputTask.GetAwaiter().GetResult()
        $ErrorOutput = $ErrorOutputTask.GetAwaiter().GetResult()
        return [pscustomobject]@{
            ExitCode = [int]$Process.ExitCode
            Output = ($Output + $ErrorOutput).TrimEnd()
        }
    } finally {
        $Process.Dispose()
    }
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$Executable = [IO.Path]::GetFullPath($ToolchainPath)
$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-native-setup-$([Guid]::NewGuid().ToString('N'))"
)
try {
    $DataRoot = Join-Path $TemporaryRoot 'data root'
    $Profile = Join-Path $DataRoot '_profile.json'
    [void][IO.Directory]::CreateDirectory($DataRoot)
    [IO.File]::WriteAllText(
        $Profile,
        "{}`r`n",
        [Text.UTF8Encoding]::new($false)
    )
    $Revision = 'sha256-' + (
        Get-FileHash -LiteralPath $Profile -Algorithm SHA256
    ).Hash.ToLowerInvariant()
    $Environment = @{
        SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL = '1'
        SWAWKIT_PROJ_CORE_COMMAND_PHASE = 'run'
        SWAWKIT_PROJ_CORE_COMMAND_ADDRESS = '.dev.setup'
        SWAWKIT_PROJ_DATA_ROOT = $DataRoot
        SWAWKIT_HOME = $RepoRoot
        SWAWKIT_PROJ_ENTRY_COMMAND = 'fixture'
        SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION = (
            'sha256-' + ('b' * 64)
        )
        SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION = $Revision
        SWAWKIT_PROJ_BUN_MODE = 'disabled'
        SWAWKIT_PROJ_PWSH_MODE = 'disabled'
        SWAWKIT_PROJ_MSVC_MODE = 'disabled'
        SWAWKIT_PROJ_RUST_MODE = 'disabled'
        SWAWKIT_PROJ_GO_MODE = 'disabled'
        SWAWKIT_PROJ_PYTHON_MODE = 'disabled'
        SWAWKIT_PROJ_UV_MODE = 'disabled'
    }
    $Legacy = Join-Path $DataRoot (
        'modules\kernel\.dev\setup\export\_state.json'
    )
    [void][IO.Directory]::CreateDirectory((Split-Path $Legacy -Parent))
    [IO.File]::WriteAllText($Legacy, '{"legacy":true}')

    $Ready = Invoke-ProjNativeSetup `
        -Executable $Executable `
        -Environment $Environment
    $SetupRoot = Join-Path $DataRoot 'modules\kernel\.dev\setup'
    $State = Get-Content -LiteralPath (Join-Path $SetupRoot '_state.json') `
        -Raw | ConvertFrom-Json
    Assert-ProjNativeSetup `
        -Condition ($Ready.ExitCode -eq 0 -and
            $Ready.Output.Contains(
                '[OK] The base development environment is ready.'
            ) -and
            $State.status -ceq 'ready' -and
            $State.producerContract -ceq 'swawkit.proj.dev-setup/v2' -and
            [IO.File]::Exists((Join-Path $SetupRoot 'export\env.cmd')) -and
            [IO.File]::Exists((Join-Path $SetupRoot 'export\env.ps1')) -and
            -not [IO.File]::Exists($Legacy)) `
        -Message "the native handler did not publish a ready provider: $($Ready.Output)"

    $Before = (
        Get-FileHash -LiteralPath (Join-Path $SetupRoot 'export\env.cmd') `
            -Algorithm SHA256
    ).Hash
    $Rejected = Invoke-ProjNativeSetup `
        -Executable $Executable `
        -Environment $Environment `
        -Arguments @('unexpected')
    $After = (
        Get-FileHash -LiteralPath (Join-Path $SetupRoot 'export\env.cmd') `
            -Algorithm SHA256
    ).Hash
    Assert-ProjNativeSetup `
        -Condition ($Rejected.ExitCode -ne 0 -and
            $Rejected.Output.Contains(
                '.dev.setup does not accept dynamic arguments'
            ) -and $Before -ceq $After) `
        -Message 'argument rejection changed the published environment'
} finally {
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        [IO.Directory]::Delete($TemporaryRoot, $true)
    }
}

Write-Host '[PASS] Proj native .dev.setup handler' -ForegroundColor Green
$global:LASTEXITCODE = 0
