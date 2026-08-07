Set-StrictMode -Version 2.0

function Set-ProjDevRustupInstallerEnvironment {
    param(
        [Parameter(Mandatory = $true)][Diagnostics.ProcessStartInfo]$Info,
        [Parameter(Mandatory = $true)][string]$InstallRoot
    )

    foreach ($RelativeRoot in @('cargo', 'rustup')) {
        $Root = Join-Path $InstallRoot $RelativeRoot
        if (-not [IO.Directory]::Exists($Root)) {
            throw "Rust staging directory is missing: $Root"
        }
        $Unexpected = [IO.Directory]::EnumerateFileSystemEntries($Root) |
            Select-Object -First 1
        if ($null -ne $Unexpected) {
            throw "Rust staging root is not clean: $Unexpected"
        }
    }
    Set-ProjDevRustProcessEnvironment `
        -Info $Info `
        -InstallRoot $InstallRoot

    # rustup 1.29 creates settings.toml while loading a fresh configuration,
    # then its legacy existence check mistakes that file for an older install.
    # This staging root is unique and empty, so skip both existence
    # checks instead of exposing rustup's false-positive warning to the user.
    $Info.EnvironmentVariables['RUSTUP_INIT_SKIP_EXISTENCE_CHECKS'] = 'yes'
    if ($Info.EnvironmentVariables.ContainsKey(
        'RUSTUP_INIT_SKIP_PATH_CHECK'
    )) {
        $Info.EnvironmentVariables.Remove('RUSTUP_INIT_SKIP_PATH_CHECK')
    }
}

function Invoke-ProjDevRustupInstaller {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][string]$InstallerPath,
        [Parameter(Mandatory = $true)][string]$InstallRoot
    )

    $Arguments = [Collections.Generic.List[string]]::new()
    foreach ($Argument in [string[]]@(
        '-y'
        '--default-host'
        [string]$Definition.Host
        '--no-modify-path'
        '--profile'
        [string]$Definition.Profile
        '--default-toolchain'
        [string]$Definition.Toolchain
    )) {
        $Arguments.Add($Argument)
    }
    foreach ($Component in [string[]]$Definition.RequiredComponents) {
        $Arguments.Add('--component')
        $Arguments.Add($Component)
    }

    $Info = [Diagnostics.ProcessStartInfo]::new()
    $Info.FileName = $InstallerPath
    $Info.Arguments = ConvertTo-ProjDevRustWindowsArguments `
        -Arguments ([string[]]$Arguments.ToArray())
    $Info.WorkingDirectory = $InstallRoot
    $Info.UseShellExecute = $false
    $Info.CreateNoWindow = $false
    Set-ProjDevRustupInstallerEnvironment `
        -Info $Info `
        -InstallRoot $InstallRoot
    $Process = [Diagnostics.Process]::Start($Info)
    if ($null -eq $Process) {
        throw 'Failed to start rustup-init.exe.'
    }
    try {
        if (-not $Process.WaitForExit(1800000)) {
            try { $Process.Kill() } catch {}
            try { [void]$Process.WaitForExit(5000) } catch {}
            throw 'rustup-init timed out after 30 minutes.'
        }
        if ($Process.ExitCode -ne 0) {
            throw "rustup-init exited with code $($Process.ExitCode)."
        }
    } finally {
        $Process.Dispose()
    }
}

function Install-ProjDevRust {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    Assert-ProjDevWindowsX64 -ToolName 'Rust'
    $Target = Get-ProjDevRustInstallRoot `
        -Context $Context `
        -Definition $Definition
    $ValidateInstalled = {
        param($ValidationContext, $ValidationDefinition, $InstallRoot)

        return Test-ProjDevRustInstalled `
            -Context $ValidationContext `
            -Definition $ValidationDefinition `
            -InstallRoot $InstallRoot
    }
    $Recovery = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target `
        -ValidateCandidate $ValidateInstalled
    if ($Recovery.Ready) {
        return $false
    }
    $Installer = Get-ProjDevVerifiedRustupInstaller `
        -Context $Context `
        -Definition $Definition
    $Parent = Split-Path -Path $Target -Parent
    [void][IO.Directory]::CreateDirectory($Parent)
    $StagedRoot = New-ProjDevInstallWorkPath `
        -TargetPath $Target `
        -Kind 'partial'
    [void][IO.Directory]::CreateDirectory($StagedRoot)
    [void][IO.Directory]::CreateDirectory((Join-Path $StagedRoot 'cargo'))
    [void][IO.Directory]::CreateDirectory((Join-Path $StagedRoot 'rustup'))
    try {
        Write-Host (
            "[STEP] Installing Rust $($Definition.Toolchain) " +
            "($($Definition.Profile))..."
        ) -ForegroundColor Cyan
        Invoke-ProjDevRustupInstaller `
            -Definition $Definition `
            -InstallerPath ([string]$Installer.Path) `
            -InstallRoot $StagedRoot
        $Probe = Get-ProjDevRustProbe `
            -Definition $Definition `
            -InstallRoot $StagedRoot
        Write-ProjDevRustMetadata `
            -Definition $Definition `
            -Probe $Probe `
            -InstallRoot $StagedRoot `
            -RustupInitSha256 ([string]$Installer.Sha256)
        if (-not (Test-ProjDevRustInstalled `
            -Context $Context `
            -Definition $Definition `
            -InstallRoot $StagedRoot)) {
            throw 'Staged Rust installation failed validation.'
        }
        Publish-ProjDevInstallDirectory `
            -Context $Context `
            -Definition $Definition `
            -StagedPath $StagedRoot `
            -TargetPath $Target `
            -ValidatePublished $ValidateInstalled
        return $true
    } finally {
        Remove-ProjDevInstallResidues `
            -Context $Context `
            -Paths @($StagedRoot) `
            -Activity 'cleaning Rust installation work data'
    }
}
