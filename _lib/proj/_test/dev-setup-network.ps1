[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ToolchainPath,
    [switch]$PublicNetwork
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjSetupNetwork {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Proj setup network test failed: $Message"
    }
}

function Assert-ProjSetupNetworkTemporaryRoot {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$RepositoryRoot
    )
    $Resolved = [IO.Path]::GetFullPath($Path)
    $ExpectedParent = [IO.Path]::GetFullPath(
        (Join-Path $RepositoryRoot 'data\_test')
    ).TrimEnd('\') + '\'
    if (-not $Resolved.StartsWith(
        $ExpectedParent,
        [StringComparison]::OrdinalIgnoreCase
    ) -or -not [IO.Path]::GetFileName($Resolved).StartsWith(
        'swawkit-setup-network-',
        [StringComparison]::Ordinal
    )) {
        throw "Unsafe setup network test root: $Resolved"
    }
    return $Resolved
}

function Invoke-ProjNetworkSetup {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][hashtable]$Environment,
        [int]$TimeoutSeconds = 60
    )
    $Info = [Diagnostics.ProcessStartInfo]::new()
    $Info.FileName = $Executable
    $Info.Arguments = 'command-v1 dev.setup'
    $Info.UseShellExecute = $false
    $Info.CreateNoWindow = $true
    $Info.RedirectStandardOutput = $true
    $Info.RedirectStandardError = $true
    $Info.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $Info.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    [void]$Info.EnvironmentVariables
    foreach ($Name in [string[]]@($Info.EnvironmentVariables.Keys)) {
        if ($Name.StartsWith('SWAWKIT_', [StringComparison]::OrdinalIgnoreCase)) {
            [void]$Info.EnvironmentVariables.Remove($Name)
        }
    }
    foreach ($Pair in $Environment.GetEnumerator()) {
        if ($null -eq $Pair.Value) {
            [void]$Info.EnvironmentVariables.Remove([string]$Pair.Key)
        } else {
            $Info.EnvironmentVariables[[string]$Pair.Key] = [string]$Pair.Value
        }
    }
    $Process = [Diagnostics.Process]::Start($Info)
    try {
        $Output = $Process.StandardOutput.ReadToEndAsync()
        $ErrorOutput = $Process.StandardError.ReadToEndAsync()
        if (-not $Process.WaitForExit($TimeoutSeconds * 1000)) {
            $Process.Kill()
            [void]$Process.WaitForExit(5000)
            throw ".dev.setup exceeded its $TimeoutSeconds second test boundary"
        }
        return [pscustomobject][ordered]@{
            ExitCode = [int]$Process.ExitCode
            Output = ([string]$Output.GetAwaiter().GetResult() +
                [string]$ErrorOutput.GetAwaiter().GetResult()).TrimEnd()
        }
    } finally {
        if (-not $Process.HasExited) {
            $Process.Kill()
            [void]$Process.WaitForExit(5000)
        }
        $Process.Dispose()
    }
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$Executable = [IO.Path]::GetFullPath($ToolchainPath)
$TemporaryRoot = Assert-ProjSetupNetworkTemporaryRoot `
    -RepositoryRoot $RepoRoot `
    -Path (Join-Path $RepoRoot (
        "data\_test\swawkit-setup-network-$([Guid]::NewGuid().ToString('N'))"
    ))

