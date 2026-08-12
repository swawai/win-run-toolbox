<#
.SYNOPSIS
  PowerShell runtime for ssh_remote_kit entry commands.

.DESCRIPTION
  kit.cmd forwards its complete argument text here once. This runtime owns
  argument parsing so free-form values are never read through CMD's %0..%9
  parameters, which treat '=' as a delimiter.
#>

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Set-StrictMode -Version 2.0

$script:ForwardedArguments = @($args | ForEach-Object { [string]$_ })
$script:KitRepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$script:ExitCode = 0

. (Join-Path $PSScriptRoot 'ps_common.ps1')
. (Join-Path $PSScriptRoot 'ssh_config.ps1')

function Get-RemoteKitForwardedInvocation {
    if ($script:ForwardedArguments.Count -lt 4) {
        throw 'Remote Kit requires four fixed connection arguments.'
    }

    $CommandArguments = if ($script:ForwardedArguments.Count -gt 4) {
        @($script:ForwardedArguments[4..(
            $script:ForwardedArguments.Count - 1
        )])
    } else {
        @()
    }
    return [pscustomobject]@{
        PortText         = $script:ForwardedArguments[0]
        RemoteHost      = $script:ForwardedArguments[1]
        RemoteUser      = $script:ForwardedArguments[2]
        SshKeyPath      = $script:ForwardedArguments[3]
        CommandArguments = $CommandArguments
    }
}

function Set-RemoteKitProcessEnvironment {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [AllowNull()][string]$Value
    )

    [Environment]::SetEnvironmentVariable($Name, $Value, 'Process')
}

function Resolve-RemoteKitPort {
    param([AllowNull()][AllowEmptyString()][string]$Value)

    $Port = [int]0
    if (-not [int]::TryParse($Value, [ref]$Port) -or
        $Port -lt 1 -or
        $Port -gt 65535) {
        throw 'SSH port must be a decimal integer between 1 and 65535.'
    }
    return $Port
}

