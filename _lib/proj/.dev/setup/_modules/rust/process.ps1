Set-StrictMode -Version 2.0

function ConvertTo-ProjDevRustWindowsArgument {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$Value
    )

    if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') {
        return $Value
    }
    $Builder = [Text.StringBuilder]::new()
    [void]$Builder.Append('"')
    $Backslashes = 0
    foreach ($Character in $Value.ToCharArray()) {
        if ($Character -eq [char]'\') {
            $Backslashes++
            continue
        }
        if ($Character -eq [char]'"') {
            [void]$Builder.Append([char]'\', $Backslashes * 2 + 1)
            [void]$Builder.Append('"')
            $Backslashes = 0
            continue
        }
        if ($Backslashes -gt 0) {
            [void]$Builder.Append([char]'\', $Backslashes)
            $Backslashes = 0
        }
        [void]$Builder.Append($Character)
    }
    if ($Backslashes -gt 0) {
        [void]$Builder.Append([char]'\', $Backslashes * 2)
    }
    [void]$Builder.Append('"')
    return $Builder.ToString()
}

function ConvertTo-ProjDevRustWindowsArguments {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [AllowEmptyString()]
        [string[]]$Arguments
    )

    $Encoded = foreach ($Argument in $Arguments) {
        ConvertTo-ProjDevRustWindowsArgument -Value ([string]$Argument)
    }
    return [string]::Join(' ', [string[]]@($Encoded))
}

function Set-ProjDevRustProcessEnvironment {
    param(
        [Parameter(Mandatory = $true)][Diagnostics.ProcessStartInfo]$Info,
        [Parameter(Mandatory = $true)][string]$InstallRoot
    )

    # Windows PowerShell 5.1 exposes this dictionary lazily; the first read
    # initializes it before indexed access.
    $null = $Info.EnvironmentVariables
    $Info.EnvironmentVariables['CARGO_HOME'] = Join-Path $InstallRoot 'cargo'
    $Info.EnvironmentVariables['RUSTUP_HOME'] = Join-Path $InstallRoot 'rustup'
    foreach ($Name in Get-ProjDevRustAmbientOverrideNames) {
        if ($Info.EnvironmentVariables.ContainsKey($Name)) {
            $Info.EnvironmentVariables.Remove($Name)
        }
    }
}

function Invoke-ProjDevRustCapturedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [int]$TimeoutSeconds = 120
    )

    $Info = [Diagnostics.ProcessStartInfo]::new()
    $Info.FileName = $Executable
    $Info.Arguments = ConvertTo-ProjDevRustWindowsArguments `
        -Arguments $Arguments
    $Info.WorkingDirectory = $WorkingDirectory
    $Info.UseShellExecute = $false
    $Info.CreateNoWindow = $true
    $Info.RedirectStandardOutput = $true
    $Info.RedirectStandardError = $true
    Set-ProjDevRustProcessEnvironment `
        -Info $Info `
        -InstallRoot $InstallRoot
    $Process = [Diagnostics.Process]::Start($Info)
    if ($null -eq $Process) {
        throw "Failed to start Rust process: $Executable"
    }
    try {
        $OutputTask = $Process.StandardOutput.ReadToEndAsync()
        $ErrorTask = $Process.StandardError.ReadToEndAsync()
        if (-not $Process.WaitForExit($TimeoutSeconds * 1000)) {
            try { $Process.Kill() } catch {}
            try { [void]$Process.WaitForExit(5000) } catch {}
            throw "Rust process timed out after $TimeoutSeconds seconds."
        }
        $Process.WaitForExit()
        return [pscustomobject][ordered]@{
            ExitCode = [int]$Process.ExitCode
            Output = ([string]$OutputTask.Result).Trim()
            Error = ([string]$ErrorTask.Result).Trim()
        }
    } finally {
        $Process.Dispose()
    }
}

function Get-ProjDevRustProbe {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][string]$InstallRoot
    )

    $CargoBin = Join-Path $InstallRoot 'cargo\bin'
    $Rustup = Join-Path $CargoBin 'rustup.exe'
    foreach ($Path in @(
        $Rustup,
        (Join-Path $CargoBin 'rustc.exe'),
        (Join-Path $CargoBin 'cargo.exe')
    )) {
        if (-not [IO.File]::Exists($Path)) {
            throw "Rust installation is missing a proxy: $Path"
        }
    }
    $WorkingDirectory = $InstallRoot
    $ToolchainName = [string]$Definition.ToolchainName
    $RustupResult = Invoke-ProjDevRustCapturedProcess `
        -Executable $Rustup `
        -Arguments @('--version') `
        -WorkingDirectory $WorkingDirectory `
        -InstallRoot $InstallRoot
    $RustcResult = Invoke-ProjDevRustCapturedProcess `
        -Executable $Rustup `
        -Arguments @('run', $ToolchainName, 'rustc', '-Vv') `
        -WorkingDirectory $WorkingDirectory `
        -InstallRoot $InstallRoot
    $CargoResult = Invoke-ProjDevRustCapturedProcess `
        -Executable $Rustup `
        -Arguments @('run', $ToolchainName, 'cargo', '--version') `
        -WorkingDirectory $WorkingDirectory `
        -InstallRoot $InstallRoot
    $RustfmtResult = Invoke-ProjDevRustCapturedProcess `
        -Executable $Rustup `
        -Arguments @('run', $ToolchainName, 'rustfmt', '--version') `
        -WorkingDirectory $WorkingDirectory `
        -InstallRoot $InstallRoot
    foreach ($Result in @(
        $RustupResult,
        $RustcResult,
        $CargoResult,
        $RustfmtResult
    )) {
        if ($Result.ExitCode -ne 0) {
            throw "Rust installation probe failed: $($Result.Error)"
        }
    }

    $RustupMatch = [regex]::Match(
        $RustupResult.Output,
        '(?m)^rustup\s+(\d+\.\d+\.\d+(?:\S*)?)'
    )
    $ReleaseMatch = [regex]::Match(
        $RustcResult.Output,
        '(?m)^release:\s+(\S+)\s*$'
    )
    $CommitMatch = [regex]::Match(
        $RustcResult.Output,
        '(?m)^commit-hash:\s+([a-f0-9]{40})\s*$'
    )
    $HostMatch = [regex]::Match(
        $RustcResult.Output,
        '(?m)^host:\s+(\S+)\s*$'
    )
    $CargoMatch = [regex]::Match(
        $CargoResult.Output,
        '(?m)^cargo\s+(\d+\.\d+\.\d+(?:\S*)?)'
    )
    $RustfmtMatch = [regex]::Match(
        $RustfmtResult.Output,
        '(?m)^rustfmt\s+(\d+\.\d+\.\d+(?:\S*)?)'
    )
    if (-not $RustupMatch.Success -or
        -not $ReleaseMatch.Success -or
        -not $CommitMatch.Success -or
        -not $HostMatch.Success -or
        -not $CargoMatch.Success -or
        -not $RustfmtMatch.Success -or
        $HostMatch.Groups[1].Value -cne [string]$Definition.Host) {
        throw 'The installed Rust toolchain reported invalid identity data.'
    }
    return [pscustomobject][ordered]@{
        RustupVersion = $RustupMatch.Groups[1].Value
        RustcVersion = $ReleaseMatch.Groups[1].Value
        RustcCommit = $CommitMatch.Groups[1].Value
        CargoVersion = $CargoMatch.Groups[1].Value
        RustfmtVersion = $RustfmtMatch.Groups[1].Value
        Host = $HostMatch.Groups[1].Value
    }
}
