Set-StrictMode -Version 2.0

function Invoke-ProjDevPwshVersionProbe {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory
    )

    $StartInfo = [Diagnostics.ProcessStartInfo]::new()
    $StartInfo.FileName = $Executable
    $StartInfo.Arguments = '-Version'
    $StartInfo.WorkingDirectory = $WorkingDirectory
    $StartInfo.UseShellExecute = $false
    $StartInfo.CreateNoWindow = $true
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true

    $Process = [Diagnostics.Process]::Start($StartInfo)
    if ($null -eq $Process) {
        throw "Failed to start the staged PowerShell executable: $Executable"
    }
    try {
        $OutputTask = $Process.StandardOutput.ReadToEndAsync()
        $ErrorTask = $Process.StandardError.ReadToEndAsync()
        if (-not $Process.WaitForExit(30000)) {
            try { $Process.Kill() } catch {}
            try { [void]$Process.WaitForExit(5000) } catch {}
            throw 'The staged PowerShell version probe timed out after 30 seconds.'
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

function Install-ProjDevPwsh {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    Assert-ProjDevWindowsX64 -ToolName 'PowerShell'
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
        [void](Resolve-ProjDevPwshRelease -Definition $Definition)
    }
    $Validate = {
        param($ValidationContext, $ValidationDefinition, $InstallRoot)

        $Executable = Resolve-ProjDevChildPath `
            -Root $InstallRoot `
            -RelativePath ([string]$ValidationDefinition.Executable) `
            -Description 'PowerShell executable'
        $Probe = Invoke-ProjDevPwshVersionProbe `
            -Executable $Executable `
            -WorkingDirectory $InstallRoot
        if ($Probe.ExitCode -ne 0) {
            throw (
                'Staged PowerShell version probe failed with exit code ' +
                "$($Probe.ExitCode): $($Probe.Error)"
            )
        }
        $Match = [regex]::Match(
            $Probe.Output,
            '^PowerShell\s+(\S+)$'
        )
        if (-not $Match.Success -or
            $Match.Groups[1].Value -cne [string]$ValidationDefinition.Version) {
            throw (
                "Staged PowerShell reports '$($Probe.Output)', expected " +
                "'PowerShell $($ValidationDefinition.Version)'."
            )
        }
        return $true
    }
    return Install-ProjDevArchiveTool `
        -Context $Context `
        -Definition $Definition `
        -Validate $Validate
}