function New-RemoteKitRuntime {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Invocation,
        [ValidateSet('write', 'install')][string]$EmbeddedConfigAction = 'write'
    )

    $EntryFile = [Environment]::GetEnvironmentVariable(
        'REMOTE_KIT_ENTRY_FILE',
        'Process'
    )
    $ExternalHost = [Environment]::GetEnvironmentVariable(
        'REMOTE_SSH_HOST',
        'Process'
    )
    $ExternalConfig = [Environment]::GetEnvironmentVariable(
        'REMOTE_SSH_CONFIG',
        'Process'
    )
    $IsEmbedded = -not [string]::IsNullOrWhiteSpace($EntryFile)
    $IsExternalConfig = -not $IsEmbedded -and
        -not [string]::IsNullOrWhiteSpace($ExternalHost)
    $RemoteShell = 'posix'

    if ($IsEmbedded) {
        if ([string]::IsNullOrWhiteSpace($env:USERPROFILE)) {
            throw 'USERPROFILE is required for embedded SSH configuration.'
        }
        $Config = if ($EmbeddedConfigAction -eq 'install') {
            Install-RemoteKitEmbeddedSshConfig `
                -EntryFile $EntryFile `
                -RepoRoot $script:KitRepoRoot `
                -UserProfile $env:USERPROFILE
        } else {
            Write-RemoteKitEmbeddedSshConfig `
                -EntryFile $EntryFile `
                -RepoRoot $script:KitRepoRoot `
                -UserProfile $env:USERPROFILE
        }
        Set-RemoteKitProcessEnvironment `
            -Name 'REMOTE_KIT_SSH_CONFIG_PATH' `
            -Value $Config.ConfigPath
        Set-RemoteKitProcessEnvironment `
            -Name 'REMOTE_KIT_SSH_HOST' `
            -Value $Config.HostAlias
        $Port = 0
        $RemoteHost = $Config.HostAlias
        $RemoteUser = $Config.HostAlias
        $SshKeyPath = ''
        $RemoteShell = $Config.RemoteShell
    } elseif ($IsExternalConfig) {
        if ([string]::IsNullOrWhiteSpace($ExternalConfig)) {
            throw 'REMOTE_SSH_CONFIG must be set when REMOTE_SSH_HOST is set.'
        }
        if ($ExternalConfig -eq 'embedded') {
            throw 'REMOTE_KIT_ENTRY_FILE is required for embedded SSH configuration.'
        }
        Set-RemoteKitProcessEnvironment `
            -Name 'REMOTE_KIT_SSH_CONFIG_PATH' `
            -Value $ExternalConfig
        Set-RemoteKitProcessEnvironment `
            -Name 'REMOTE_KIT_SSH_HOST' `
            -Value $ExternalHost
        $Port = 0
        $RemoteHost = $ExternalHost
        $RemoteUser = $ExternalHost
        $SshKeyPath = ''
    } else {
        Set-RemoteKitProcessEnvironment -Name 'REMOTE_KIT_SSH_CONFIG_PATH' -Value $null
        Set-RemoteKitProcessEnvironment -Name 'REMOTE_KIT_SSH_HOST' -Value $null
        $Port = Resolve-RemoteKitPort -Value $Invocation.PortText
        if ([string]::IsNullOrWhiteSpace($Invocation.RemoteHost)) {
            throw 'SSH host must not be empty.'
        }
        if ([string]::IsNullOrWhiteSpace($Invocation.RemoteUser)) {
            throw 'SSH user must not be empty.'
        }
        $RemoteHost = $Invocation.RemoteHost
        $RemoteUser = $Invocation.RemoteUser
        $SshKeyPath = if ([string]::IsNullOrWhiteSpace($Invocation.SshKeyPath)) {
            Join-Path $env:USERPROFILE '.ssh\id_rsa'
        } else {
            $Invocation.SshKeyPath
        }
    }

    $Context = Initialize-RemoteKitContext `
        -Port $Port `
        -RemoteHost $RemoteHost `
        -RemoteUser $RemoteUser `
        -SshKeyPath $SshKeyPath `
        -ModuleRoot $PSScriptRoot `
        -UploadSubdir 'kit'
    $VscodeAuthority = if ($Context.UseSshConfigHost) {
        'ssh-remote+' + $Context.RemoteTarget
    } else {
        'ssh-remote+' + $RemoteUser + '@' + $RemoteHost + ':' + $Port
    }
    return [pscustomobject]@{
        Context         = $Context
        EntryFile       = $EntryFile
        IsEmbedded      = $IsEmbedded
        RemoteShell     = $RemoteShell
        Port            = $Port
        RemoteHost      = $RemoteHost
        RemoteUser      = $RemoteUser
        SshKeyPath      = $SshKeyPath
        VscodeAuthority = $VscodeAuthority
    }
}

