[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '.dev\setup\_lib\bootstrap.ps1')

function Assert-ProjRustTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "Rust test failed: $Message"
    }
}

function Assert-ProjRustThrows {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [Parameter(Mandatory = $true)][string]$Pattern
    )

    $Thrown = $false
    try {
        & $Action
    } catch {
        $Thrown = $_.Exception.Message -like $Pattern
    }
    Assert-ProjRustTest `
        -Condition $Thrown `
        -Message "expected failure matching '$Pattern'"
}

function New-ProjRustTestContext {
    param(
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$CacheDataRoot
    )

    return New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $DataRoot `
        -CacheDataRoot $CacheDataRoot
}

$EnvironmentSnapshot = @{}
$CurrentEnvironment = [Environment]::GetEnvironmentVariables('Process')
foreach ($Name in $CurrentEnvironment.Keys) {
    $EnvironmentSnapshot[[string]$Name] = [string]$CurrentEnvironment[$Name]
}
$TestBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TestBase)
$TemporaryRoot = Join-Path $TestBase (
    "swawkit-proj-rust-$([Guid]::NewGuid().ToString('N'))"
)

try {
    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    $FixtureRoot = Join-Path $TemporaryRoot 'fixture'
    [void][IO.Directory]::CreateDirectory($ProjectRoot)
    [void][IO.Directory]::CreateDirectory($FixtureRoot)

    $env:SWAWKIT_PROJ_RUST_MODE = 'rustup'
    $env:SWAWKIT_PROJ_RUST_PROFILE = 'minimal'
    $env:SWAWKIT_PROJ_RUST_HOST = 'x86_64-pc-windows-msvc'
    foreach ($Selector in @(
        'stable',
        'nightly',
        'nightly-2026-07-28',
        '1.88',
        '1.88.0'
    )) {
        $env:SWAWKIT_PROJ_RUST_TOOLCHAIN = $Selector
        $Candidate = Get-ProjDevRustDefinition
        Assert-ProjRustTest `
            -Condition (
                [string]$Candidate.Toolchain -ceq $Selector -and
                [string]$Candidate.ToolchainName -ceq
                    "$Selector-x86_64-pc-windows-msvc" -and
                @($Candidate.RequiredComponents).Count -eq 1 -and
                ([string[]]$Candidate.RequiredComponents)[0] -ceq 'rustfmt'
            ) `
            -Message "toolchain selector '$Selector' was not preserved"
    }
    $env:SWAWKIT_PROJ_RUST_TOOLCHAIN = '1.88.0-2026-07-28'
    Assert-ProjRustThrows `
        -Action { [void](Get-ProjDevRustDefinition) } `
        -Pattern '*must be stable, beta, nightly*'
    $env:SWAWKIT_PROJ_RUST_TOOLCHAIN = 'stable'
    $Definition = Get-ProjDevRustDefinition

    $FixtureInstaller = Join-Path $FixtureRoot 'rustup-init.exe'
    [IO.File]::WriteAllBytes(
        $FixtureInstaller,
        [Text.Encoding]::ASCII.GetBytes('rustup fixture executable')
    )
    $FixtureHash = Get-ProjDevFileSha256 -Path $FixtureInstaller
    $FixtureChecksum = Join-Path $FixtureRoot 'rustup-init.exe.sha256'
    [IO.File]::WriteAllText(
        $FixtureChecksum,
        "$FixtureHash  rustup-init.exe`n",
        [Text.Encoding]::ASCII
    )
    $Definition.RustupInitUrl = $FixtureInstaller
    $Definition.RustupInitChecksumUrl = $FixtureChecksum

    $BadChecksum = Join-Path $FixtureRoot 'bad.sha256'
    [IO.File]::WriteAllText(
        $BadChecksum,
        "$(('0' * 64))  rustup-init.exe`n",
        [Text.Encoding]::ASCII
    )
    $BadDefinition = Get-ProjDevRustDefinition
    $BadDefinition.RustupInitUrl = $FixtureInstaller
    $BadDefinition.RustupInitChecksumUrl = $BadChecksum
    $BadContext = New-ProjRustTestContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot (Join-Path $TemporaryRoot 'bad data') `
        -CacheDataRoot (Join-Path $TemporaryRoot 'shared cache')
    Assert-ProjRustThrows `
        -Action {
            [void](Get-ProjDevVerifiedRustupInstaller `
                -Context $BadContext `
                -Definition $BadDefinition)
        } `
        -Pattern '*SHA-256 verification failed*'

    $Context = New-ProjRustTestContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot (Join-Path $TemporaryRoot 'data root') `
        -CacheDataRoot (Join-Path $TemporaryRoot 'shared cache')
    $Installer = Get-ProjDevVerifiedRustupInstaller `
        -Context $Context `
        -Definition $Definition
    Assert-ProjRustTest `
        -Condition (
            [IO.File]::Exists([string]$Installer.Path) -and
            [string]$Installer.Sha256 -ceq $FixtureHash
        ) `
        -Message 'verified rustup-init was not cached'
    [IO.File]::Delete($FixtureInstaller)
    [IO.File]::Delete($FixtureChecksum)
    $CachedInstaller = Get-ProjDevVerifiedRustupInstaller `
        -Context $Context `
        -Definition $Definition
    Assert-ProjRustTest `
        -Condition (
            [string]$CachedInstaller.Path -ceq [string]$Installer.Path -and
            [string]$CachedInstaller.Sha256 -ceq $FixtureHash
        ) `
        -Message 'verified rustup-init cache was not reusable offline'

    [IO.File]::WriteAllBytes(
        $FixtureInstaller,
        [Text.Encoding]::ASCII.GetBytes('updated rustup fixture executable')
    )
    $UpdatedFixtureHash = Get-ProjDevFileSha256 -Path $FixtureInstaller
    [IO.File]::WriteAllText(
        $FixtureChecksum,
        "$UpdatedFixtureHash  rustup-init.exe`n",
        [Text.Encoding]::ASCII
    )
    [IO.File]::Delete([string]$CachedInstaller.Path)
    $RefreshedInstaller = Get-ProjDevVerifiedRustupInstaller `
        -Context $Context `
        -Definition $Definition
    Assert-ProjRustTest `
        -Condition (
            [string]$RefreshedInstaller.Sha256 -ceq
                $UpdatedFixtureHash -and
            (Get-ProjDevFileSha256 -Path (
                [string]$RefreshedInstaller.Path
            )) -ceq $UpdatedFixtureHash
        ) `
        -Message 'a stale checksum sidecar prevented cache self-repair'
    $FixtureHash = $UpdatedFixtureHash

    $TargetRoot = Get-ProjDevRustInstallRoot `
        -Context $Context `
        -Definition $Definition
    $StageRoot = Join-Path (
        Split-Path -Path $TargetRoot -Parent
    ) '.partial-fixture'
    foreach ($RelativePath in Get-ProjDevRustRequiredPaths `
        -ToolchainName ([string]$Definition.ToolchainName) `
        -HostTriple ([string]$Definition.Host) `
        -RequiredComponents ([string[]]$Definition.RequiredComponents)) {
        $Path = Resolve-ProjDevChildPath `
            -Root $StageRoot `
            -RelativePath $RelativePath `
            -Description 'Rust test file'
        [void][IO.Directory]::CreateDirectory(
            (Split-Path -Path $Path -Parent)
        )
        if ($RelativePath -ceq 'cargo\bin\rustup.exe') {
            [IO.File]::Copy(
                [string]$RefreshedInstaller.Path,
                $Path,
                $false
            )
        } else {
            [IO.File]::WriteAllText(
                $Path,
                "fixture:$RelativePath",
                [Text.UTF8Encoding]::new($false)
            )
        }
    }
    $DriverRelative = (
        "rustup\toolchains\$($Definition.ToolchainName)\" +
        'bin\rustc_driver-fixture.dll'
    )
    $StdRelative = (
        "rustup\toolchains\$($Definition.ToolchainName)\" +
        "lib\rustlib\$($Definition.Host)\lib\libstd-fixture.rlib"
    )
    foreach ($RelativePath in @($DriverRelative, $StdRelative)) {
        $Path = Resolve-ProjDevChildPath `
            -Root $StageRoot `
            -RelativePath $RelativePath `
            -Description 'Rust test component file'
        [void][IO.Directory]::CreateDirectory(
            (Split-Path -Path $Path -Parent)
        )
        [IO.File]::WriteAllText(
            $Path,
            "fixture:$RelativePath",
            [Text.UTF8Encoding]::new($false)
        )
    }
    $Probe = [pscustomobject][ordered]@{
        RustupVersion = '1.28.2'
        RustcVersion = '1.88.0'
        RustcCommit = 'a' * 40
        CargoVersion = '1.88.0'
        RustfmtVersion = '1.8.0-stable'
        Host = [string]$Definition.Host
    }
    Write-ProjDevRustMetadata `
        -Definition $Definition `
        -Probe $Probe `
        -InstallRoot $StageRoot `
        -RustupInitSha256 $FixtureHash
    Assert-ProjRustTest `
        -Condition (Test-ProjDevRustInstalled `
            -Context $Context `
            -Definition $Definition `
            -InstallRoot $StageRoot) `
        -Message 'a complete staged Rust installation was rejected'
    $Validator = {
        param($ValidationContext, $ValidationDefinition, $InstallRoot)

        return Test-ProjDevRustInstalled `
            -Context $ValidationContext `
            -Definition $ValidationDefinition `
            -InstallRoot $InstallRoot
    }
    Publish-ProjDevInstallDirectory `
        -Context $Context `
        -Definition $Definition `
        -StagedPath $StageRoot `
        -TargetPath $TargetRoot `
        -ValidatePublished $Validator
    Assert-ProjRustTest `
        -Condition (-not (Install-ProjDevRust `
            -Context $Context `
            -Definition $Definition)) `
        -Message 'healthy moving-channel setup did not remain pinned'

    $Plan = New-ProjDevEnvironmentPlan -Context $Context
    Add-ProjDevRustEnvironment `
        -Context $Context `
        -Definition $Definition `
        -Plan $Plan
    $Scripts = ConvertTo-ProjDevEnvironmentScripts -Plan $Plan
    Assert-ProjRustTest `
        -Condition (
            $Plan.PathPrefixes.Count -eq 1 -and
            [string]$Plan.PathPrefixes[0] -ceq
                (Join-Path $TargetRoot 'cargo\bin')
        ) `
        -Message 'generated Rust environment lost isolation or override clearing'
    $ClearedOverrides = @(Get-ProjDevRustAmbientOverrideNames |
        Where-Object { $_ -cne 'RUSTUP_TOOLCHAIN' })
    foreach ($Name in $ClearedOverrides) {
        Assert-ProjRustTest `
            -Condition (
                $Scripts.Cmd -like "*set `"$Name=`"*" -and
                $Scripts.Ps1 -like "*Env:$Name*"
            ) `
            -Message "generated environment retained ambient $Name"
    }
    Assert-ProjRustTest `
        -Condition (
            $Scripts.Cmd -like (
                "*RUSTUP_TOOLCHAIN=$($Definition.ToolchainName)*"
            ) -and
            $Scripts.Ps1 -like (
                "*RUSTUP_TOOLCHAIN*'$($Definition.ToolchainName)'*"
            )
        ) `
        -Message 'generated environment did not pin the Rust toolchain'
    Assert-ProjRustTest `
        -Condition (Publish-ProjDevEnvironmentScripts `
            -Context $Context `
            -Scripts $Scripts) `
        -Message 'Rust environment scripts were not published'
    $env:SWAWKIT_PROJ_BUN_MODE = 'disabled'
    [void](Publish-ProjDevEnvironmentState `
        -Context $Context `
        -GenerationId ([string]$Scripts.GenerationId))
    $env:SWAWKIT_PROJ_BUN_MODE = 'managed'
    $env:SWAWKIT_PROJ_BUN_VERSION = '9.9.9'
    [void](Import-ProjDevGeneratedEnvironment -Context $Context)
    Assert-ProjDevRustEnvironmentCurrent `
        -Context $Context `
        -Definition $Definition
    Assert-ProjRustTest `
        -Condition ([string]$env:RUSTUP_TOOLCHAIN -ceq
            [string]$Definition.ToolchainName) `
        -Message 'an unrelated Bun declaration change blocked Rust activation'
    foreach ($Name in Get-ProjDevRustAmbientOverrideNames) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            'foreign-override',
            'Process'
        )
    }
    . $Context.EnvPs1Path
    Assert-ProjDevRustEnvironmentCurrent `
        -Context $Context `
        -Definition $Definition
    $RetainedOverrides = @(foreach (
        $Name in $ClearedOverrides
    ) {
        if (-not [string]::IsNullOrWhiteSpace(
            [Environment]::GetEnvironmentVariable($Name, 'Process')
        )) {
            $Name
        }
    })
    Assert-ProjRustTest `
        -Condition (
            $RetainedOverrides.Count -eq 0 -and
            [string]$env:RUSTUP_TOOLCHAIN -ceq
                [string]$Definition.ToolchainName -and
            [string]$env:RUSTC -ceq (Join-Path $TargetRoot (
                "rustup\toolchains\$($Definition.ToolchainName)\bin\rustc.exe"
            ))
        ) `
        -Message "generated environment retained: $RetainedOverrides"

    $ProcessInfo = [Diagnostics.ProcessStartInfo]::new()
    $ProcessInfo.FileName = $env:ComSpec
    $ProcessInfo.UseShellExecute = $false
    $null = $ProcessInfo.EnvironmentVariables
    foreach ($Name in Get-ProjDevRustAmbientOverrideNames) {
        $ProcessInfo.EnvironmentVariables[$Name] = 'foreign'
    }
    Set-ProjDevRustProcessEnvironment `
        -Info $ProcessInfo `
        -InstallRoot $TargetRoot
    Assert-ProjRustTest `
        -Condition (
            @(
                Get-ProjDevRustAmbientOverrideNames |
                    Where-Object {
                        $ProcessInfo.EnvironmentVariables.ContainsKey($_)
                    }
            ).Count -eq 0 -and
            $ProcessInfo.EnvironmentVariables['CARGO_HOME'] -ceq
                (Join-Path $TargetRoot 'cargo') -and
            $ProcessInfo.EnvironmentVariables['RUSTUP_HOME'] -ceq
                (Join-Path $TargetRoot 'rustup')
        ) `
        -Message 'rustup child-process isolation is incorrect'

    $InstallerRoot = Join-Path $TemporaryRoot 'installer environment'
    [void][IO.Directory]::CreateDirectory((Join-Path $InstallerRoot 'cargo'))
    [void][IO.Directory]::CreateDirectory((Join-Path $InstallerRoot 'rustup'))
    $InstallerInfo = [Diagnostics.ProcessStartInfo]::new()
    $InstallerInfo.FileName = $env:ComSpec
    $InstallerInfo.UseShellExecute = $false
    $null = $InstallerInfo.EnvironmentVariables
    $InstallerInfo.EnvironmentVariables['RUSTUP_INIT_SKIP_PATH_CHECK'] = 'yes'
    Set-ProjDevRustupInstallerEnvironment `
        -Info $InstallerInfo `
        -InstallRoot $InstallerRoot
    Assert-ProjRustTest `
        -Condition (
            $InstallerInfo.EnvironmentVariables[
                'RUSTUP_INIT_SKIP_EXISTENCE_CHECKS'
            ] -ceq 'yes' -and
            -not $InstallerInfo.EnvironmentVariables.ContainsKey(
                'RUSTUP_INIT_SKIP_PATH_CHECK'
            )
        ) `
        -Message 'rustup installer existence checks were not isolated'
    [IO.File]::WriteAllText(
        (Join-Path $InstallerRoot 'rustup\settings.toml'),
        'stale'
    )
    Assert-ProjRustThrows `
        -Action {
            Set-ProjDevRustupInstallerEnvironment `
                -Info $InstallerInfo `
                -InstallRoot $InstallerRoot
        } `
        -Pattern '*Rust staging root is not clean*'

    $ExtraPath = Join-Path $TargetRoot (
        "rustup\toolchains\$($Definition.ToolchainName)\bin\untracked.dll"
    )
    [IO.File]::WriteAllText($ExtraPath, 'unexpected')
    Assert-ProjRustTest `
        -Condition (-not (Test-ProjDevRustInstalled `
            -Context $Context -Definition $Definition)) `
        -Message 'an untracked Rust toolchain file was not detected'
    [IO.File]::Delete($ExtraPath)

    $RustfmtRelative = (
        "rustup\toolchains\$($Definition.ToolchainName)\bin\rustfmt.exe"
    )
    [IO.File]::Delete((Join-Path $TargetRoot $RustfmtRelative))
    Assert-ProjRustTest `
        -Condition (-not (Test-ProjDevRustInstalled `
            -Context $Context `
            -Definition $Definition)) `
        -Message 'missing required rustfmt component was not detected'

    Write-Host '[PASS] Proj Rust module test' -ForegroundColor Green
} finally {
    $CurrentNames = @(
        [Environment]::GetEnvironmentVariables('Process').Keys |
            ForEach-Object { [string]$_ }
    )
    foreach ($Name in $CurrentNames) {
        if (-not $EnvironmentSnapshot.ContainsKey($Name)) {
            [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
        }
    }
    foreach ($Name in $EnvironmentSnapshot.Keys) {
        [Environment]::SetEnvironmentVariable(
            [string]$Name,
            [string]$EnvironmentSnapshot[$Name],
            'Process'
        )
    }
    $ResolvedRoot = [IO.Path]::GetFullPath($TemporaryRoot)
    $AllowedPrefix = $TestBase.TrimEnd('\') + '\'
    if ($ResolvedRoot.StartsWith(
        $AllowedPrefix,
        [StringComparison]::OrdinalIgnoreCase
    ) -and
        [IO.Path]::GetFileName($ResolvedRoot).StartsWith(
            'swawkit-proj-rust-',
            [StringComparison]::Ordinal
        ) -and
        [IO.Directory]::Exists($ResolvedRoot)) {
        Remove-Item -LiteralPath $ResolvedRoot -Recurse -Force
    }
}
