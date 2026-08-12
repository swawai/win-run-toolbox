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

function ConvertTo-RemoteKitPowerShellEncodedCommand {
    param([Parameter(Mandatory = $true)][string]$Source)

    return [Convert]::ToBase64String(
        [Text.Encoding]::Unicode.GetBytes($Source)
    )
}

function Copy-RemoteKitStandardInputToPayload {
    $Context = Get-RemoteKitContext
    [IO.Directory]::CreateDirectory($Context.UploadTempRoot) | Out-Null
    $PayloadPath = Join-Path $Context.UploadTempRoot (
        'swaw-kit-ssh-stdin-' + [Guid]::NewGuid().ToString('N') + '.bin'
    )
    $InputStream = [Console]::OpenStandardInput()
    $PayloadStream = $null
    try {
        $PayloadStream = New-Object IO.FileStream(
            $PayloadPath,
            [IO.FileMode]::CreateNew,
            [IO.FileAccess]::Write,
            [IO.FileShare]::None
        )
        $InputStream.CopyTo($PayloadStream)
        $PayloadStream.Flush()
        return $PayloadPath
    } catch {
        Remove-Item -LiteralPath $PayloadPath -Force -ErrorAction SilentlyContinue
        throw
    } finally {
        if ($null -ne $PayloadStream) {
            $PayloadStream.Dispose()
        }
    }
}

function New-RemoteKitWindowsStdinLoaderSource {
    param(
        [Parameter(Mandatory = $true)][string]$RemotePayloadName,
        [Parameter(Mandatory = $true)][string]$RemoteCommand
    )

    $CommandBase64 = [Convert]::ToBase64String(
        [Text.Encoding]::UTF8.GetBytes($RemoteCommand)
    )
    $Template = @'
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$payloadPath = Join-Path ([Environment]::GetFolderPath('UserProfile')) '__REMOTE_PAYLOAD_NAME__'
$exitCode = 1
try {
    $command = [Text.Encoding]::UTF8.GetString(
        [Convert]::FromBase64String('__REMOTE_COMMAND_BASE64__')
    )
    $process = New-Object Diagnostics.Process
    try {
        $process.StartInfo.FileName = $env:ComSpec
        $process.StartInfo.Arguments = '/d /s /c "chcp 65001>nul <nul & ' + $command + ' < "' + $payloadPath + '""'
        $process.StartInfo.UseShellExecute = $false
        $process.StartInfo.CreateNoWindow = $false
        $process.StartInfo.RedirectStandardInput = $false
        $process.StartInfo.RedirectStandardOutput = $false
        $process.StartInfo.RedirectStandardError = $false
        $process.Start() | Out-Null
        $process.WaitForExit()
        $exitCode = $process.ExitCode
    } finally {
        $process.Dispose()
    }
} catch {
    [Console]::Error.WriteLine('[ERROR] Windows stdin loader: ' + $_.Exception.Message)
} finally {
    Remove-Item -LiteralPath $payloadPath -Force -ErrorAction SilentlyContinue
}
exit $exitCode
'@
    return $Template.Replace(
        '__REMOTE_PAYLOAD_NAME__',
        $RemotePayloadName
    ).Replace(
        '__REMOTE_COMMAND_BASE64__',
        $CommandBase64
    )
}

function Invoke-RemoteKitSshWithInheritedHandles {
    param([Parameter(Mandatory = $true)][object[]]$Arguments)

    $Context = Get-RemoteKitContext
    Write-RemoteKitInfrastructureLog (
        '[DEBUG] ssh command: ' +
        (Format-RemoteKitCommandForLog $Context.SshExe $Arguments)
    )
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
        return $Process.ExitCode
    } finally {
        if ($Started -and -not $Process.HasExited) {
            try { $Process.Kill() } catch { }
        }
        $Process.Dispose()
    }
}

function Invoke-RemoteKitPosixStdinCommand {
    param([Parameter(Mandatory = $true)][string]$RemoteCommand)

    $Context = Get-RemoteKitContext
    $ExtraOptions = @($Context.SshCommandOpts | Where-Object {
        $_ -ne '-n' -and $_ -ne '-T'
    })
    $Arguments = @(Get-RemoteKitOpenSshBaseArgs) + @('-T') +
        @($ExtraOptions) + @(Get-RemoteKitOpenSshTargetArgs) +
        @($RemoteCommand)
    return Invoke-RemoteKitSshWithInheritedHandles -Arguments $Arguments
}

function Invoke-RemoteKitWindowsCmdStdinCommand {
    param([Parameter(Mandatory = $true)][string]$RemoteCommand)

    $Context = Get-RemoteKitContext
    $LocalPayloadPath = $null
    $RemotePayloadName = (
        '.swaw-kit-ssh-stdin-' + [Guid]::NewGuid().ToString('N') + '.bin'
    )
    try {
        $LocalPayloadPath = Copy-RemoteKitStandardInputToPayload
        $ScpArgs = @(Get-RemoteKitOpenSshBaseArgs) +
            @('-o', 'BatchMode=yes')
        if (-not $Context.UseSshConfigHost) {
            $ScpArgs += @('-P', [string]$Context.Port)
        }
        $ScpArgs += @(
            $LocalPayloadPath,
            ($Context.RemoteTarget + ':' + $RemotePayloadName)
        )
        $ScpExitCode = Invoke-RemoteKitLoggedCommand `
            -Label 'stdin payload upload' `
            -ExePath 'scp.exe' `
            -Arguments $ScpArgs `
            -OutputOnlyOnError
        if ($ScpExitCode -ne 0) {
            throw "stdin payload upload failed with exit code $ScpExitCode."
        }

        $LoaderSource = New-RemoteKitWindowsStdinLoaderSource `
            -RemotePayloadName $RemotePayloadName `
            -RemoteCommand $RemoteCommand
        $LoaderCommand = (
            'powershell.exe -NoLogo -NoProfile -NonInteractive ' +
            '-ExecutionPolicy Bypass -EncodedCommand ' +
            (ConvertTo-RemoteKitPowerShellEncodedCommand $LoaderSource)
        )
        $SshArgs = @(Get-RemoteKitOpenSshBaseArgs) +
            @($Context.SshCommandOpts) +
            @(Get-RemoteKitOpenSshTargetArgs) +
            @($LoaderCommand)
        return Invoke-RemoteKitSshWithInheritedHandles -Arguments $SshArgs
    } finally {
        Remove-RemoteKitTempPath $LocalPayloadPath
    }
}

try {
    $RemoteArguments = @(
        Get-RemoteKitStdinArguments -Count $RemoteArgumentCount
    )
    if ($RemoteArguments | Where-Object { $_ -match '[\r\n]' }) {
        throw 'stdin remote arguments cannot contain line breaks.'
    }
    $RemoteCommand = $RemoteArguments -join ' '
    if ($RemoteShell -notin @('posix', 'win.cmd')) {
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
    if ($RemoteShell -eq 'win.cmd') {
        $ExitCode = Invoke-RemoteKitWindowsCmdStdinCommand `
            -RemoteCommand $RemoteCommand
    } else {
        $ExitCode = Invoke-RemoteKitPosixStdinCommand `
            -RemoteCommand $RemoteCommand
    }
    exit $ExitCode
} catch {
    [Console]::Error.WriteLine("[ERROR] $($_.Exception.Message)")
    exit 1
}