function Get-RemoteKitApplicationPath {
    param([Parameter(Mandatory = $true)][string]$Name)

    return [string](Get-Command `
        -Name $Name `
        -CommandType Application `
        -ErrorAction Stop |
        Select-Object -First 1).Source
}

function Invoke-RemoteKitInheritedCommand {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][object[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$Label
    )

    Write-RemoteKitInfrastructureLog (
        '[DEBUG] ' + $Label + ': ' +
        (Format-RemoteKitCommandForLog $FilePath $Arguments)
    )
    & $FilePath @Arguments
    $script:ExitCode = $LASTEXITCODE
}

function Get-RemoteKitTtyOptions {
    $Options = @(Split-RemoteKitOptionString $env:REMOTE_KIT_SSH_TTY_OPTS)
    if ($Options.Count -eq 0) {
        $Options = @(
            '-tt',
            '-o', 'BatchMode=yes',
            '-o', 'ServerAliveInterval=60',
            '-o', 'ServerAliveCountMax=3'
        )
    }
    return $Options
}

function Join-RemoteKitRemoteCommand {
    param(
        [Parameter(Mandatory = $true)][object[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$RemoteShell
    )

    if ($Arguments.Count -eq 0) {
        throw 'A remote command is required.'
    }
    if ($Arguments | Where-Object { [string]$_ -match '[\r\n]' }) {
        throw 'Remote command arguments cannot contain line breaks.'
    }
    $Command = @($Arguments | ForEach-Object { [string]$_ }) -join ' '
    if ($RemoteShell -eq 'win.cmd') {
        return 'chcp 65001>nul & ' + $Command
    }
    if ($RemoteShell -ne 'posix') {
        throw (
            "Remote shell profile '$RemoteShell' is recognized but not " +
            'implemented for remote commands.'
        )
    }
    return $Command
}

function Invoke-RemoteKitRemoteCommand {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Runtime,
        [Parameter(Mandatory = $true)][object[]]$CommandArguments,
        [switch]$Tty
    )

    $RemoteCommand = Join-RemoteKitRemoteCommand `
        -Arguments $CommandArguments `
        -RemoteShell $Runtime.RemoteShell
    $SshOptions = if ($Tty) {
        @(Get-RemoteKitTtyOptions)
    } else {
        @($Runtime.Context.SshCommandOpts)
    }
    $Arguments = @(Get-RemoteKitOpenSshBaseArgs) +
        $SshOptions +
        @(Get-RemoteKitOpenSshTargetArgs) +
        @($RemoteCommand)
    return Invoke-RemoteKitInheritedCommand `
        -FilePath (Get-RemoteKitApplicationPath 'ssh') `
        -Arguments $Arguments `
        -Label 'ssh command'
}

function Invoke-RemoteKitChildPowerShell {
    param(
        [Parameter(Mandatory = $true)][string]$ScriptPath,
        [Parameter(Mandatory = $true)][object[]]$Arguments
    )

    $PowerShellArguments = @(
        '-NoLogo',
        '-NoProfile',
        '-ExecutionPolicy', 'Bypass',
        '-File', $ScriptPath
    ) + @($Arguments)
    $Process = New-Object Diagnostics.Process
    $Started = $false
    try {
        $Process.StartInfo.FileName = Join-Path $PSHOME 'powershell.exe'
        $Process.StartInfo.Arguments = Join-RemoteKitProcessArguments `
            -Arguments $PowerShellArguments
        $Process.StartInfo.UseShellExecute = $false
        $Process.StartInfo.CreateNoWindow = $false
        $Process.StartInfo.RedirectStandardInput = $false
        $Process.StartInfo.RedirectStandardOutput = $false
        $Process.StartInfo.RedirectStandardError = $false
        $Process.Start() | Out-Null
        $Started = $true
        $Process.WaitForExit()
        $script:ExitCode = $Process.ExitCode
    } finally {
        if ($Started -and -not $Process.HasExited) {
            try { $Process.Kill() } catch { }
        }
        $Process.Dispose()
    }
}

function Invoke-RemoteKitScriptVerb {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Runtime,
        [Parameter(Mandatory = $true)][object[]]$Arguments
    )

    if ($Arguments.Count -lt 1) {
        throw 'script requires a local script path.'
    }
    if ([string]::IsNullOrWhiteSpace([string]$Arguments[0])) {
        throw 'script path must not be empty.'
    }
    $ScriptArguments = @(
        if ($Arguments.Count -gt 1) {
            $Arguments[1..($Arguments.Count - 1)]
        }
    )
    Set-RemoteKitProcessEnvironment `
        -Name 'REMOTE_KIT_SCRIPT_ARG_COUNT' `
        -Value ([string]$ScriptArguments.Count)
    for ($Index = 0; $Index -lt $ScriptArguments.Count; $Index++) {
        Set-RemoteKitProcessEnvironment `
            -Name ('REMOTE_KIT_SCRIPT_ARG_' + ($Index + 1)) `
            -Value ([string]$ScriptArguments[$Index])
    }
    return Invoke-RemoteKitChildPowerShell `
        -ScriptPath (Join-Path $PSScriptRoot 'script_runner.ps1') `
        -Arguments @(
            '-Port', $Runtime.Port,
            '-RemoteHost', $Runtime.RemoteHost,
            '-RemoteUser', $Runtime.RemoteUser,
            '-SshKeyPath', $Runtime.SshKeyPath,
            '-ScriptPath', [string]$Arguments[0]
        )
}

function Invoke-RemoteKitStdinVerb {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Runtime,
        [Parameter(Mandatory = $true)][object[]]$Arguments
    )

    if ($Arguments.Count -lt 2 -or [string]$Arguments[0] -ne '--') {
        throw 'stdin requires -- followed by a remote command.'
    }
    $RemoteArguments = @($Arguments[1..($Arguments.Count - 1)])
    Set-RemoteKitProcessEnvironment `
        -Name 'REMOTE_KIT_STDIN_ARG_COUNT' `
        -Value ([string]$RemoteArguments.Count)
    for ($Index = 0; $Index -lt $RemoteArguments.Count; $Index++) {
        Set-RemoteKitProcessEnvironment `
            -Name ('REMOTE_KIT_STDIN_ARG_' + ($Index + 1)) `
            -Value ([string]$RemoteArguments[$Index])
    }
    return Invoke-RemoteKitChildPowerShell `
        -ScriptPath (Join-Path $PSScriptRoot 'stdin_runner.ps1') `
        -Arguments @(
            '-Port', $Runtime.Port,
            '-RemoteHost', $Runtime.RemoteHost,
            '-RemoteUser', $Runtime.RemoteUser,
            '-SshKeyPath', $Runtime.SshKeyPath,
            '-RemoteArgumentCount', $RemoteArguments.Count,
            '-RemoteShell', $Runtime.RemoteShell
        )
}

function Get-RemoteKitScpBaseArgs {
    param([Parameter(Mandatory = $true)][pscustomobject]$Runtime)

    $Arguments = @(Get-RemoteKitOpenSshBaseArgs)
    if (-not $Runtime.Context.UseSshConfigHost) {
        $Arguments += @('-P', [string]$Runtime.Port)
    }
    return $Arguments
}

function Invoke-RemoteKitCopyVerb {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Runtime,
        [Parameter(Mandatory = $true)][object[]]$Arguments
    )

    if ($Arguments.Count -ne 2) {
        throw 'copy requires exactly two paths.'
    }
    $Source = [string]$Arguments[0]
    $Destination = [string]$Arguments[1]
    if ($Source.Length -eq 0 -or $Destination.Length -eq 0) {
        throw 'copy paths must not be empty.'
    }
    $SourceRemote = $Source.StartsWith(':')
    $DestinationRemote = $Destination.StartsWith(':')
    if (-not $SourceRemote -and -not $DestinationRemote) {
        throw 'copy requires at least one remote path beginning with a colon.'
    }
    $Target = $Runtime.Context.RemoteTarget
    $ScpArguments = @(Get-RemoteKitScpBaseArgs -Runtime $Runtime)
    if ($SourceRemote -and $DestinationRemote) {
        $RemoteSource = $Source.Substring(1)
        $RemoteDestination = $Destination.Substring(1)
        if ($RemoteSource.Length -eq 0 -or $RemoteDestination.Length -eq 0) {
            throw 'Remote copy paths must not be empty.'
        }
        Write-Host "scp -3: `"${Target}:$RemoteSource`"  to  `"${Target}:$RemoteDestination`""
        $ScpArguments = @('-3') + $ScpArguments + @(
            '-r',
            "${Target}:$RemoteSource",
            "${Target}:$RemoteDestination"
        )
    } elseif ($SourceRemote) {
        $RemoteSource = $Source.Substring(1)
        if ($RemoteSource.Length -eq 0) {
            throw 'Remote source path must not be empty.'
        }
        Write-Host "scp: `"${Target}:$RemoteSource`"  to  `"$Destination`""
        $ScpArguments += @('-r', "${Target}:$RemoteSource", $Destination)
    } else {
        $RemoteDestination = $Destination.Substring(1)
        if ($RemoteDestination.Length -eq 0) {
            throw 'Remote destination path must not be empty.'
        }
        Write-Host "scp: `"$Source`"  to  `"${Target}:$RemoteDestination`""
        $ScpArguments += @('-r', $Source, "${Target}:$RemoteDestination")
    }
    return Invoke-RemoteKitInheritedCommand `
        -FilePath (Get-RemoteKitApplicationPath 'scp') `
        -Arguments $ScpArguments `
        -Label 'scp command'
}

function Get-RemoteKitRemoteHome {
    param([Parameter(Mandatory = $true)][pscustomobject]$Runtime)

    $Arguments = @(Get-RemoteKitOpenSshBaseArgs) +
        @($Runtime.Context.SshCommandOpts) +
        @(Get-RemoteKitOpenSshTargetArgs) +
        @('echo $HOME')
    $Output = @(& (Get-RemoteKitApplicationPath 'ssh') @Arguments)
    $ExitCode = $LASTEXITCODE
    if ($ExitCode -ne 0) {
        throw "Failed to read remote HOME; ssh exited with $ExitCode."
    }
    $Home = @($Output | Where-Object {
        -not [string]::IsNullOrWhiteSpace([string]$_)
    } | Select-Object -First 1)
    if ($Home.Count -ne 1 -or [string]$Home[0] -eq '$HOME') {
        throw 'Failed to read remote $HOME. Check that the host is online and Unix-like.'
    }
    return [string]$Home[0]
}

function Resolve-RemoteKitEditorRemotePath {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Runtime,
        [Parameter(Mandatory = $true)][string]$Value
    )

    $RemoteInput = if ($Value.StartsWith(':')) {
        $Value.Substring(1)
    } else {
        $Value
    }
    if ($RemoteInput.Length -eq 0) {
        throw 'Remote path must not be empty.'
    }
    if ($RemoteInput.StartsWith('/')) {
        return $RemoteInput
    }
    return (Get-RemoteKitRemoteHome -Runtime $Runtime).TrimEnd('/') + '/' +
        $RemoteInput
}

function Invoke-RemoteKitEditorLaunch {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('code', 'cursor')]
        [string]$Tool,
        [Parameter(Mandatory = $true)][string]$Target,
        [AllowEmptyString()][string]$RemoteAuthority
    )

    $ReuseBootstrap = [string]::Equals(
        $env:WIN_RUN_EDITOR_BOOTSTRAP,
        $Tool,
        [StringComparison]::OrdinalIgnoreCase
    )
    Set-RemoteKitProcessEnvironment -Name 'WIN_RUN_EDITOR_BOOTSTRAP' -Value $null
    Set-RemoteKitProcessEnvironment `
        -Name 'WIN_RUN_REMOTE_EDITOR_TARGET' `
        -Value $Target
    Set-RemoteKitProcessEnvironment `
        -Name 'WIN_RUN_REMOTE_EDITOR_AUTHORITY' `
        -Value $RemoteAuthority
    try {
        $Arguments = @('-Tool', $Tool)
        if ($ReuseBootstrap) {
            $Arguments += '-ReuseBootstrapWindow'
        }
        return Invoke-RemoteKitChildPowerShell `
            -ScriptPath (Join-Path $PSScriptRoot 'editor-launch.ps1') `
            -Arguments $Arguments
    } finally {
        Set-RemoteKitProcessEnvironment `
            -Name 'WIN_RUN_REMOTE_EDITOR_TARGET' `
            -Value $null
        Set-RemoteKitProcessEnvironment `
            -Name 'WIN_RUN_REMOTE_EDITOR_AUTHORITY' `
            -Value $null
    }
}

function Invoke-RemoteKitEditorVerb {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Runtime,
        [Parameter(Mandatory = $true)][ValidateSet('code', 'cursor')]
        [string]$Tool,
        [Parameter(Mandatory = $true)][object[]]$Arguments
    )

    if ($Arguments.Count -eq 1) {
        $RemotePath = Resolve-RemoteKitEditorRemotePath `
            -Runtime $Runtime `
            -Value ([string]$Arguments[0])
        return Invoke-RemoteKitEditorLaunch `
            -Tool $Tool `
            -Target $RemotePath `
            -RemoteAuthority $Runtime.VscodeAuthority
    }
    if ($Arguments.Count -ne 2) {
        throw "$Tool requires one remote path or one remote and one local path."
    }

    $First = [string]$Arguments[0]
    $Second = [string]$Arguments[1]
    if ($First.StartsWith(':') -eq $Second.StartsWith(':')) {
        throw 'SFTP setup requires exactly one remote path and one local path.'
    }
    if ($First.StartsWith(':')) {
        $RemoteArgument = $First
        $LocalPath = $Second
    } else {
        $LocalPath = $First
        $RemoteArgument = $Second
    }
    if ($LocalPath.Length -eq 0) {
        throw 'Local sync directory must not be empty.'
    }
    $RemotePath = Resolve-RemoteKitEditorRemotePath `
        -Runtime $Runtime `
        -Value $RemoteArgument
    $LocalPath = [IO.Path]::GetFullPath(
        [Environment]::ExpandEnvironmentVariables($LocalPath)
    )
    if ([IO.File]::Exists($LocalPath)) {
        throw "Local sync path exists but is not a directory: $LocalPath"
    }
    $VscodeDirectory = Join-Path $LocalPath '.vscode'
    [IO.Directory]::CreateDirectory($VscodeDirectory) | Out-Null
    $SftpPath = Join-Path $VscodeDirectory 'SFTP.json'
    if ([IO.File]::Exists($SftpPath)) {
        $BackupPath = $SftpPath + '.swaw-kit-ssh-remote-backup-' +
            (Get-Date -Format 'yyyyMMddHHmmss')
        [IO.File]::Copy($SftpPath, $BackupPath, $false)
        Write-Host "Existing SFTP config backed up: `"$BackupPath`""
    }
    $SftpNameSuffix = if ($Runtime.Context.UseSshConfigHost) {
        $Runtime.Context.RemoteTarget
    } else {
        $Runtime.RemoteUser
    }
    $SftpName = $LocalPath.Replace('\', '/') + '.' + $SftpNameSuffix
    if ($Runtime.Context.UseSshConfigHost) {
        $SftpDocument = [ordered]@{
            name          = $SftpName
            host          = $Runtime.Context.RemoteTarget
            protocol      = 'sftp'
            sshConfigPath = $Runtime.Context.SshConfigPath.Replace('\', '/')
            remotePath    = $RemotePath
            uploadOnSave  = $true
            useTempFile   = $false
            openSsh       = $true
        }
    } else {
        $SftpDocument = [ordered]@{
            name           = $SftpName
            host           = $Runtime.RemoteHost
            protocol       = 'sftp'
            port           = $Runtime.Port
            username       = $Runtime.RemoteUser
            privateKeyPath = $Runtime.SshKeyPath.Replace('\', '/')
            remotePath     = $RemotePath
            uploadOnSave   = $true
            useTempFile    = $false
            openSsh        = $false
        }
    }
    [IO.File]::WriteAllText(
        $SftpPath,
        (ConvertTo-Json -InputObject $SftpDocument -Depth 4) +
            [Environment]::NewLine,
        (New-Object Text.UTF8Encoding($false))
    )
    Write-Host "SFTP config written: `"$SftpPath`""
    Write-Host 'SFTP config is ready. Required extension: SFTP by Natizyskunk'
    return Invoke-RemoteKitEditorLaunch `
        -Tool $Tool `
        -Target $LocalPath `
        -RemoteAuthority ''
}

function Invoke-RemoteKitKeyVerb {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Runtime,
        [Parameter(Mandatory = $true)][string]$Verb
    )

    $Action = if ($Verb -eq 'key.remove') { 'remove' } elseif (
        $Verb -eq 'key.fix'
    ) { 'fix' } else { 'add' }
    $Arguments = @(
        '-Port', $Runtime.Port,
        '-RemoteHost', $Runtime.RemoteHost,
        '-RemoteUser', $Runtime.RemoteUser,
        '-SshKeyPath', $Runtime.SshKeyPath,
        '-Action', $Action
    )
    if ($Verb -eq 'key.add.fix') {
        $Arguments += '-FixSshdConfig'
    }
    return Invoke-RemoteKitChildPowerShell `
        -ScriptPath (Join-Path $PSScriptRoot 'key_manager.ps1') `
        -Arguments $Arguments
}

function Invoke-RemoteKitMain {
    if (Test-RemoteKitVerbose) {
        Write-Host (
            '[DEBUG] CMD bridge argv=' +
            ($script:ForwardedArguments | ConvertTo-Json -Compress)
        )
    }
    $Invocation = Get-RemoteKitForwardedInvocation
    $CommandArguments = @($Invocation.CommandArguments)
    $Verb = if ($CommandArguments.Count -eq 0) {
        ''
    } else {
        [string]$CommandArguments[0]
    }
    $VerbArguments = @(
        if ($CommandArguments.Count -gt 1) {
            $CommandArguments[1..($CommandArguments.Count - 1)]
        }
    )

    if ($Verb -eq 'config.remove') {
        if ($VerbArguments.Count -ne 0 -or
            [string]::IsNullOrWhiteSpace($env:REMOTE_KIT_ENTRY_FILE)) {
            throw 'config.remove is only supported by an embedded SSH entry.'
        }
        $HostAlias = Get-RemoteKitEntryHostAlias `
            -EntryFile $env:REMOTE_KIT_ENTRY_FILE
        Remove-RemoteKitEmbeddedSshConfig `
            -EntryFile $env:REMOTE_KIT_ENTRY_FILE `
            -RepoRoot $script:KitRepoRoot `
            -UserProfile $env:USERPROFILE
        Write-Host "SSH config removed for `"$HostAlias`"."
        $script:ExitCode = 0
        return
    }
    if ($Verb -eq 'config.install' -and $VerbArguments.Count -ne 0) {
        throw 'config.install does not accept arguments.'
    }
    if (($Verb -eq 'code' -or $Verb -eq 'cursor') -and
        $env:REMOTE_KIT_PROTOCOL -ne '2') {
        throw (
            'This remote entry predates the clean editor bootstrap. ' +
            'Update its header from Favorites\template.vps1.cmd.'
        )
    }

    $InstallEmbeddedConfig = $Verb -eq 'config.install' -or
        (($Verb -eq 'code' -or $Verb -eq 'cursor') -and
            $VerbArguments.Count -eq 1)
    $Runtime = New-RemoteKitRuntime `
        -Invocation $Invocation `
        -EmbeddedConfigAction $(if ($InstallEmbeddedConfig) {
            'install'
        } else {
            'write'
        })

    if ($Verb.Length -eq 0) {
        return Invoke-RemoteKitInheritedCommand `
            -FilePath (Get-RemoteKitApplicationPath 'ssh') `
            -Arguments (
                @(Get-RemoteKitOpenSshBaseArgs) +
                @(Get-RemoteKitOpenSshTargetArgs)
            ) `
            -Label 'ssh command'
    }
    if ($Verb -eq '--') {
        return Invoke-RemoteKitRemoteCommand `
            -Runtime $Runtime `
            -CommandArguments $VerbArguments
    }
    if ($Verb -eq 'tty') {
        if ($VerbArguments.Count -lt 2 -or
            [string]$VerbArguments[0] -ne '--') {
            throw 'tty requires -- followed by a remote command.'
        }
        return Invoke-RemoteKitRemoteCommand `
            -Runtime $Runtime `
            -CommandArguments @(
                $VerbArguments[1..($VerbArguments.Count - 1)]
            ) `
            -Tty
    }
    if ($Verb -eq 'script') {
        return Invoke-RemoteKitScriptVerb `
            -Runtime $Runtime `
            -Arguments $VerbArguments
    }
    if ($Verb -eq 'stdin') {
        return Invoke-RemoteKitStdinVerb `
            -Runtime $Runtime `
            -Arguments $VerbArguments
    }
    if ($Verb -eq 'copy') {
        return Invoke-RemoteKitCopyVerb `
            -Runtime $Runtime `
            -Arguments $VerbArguments
    }
    if ($Verb -eq 'code' -or $Verb -eq 'cursor') {
        return Invoke-RemoteKitEditorVerb `
            -Runtime $Runtime `
            -Tool $Verb `
            -Arguments $VerbArguments
    }
    if ($Verb -in @(
        'key.add',
        'key.remove',
        'key.fix',
        'key.add.fix'
    )) {
        if ($VerbArguments.Count -ne 0) {
            throw "$Verb does not accept arguments."
        }
        return Invoke-RemoteKitKeyVerb -Runtime $Runtime -Verb $Verb
    }
    if ($Verb -eq 'config.install') {
        if ($VerbArguments.Count -ne 0 -or -not $Runtime.IsEmbedded) {
            throw 'config.install is only supported by an embedded SSH entry.'
        }
        Write-Host (
            'SSH config installed for "' +
            $Runtime.Context.RemoteTarget +
            '": "' +
            $Runtime.Context.SshConfigPath +
            '"'
        )
        $script:ExitCode = 0
        return
    }
    throw 'Unrecognized argument combination. Run -h to view usage.'
}

try {
    $Utf8 = New-Object Text.UTF8Encoding($false)
    [Console]::OutputEncoding = $Utf8
    $OutputEncoding = $Utf8
    Invoke-RemoteKitMain
    exit $script:ExitCode
} catch {
    [Console]::Out.WriteLine("[ERROR] $($_.Exception.Message)")
    if (Test-RemoteKitVerbose) {
        [Console]::Out.WriteLine($_.ScriptStackTrace)
    }
    exit 1
}
