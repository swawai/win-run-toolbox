Set-StrictMode -Version 2.0

function ConvertTo-ProjDevWindowsArgument {
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

function ConvertTo-ProjDevWindowsArguments {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [AllowEmptyString()]
        [string[]]$Arguments
    )

    $Encoded = foreach ($Argument in $Arguments) {
        ConvertTo-ProjDevWindowsArgument -Value ([string]$Argument)
    }
    return [string]::Join(' ', [string[]]@($Encoded))
}

function Invoke-ProjDevConsoleProcess {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [AllowEmptyString()]
        [string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory
    )

    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw 'The managed console process adapter supports Windows only.'
    }
    $StartInfo = [Diagnostics.ProcessStartInfo]::new()
    $StartInfo.FileName = $Executable
    $StartInfo.Arguments = ConvertTo-ProjDevWindowsArguments `
        -Arguments $Arguments
    if ($StartInfo.Arguments.Length -gt 32000) {
        throw 'The child process arguments exceed the Windows command-line limit.'
    }
    $StartInfo.WorkingDirectory = $WorkingDirectory
    $StartInfo.UseShellExecute = $false
    $StartInfo.CreateNoWindow = $false

    $Process = [Diagnostics.Process]::Start($StartInfo)
    if ($null -eq $Process) {
        throw "Failed to start the console process: $Executable"
    }
    try {
        $Process.WaitForExit()
        return [int]$Process.ExitCode
    } finally {
        $Process.Dispose()
    }
}
