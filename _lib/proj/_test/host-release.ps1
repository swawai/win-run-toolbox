[CmdletBinding()]
param(
    [string]$LauncherPath = '',
    [string]$CorePath = '',
    [string]$HostPath = '',
    [string]$ToolchainPath = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjHostReleaseTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Host Release Set test failed: $Message"
    }
}

function Get-ProjHostReleaseProcesses {
    param([Parameter(Mandatory = $true)][string]$ExpectedPath)

    $ExpectedPath = [IO.Path]::GetFullPath($ExpectedPath)
    return @(
        Get-Process -Name 'swawkit-proj-host' -ErrorAction SilentlyContinue |
            Where-Object {
                try {
                    [IO.Path]::GetFullPath($_.Path).Equals(
                        $ExpectedPath,
                        [StringComparison]::OrdinalIgnoreCase
                    )
                } catch {
                    $false
                }
            }
    )
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
. (Join-Path $PSScriptRoot '_lib\runtime-fixture.ps1')
. (Join-Path $PSScriptRoot '_lib\owned-process-tree.ps1')
$Artifacts = Resolve-ProjCandidateRuntimeArtifacts `
    -LauncherPath $LauncherPath `
    -CorePath $CorePath `
    -HostPath $HostPath `
    -ToolchainPath $ToolchainPath
$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-proj-host-release-$([Guid]::NewGuid().ToString('N'))"
)
$Runtime = $null
$HostProcess = $null
$OwnedTrees = [Collections.Generic.List[IDisposable]]::new()

try {
    $Runtime = New-ProjCandidateRuntimeFixture `
        -RuntimeHome (Join-Path $TemporaryRoot 'runtime-home') `
        -LauncherPath $Artifacts.LauncherPath `
        -CorePath $Artifacts.CorePath `
        -HostPath $Artifacts.HostPath `
        -ToolchainPath $Artifacts.ToolchainPath
    $EntryPath = Add-ProjCandidateRuntimeEntry `
        -Runtime $Runtime `
        -RelativePath 'Favorites\host-release.exe'
    $PrimaryTree = Start-ProjOwnedProcessTree `
        -FilePath $EntryPath `
        -WorkingDirectory $Runtime.Home
    $OwnedTrees.Add($PrimaryTree)
    Assert-ProjHostReleaseTest `
        -Condition $PrimaryTree.WaitForExit(5000) `
        -Message 'the first Launcher did not exit after handing off to Core'
    Assert-ProjHostReleaseTest `
        -Condition ($PrimaryTree.ExitCode -eq 0) `
        -Message 'the first Launcher reported a failed Core handoff'
    $RuntimeDirectory = Join-Path $Runtime.Home 'data\proj.swawkit\runtime\hosts'
    $Deadline = [DateTime]::UtcNow.AddSeconds(10)
    $Document = $null
    while ([DateTime]::UtcNow -lt $Deadline) {
        $DocumentPath = Get-ChildItem `
            -LiteralPath $RuntimeDirectory `
            -Filter '*.json' `
            -File `
            -ErrorAction SilentlyContinue |
            Select-Object -First 1 -ExpandProperty FullName
        if ($null -ne $DocumentPath) {
            try {
                $Document = [IO.File]::ReadAllText(
                    $DocumentPath,
                    [Text.Encoding]::UTF8
                ) | ConvertFrom-Json
                break
            } catch {
            }
        }
        Start-Sleep -Milliseconds 50
    }
    Assert-ProjHostReleaseTest `
        -Condition ($null -ne $Document) `
        -Message 'the Host did not publish a runtime document'
    $HostProcess = Get-Process -Id ([int]$Document.pid) -ErrorAction Stop
    $ExpectedHost = Join-Path $Runtime.RuntimeRelease 'swawkit-proj-host.exe'
    Assert-ProjHostReleaseTest `
        -Condition (
            [IO.Path]::GetFullPath($HostProcess.Path).Equals(
                [IO.Path]::GetFullPath($ExpectedHost),
                [StringComparison]::OrdinalIgnoreCase
            ) -and
            [string]$Document.url -cmatch '^http://127\.0\.0\.1:\d+/$'
        ) `
        -Message 'the selected Core did not transfer ownership to its sibling Host'

    $SecondTree = Start-ProjOwnedProcessTree `
        -FilePath $EntryPath `
        -WorkingDirectory $Runtime.Home
    $OwnedTrees.Add($SecondTree)
    Assert-ProjHostReleaseTest `
        -Condition $SecondTree.WaitForExit(5000) `
        -Message 'the second Launcher did not return after finding the existing Host'
    Assert-ProjHostReleaseTest `
        -Condition ($SecondTree.ExitCode -eq 0) `
        -Message 'the second Launcher reported a failed Host activation'

    $ActivationDeadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $ExpectedHosts = @(Get-ProjHostReleaseProcesses `
            -ExpectedPath $ExpectedHost)
        $ActivationObserved = $SecondTree.TotalProcesses -ge 4 -and
            $ExpectedHosts.Count -eq 1 -and
            $ExpectedHosts[0].Id -eq [int]$Document.pid
        if (-not $ActivationObserved) {
            Start-Sleep -Milliseconds 50
        }
    } while (-not $ActivationObserved -and
        [DateTime]::UtcNow -lt $ActivationDeadline)
    Assert-ProjHostReleaseTest `
        -Condition $ActivationObserved `
        -Message (
            'a second Entry launch did not activate and retire its transient ' +
            'Core/Host process tree'
        )
    $DocumentAfterActivation = [IO.File]::ReadAllText(
        $DocumentPath,
        [Text.Encoding]::UTF8
    ) | ConvertFrom-Json
    Assert-ProjHostReleaseTest `
        -Condition ([int]$DocumentAfterActivation.pid -eq [int]$Document.pid) `
        -Message 'the second Entry launch replaced the primary Host identity'
    Invoke-WebRequest `
        -UseBasicParsing `
        -Uri ([string]$Document.url) `
        -TimeoutSec 5 |
        Out-Null
    $SecondTree.Dispose()
    [void]$OwnedTrees.Remove($SecondTree)

    Invoke-WebRequest `
        -UseBasicParsing `
        -Method Post `
        -Uri ([string]$Document.url + 'api/v2/host/shutdown') `
        -Headers @{ 'x-swawkit-control' = 'shutdown' } |
        Out-Null
    Assert-ProjHostReleaseTest `
        -Condition $HostProcess.WaitForExit(10000) `
        -Message 'the Host did not honor its product shutdown path'
    $HostProcess.Dispose()
    $HostProcess = $null
} finally {
    for ($Index = $OwnedTrees.Count - 1; $Index -ge 0; $Index--) {
        try {
            $OwnedTrees[$Index].Dispose()
        } catch {
            Write-Warning (
                'Owned Host test process tree cleanup failed: ' +
                $_.Exception.Message
            )
        }
    }
    if ($null -ne $HostProcess) {
        $HostProcess.Dispose()
    }
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        Remove-ProjCandidateRuntimeFixture -Path $TemporaryRoot
    }
}

Write-Host '[PASS] Proj Core-to-Host Release Set handoff' -ForegroundColor Green
$global:LASTEXITCODE = 0
