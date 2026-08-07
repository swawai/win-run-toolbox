Set-StrictMode -Version 2.0

# Bun installation recipe used only by .dev.setup.
function Invoke-ProjDevBunVersionProbe {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory
    )

    $StartInfo = [Diagnostics.ProcessStartInfo]::new()
    $StartInfo.FileName = $Executable
    $StartInfo.Arguments = '--version'
    $StartInfo.WorkingDirectory = $WorkingDirectory
    $StartInfo.UseShellExecute = $false
    $StartInfo.CreateNoWindow = $true
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true

    $Process = [Diagnostics.Process]::Start($StartInfo)
    if ($null -eq $Process) {
        throw "Failed to start the staged Bun executable: $Executable"
    }
    try {
        $OutputTask = $Process.StandardOutput.ReadToEndAsync()
        $ErrorTask = $Process.StandardError.ReadToEndAsync()
        if (-not $Process.WaitForExit(30000)) {
            try {
                $Process.Kill()
            } catch {
                # Preserve the timeout as the primary error.
            }
            try { [void]$Process.WaitForExit(5000) } catch {}
            throw 'The staged Bun version probe timed out after 30 seconds.'
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

function Install-ProjDevBun {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    Assert-ProjDevWindowsX64 -ToolName 'Bun'
    $Target = Get-ProjDevInstallRoot `
        -Context $Context `
        -Definition $Definition
    $Recovery = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target
    if ($Recovery.Ready) {
        return $false
    }
    if ([string]$Definition.Release.Provider -ceq 'github' -and
        -not [bool]$Definition.ReleaseResolved) {
        [void](Resolve-ProjDevBunRelease -Definition $Definition)
    }
    $Prepare = {
        param($StagedRoot)

        $BunxPath = Join-Path $StagedRoot 'bunx.cmd'
        $Content = "@echo off`r`n`"%~dp0bun.exe`" x %*`r`n"
        [IO.File]::WriteAllText(
            $BunxPath,
            $Content,
            [Text.UTF8Encoding]::new($false)
        )
    }
    $Validate = {
        param($ValidationContext, $ValidationDefinition, $InstallRoot)

        $Executable = Resolve-ProjDevChildPath `
            -Root $InstallRoot `
            -RelativePath ([string]$ValidationDefinition.Executable) `
            -Description 'Bun executable'
        $Probe = Invoke-ProjDevBunVersionProbe `
            -Executable $Executable `
            -WorkingDirectory $InstallRoot
        if ($Probe.ExitCode -ne 0) {
            throw (
                "Staged Bun version probe failed with exit code " +
                "$($Probe.ExitCode): $($Probe.Error)"
            )
        }
        if ($Probe.Output -cne [string]$ValidationDefinition.Version) {
            throw (
                "Staged Bun reports '$($Probe.Output)', expected " +
                "'$($ValidationDefinition.Version)'."
            )
        }
        return $true
    }
    return Install-ProjDevArchiveTool `
        -Context $Context `
        -Definition $Definition `
        -Prepare $Prepare `
        -Validate $Validate
}
