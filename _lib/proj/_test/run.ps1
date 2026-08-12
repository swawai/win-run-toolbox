[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

& (Join-Path $PSScriptRoot 'launcher-build.ps1')
$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
. (Join-Path $RepoRoot '_lib\proj\_toolchain\bootstrap-layout.ps1')
$Layout = Get-ProjBootstrapLayout
$CandidateArguments = @{
    LauncherPath = $Layout.LauncherCandidatePath
    CorePath = Join-Path $Layout.BuildRoot 'release\swawkit-proj.exe'
    HostPath = Join-Path $Layout.BuildRoot 'release\swawkit-proj-host.exe'
    ToolchainPath = Join-Path $Layout.BuildRoot (
        'release\swawkit-proj-toolchain.exe'
    )
}
& (Join-Path $PSScriptRoot 'launcher-runtime.ps1') @CandidateArguments
& (Join-Path $PSScriptRoot 'smoke-entry.ps1') @CandidateArguments
& (Join-Path $PSScriptRoot 'host-release.ps1') @CandidateArguments
& (Join-Path $PSScriptRoot 'claim-entry.ps1') @CandidateArguments
& (Join-Path $PSScriptRoot 'development-declaration.ps1')
& (Join-Path $PSScriptRoot 'development-command-layout.ps1')
& (Join-Path $PSScriptRoot 'command-export.ps1')
& (Join-Path $PSScriptRoot 'provider-state.ps1')
& (Join-Path $PSScriptRoot 'provider-activation.ps1')
& (Join-Path $PSScriptRoot 'project-build-export.ps1')
$TypeScriptTests = @(
    (Join-Path $RepoRoot '.swaw\proj\build\_lib\release-set.test.ts'),
    (Join-Path $RepoRoot '.swaw\proj\publish\_lib\runtime-release.test.ts')
)
& $RepoRoot\swawkit.exe .dev.bun test @TypeScriptTests
if ($LASTEXITCODE -ne 0) {
    throw "Proj Action TypeScript contract tests failed with exit code $LASTEXITCODE."
}
& (Join-Path $PSScriptRoot 'app-build.ps1')
& (Join-Path $PSScriptRoot 'app-publish.ps1')
& (Join-Path $PSScriptRoot 'app-core.ps1')
& (Join-Path $PSScriptRoot 'toolchain.ps1') `
    -ToolchainPath $CandidateArguments.ToolchainPath
& (Join-Path $PSScriptRoot 'toolchain.setup.ps1') `
    -ToolchainPath $CandidateArguments.ToolchainPath
& (Join-Path $PSScriptRoot 'web.ps1')
& (Join-Path $PSScriptRoot 'bootstrap-contract.ps1')
& (Join-Path $PSScriptRoot 'shell.ps1') @CandidateArguments
& (Join-Path $PSScriptRoot 'install-recovery.ps1')
& (Join-Path $PSScriptRoot 'command-event.ps1')
& (Join-Path $PSScriptRoot 'bun.ps1') `
    -ToolchainPath $CandidateArguments.ToolchainPath
& (Join-Path $PSScriptRoot 'pwsh.ps1')
& (Join-Path $PSScriptRoot 'msvc.ps1') `
    -ToolchainPath $CandidateArguments.ToolchainPath
& (Join-Path $PSScriptRoot 'msvc.command.ps1')
& (Join-Path $PSScriptRoot 'msvc.cache.ps1')
& (Join-Path $PSScriptRoot 'rust.ps1')
& (Join-Path $PSScriptRoot 'rust.strict.ps1')

Write-Host '[PASS] Proj test suite' -ForegroundColor Green
$global:LASTEXITCODE = 0
