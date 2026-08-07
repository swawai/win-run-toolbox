<#
.SYNOPSIS
  Stream this process's standard input to a remote command through OpenSSH.
#>

param(
    [Parameter(Mandatory = $true)][int]$Port,
    [Parameter(Mandatory = $true)][string]$RemoteHost,
    [Parameter(Mandatory = $true)][string]$RemoteUser,
    [AllowEmptyString()]
    [Parameter(Mandatory = $true)][string]$SshKeyPath,
    [Parameter(Mandatory = $true)]
    [ValidateRange(1, 32767)][int]$RemoteArgumentCount,
    [Parameter(Mandatory = $true)]
    [ValidateSet(
        'posix',
        'win.cmd',
        'win.powershell',
        'win.pwsh',
        'win.git-bash'
    )][string]$RemoteShell
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'ps_common.ps1')

function Get-RemoteKitStdinArguments {
    param([Parameter(Mandatory = $true)][int]$Count)

    $Result = New-Object 'Collections.Generic.List[string]'
    for ($Index = 1; $Index -le $Count; $Index++) {
        $Name = "REMOTE_KIT_STDIN_ARG_$Index"
        $Value = [Environment]::GetEnvironmentVariable($Name, 'Process')
        if ($null -eq $Value) {
            throw "Missing stdin remote argument: $Name"
        }
        $Result.Add($Value)
    }
    return $Result.ToArray()
}

try {
    $RemoteArguments = @(
        Get-RemoteKitStdinArguments -Count $RemoteArgumentCount
    )
    if ($RemoteArguments | Where-Object { $_ -match '[\r\n]' }) {
        throw 'stdin remote arguments cannot contain line breaks.'
    }
    $RemoteCommand = $RemoteArguments -join ' '
    if ($RemoteShell -eq 'win.cmd') {
        $RemoteCommand = "chcp 65001>nul & $RemoteCommand"
    } elseif ($RemoteShell -ne 'posix') {
        throw "Remote shell profile '$RemoteShell' is recognized but not implemented for stdin commands."
    }

    $Context = Initialize-RemoteKitContext `
        -Port $Port `
        -RemoteHost $RemoteHost `
        -RemoteUser $RemoteUser `
        -SshKeyPath $SshKeyPath `
        -ModuleRoot $PSScriptRoot `
        -UploadSubdir 'stdin_runner' `
        -QuietInfrastructureOutput
    $ExtraOptions = @($Context.SshCommandOpts | Where-Object {
        $_ -ne '-n' -and $_ -ne '-T'
    })
    $Arguments = @(Get-RemoteKitOpenSshBaseArgs) + @('-T') +
        @($ExtraOptions) + @(Get-RemoteKitOpenSshTargetArgs) +
        @($RemoteCommand)
    $Process = New-Object Diagnostics.Process
    $Started = $false
    try {
        $Process.StartInfo.FileName = $Context.SshExe
        $Process.StartInfo.Arguments = Join-RemoteKitProcessArguments $Arguments
        $Process.StartInfo.UseShellExecute = $false
        $Process.StartInfo.CreateNoWindow = $false
        $Process.StartInfo.RedirectStandardInput = $false
        $Process.StartInfo.RedirectStandardOutput = $false
        $Process.StartInfo.RedirectStandardError = $false
        $Process.Start() | Out-Null
        $Started = $true
        $Process.WaitForExit()
        exit $Process.ExitCode
    } finally {
        if ($Started -and -not $Process.HasExited) {
            try { $Process.Kill() } catch { }
        }
        $Process.Dispose()
    }
} catch {
    [Console]::Error.WriteLine("[ERROR] $($_.Exception.Message)")
    exit 1
}