try {
    $FixtureHome = Join-Path $TemporaryRoot 'home'
    $DataRoot = Join-Path $FixtureHome 'data\proj.network'
    [void][IO.Directory]::CreateDirectory($DataRoot)
    [void][IO.Directory]::CreateDirectory(
        (Join-Path $FixtureHome 'data\proj_cache')
    )
    $ProfilePath = Join-Path $DataRoot '_profile.json'
    [IO.File]::WriteAllText(
        $ProfilePath,
        "{}`r`n",
        [Text.UTF8Encoding]::new($false)
    )
    $ProfileRevision = 'sha256-' + (
        Get-FileHash -LiteralPath $ProfilePath -Algorithm SHA256
    ).Hash.ToLowerInvariant()
    $Environment = @{
        SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL = '1'
        SWAWKIT_PROJ_CORE_COMMAND_PHASE = 'run'
        SWAWKIT_PROJ_CORE_COMMAND_ADDRESS = '.dev.setup'
        SWAWKIT_PROJ_DATA_ROOT = $DataRoot
        SWAWKIT_HOME = $FixtureHome
        SWAWKIT_PROJ_ENTRY_COMMAND = 'network-fixture'
        SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION = (
            'sha256-' + ('b' * 64)
        )
        SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION = $ProfileRevision
        SWAWKIT_PROJ_BUN_MODE = 'managed'
        SWAWKIT_PROJ_BUN_VERSION = 'latest'
        SWAWKIT_PROJ_BUN_SHA256 = ''
        SWAWKIT_PROJ_PWSH_MODE = 'disabled'
        SWAWKIT_PROJ_PWSH_VERSION = ''
        SWAWKIT_PROJ_PWSH_SHA256 = ''
        SWAWKIT_PROJ_MSVC_MODE = 'disabled'
        SWAWKIT_PROJ_RUST_MODE = 'disabled'
        SWAWKIT_PROJ_GO_MODE = 'disabled'
        SWAWKIT_PROJ_PYTHON_MODE = 'disabled'
        SWAWKIT_PROJ_UV_MODE = 'disabled'
        HTTP_PROXY = 'http://127.0.0.1:1'
        HTTPS_PROXY = 'http://127.0.0.1:1'
        ALL_PROXY = 'http://127.0.0.1:1'
        NO_PROXY = ''
    }
    $Failed = Invoke-ProjNetworkSetup `
        -Executable $Executable `
        -Environment $Environment `
        -TimeoutSeconds 45
    Write-Verbose $Failed.Output
    $SetupRoot = Join-Path $DataRoot 'modules\kernel\.dev\setup'
    $SelectionPath = Join-Path $SetupRoot (
        'export\bun\.swawkit-dev-selection.json'
    )
    $Provider = Get-Content -LiteralPath (Join-Path $SetupRoot '_state.json') `
        -Raw | ConvertFrom-Json
    Assert-ProjSetupNetwork `
        -Condition (
            $Failed.ExitCode -ne 0 -and
            $Failed.Output.Contains(
                'cannot resolve Bun from GitHub Releases'
            ) -and
            $Provider.status -ceq 'unavailable' -and
            -not [IO.File]::Exists($SelectionPath) -and
            -not [IO.Directory]::Exists((Join-Path $SetupRoot 'export\bun\installs'))
        ) `
        -Message (
            'a refused proxy did not fail closed before publication: ' +
            $Failed.Output
        )

    if (-not $PublicNetwork) {
        Write-Host '[PASS] Proj .dev.setup deterministic network failure' `
            -ForegroundColor Green
        $global:LASTEXITCODE = 0
        return
    }

    $Environment.HTTP_PROXY = $null
    $Environment.HTTPS_PROXY = $null
    $Environment.ALL_PROXY = $null
    $Environment.NO_PROXY = $null
    $Cold = Invoke-ProjNetworkSetup `
        -Executable $Executable `
        -Environment $Environment `
        -TimeoutSeconds 1200
    Assert-ProjSetupNetwork `
        -Condition ($Cold.ExitCode -eq 0 -and [IO.File]::Exists($SelectionPath)) `
        -Message "public cold setup failed: $($Cold.Output)"
    $SelectionBytes = [IO.File]::ReadAllBytes($SelectionPath)
    $Selection = Get-Content -LiteralPath $SelectionPath -Raw | ConvertFrom-Json
    $InstallRoot = Join-Path $SetupRoot (
        'export\bun\installs\' + [string]$Selection.version
    )
    $BunExecutable = Join-Path $InstallRoot 'bun.exe'
    $CacheArchives = @(
        Get-ChildItem -LiteralPath (Join-Path $FixtureHome 'data\proj_cache\downloads') `
            -Filter '*.zip' -File -Recurse
    )
    Assert-ProjSetupNetwork `
        -Condition (
            [IO.File]::Exists($BunExecutable) -and
            $CacheArchives.Count -ge 1 -and
            [string]$Selection.sourceSha256 -match '^[0-9a-f]{64}$' -and
            [string]$Selection.sourceVerification -match '^(github|unverified)$'
        ) `
        -Message 'public cold setup did not publish a verified installation and cache'
    $ActualVersion = @(& $BunExecutable --version 2>&1).Trim()
    Assert-ProjSetupNetwork `
        -Condition ($LASTEXITCODE -eq 0 -and $ActualVersion -ceq $Selection.version) `
        -Message "installed Bun version mismatch: $ActualVersion"

    $InstallPrefix = [IO.Path]::GetFullPath(
        (Join-Path $SetupRoot 'export\bun\installs')
    ).TrimEnd('\') + '\'
    $ResolvedInstall = [IO.Path]::GetFullPath($InstallRoot)
    Assert-ProjSetupNetwork `
        -Condition ($ResolvedInstall.StartsWith(
            $InstallPrefix,
            [StringComparison]::OrdinalIgnoreCase
        )) `
        -Message "unsafe network test install path: $ResolvedInstall"
    [IO.Directory]::Delete($ResolvedInstall, $true)
    $Environment.HTTP_PROXY = 'http://127.0.0.1:1'
    $Environment.HTTPS_PROXY = 'http://127.0.0.1:1'
    $Environment.ALL_PROXY = 'http://127.0.0.1:1'
    $Environment.NO_PROXY = ''
    $Offline = Invoke-ProjNetworkSetup `
        -Executable $Executable `
        -Environment $Environment `
        -TimeoutSeconds 90
    Assert-ProjSetupNetwork `
        -Condition (
            $Offline.ExitCode -eq 0 -and
            [IO.File]::Exists($BunExecutable) -and
            [Linq.Enumerable]::SequenceEqual(
                [byte[]]$SelectionBytes,
                [byte[]][IO.File]::ReadAllBytes($SelectionPath)
            )
        ) `
        -Message (
            'published latest selection could not reinstall from cache offline: ' +
            $Offline.Output
        )

    Write-Host (
        '[PASS] Proj .dev.setup public cold download and offline cache reuse ' +
        "(Bun $($Selection.version))"
    ) -ForegroundColor Green
} finally {
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        [void](Assert-ProjSetupNetworkTemporaryRoot `
            -Path $TemporaryRoot `
            -RepositoryRoot $RepoRoot)
        [IO.Directory]::Delete($TemporaryRoot, $true)
    }
}

$global:LASTEXITCODE = 0
