Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot 'process-job.ps1')

function Resolve-RdpClientPeerSshEntryPath {
    param([AllowNull()][AllowEmptyString()][string]$Value)

    if ([string]::IsNullOrWhiteSpace($Value)) {
        throw (
            'RDP_PEER_SSH_ENTRY is empty. Set it to a template.vps1.cmd ' +
            'entry that can connect to this Windows target.'
        )
    }

    $ExpandedPath = [Environment]::ExpandEnvironmentVariables($Value.Trim())
    if (-not [IO.Path]::IsPathRooted($ExpandedPath)) {
        throw 'RDP_PEER_SSH_ENTRY must be an absolute path.'
    }

    $ResolvedPath = [IO.Path]::GetFullPath($ExpandedPath)
    if (-not [IO.File]::Exists($ResolvedPath)) {
        throw "RDP_PEER_SSH_ENTRY was not found: $ResolvedPath"
    }
    if (@('.cmd', '.bat') -notcontains [IO.Path]::GetExtension($ResolvedPath).ToLowerInvariant()) {
        throw 'RDP_PEER_SSH_ENTRY must name a .cmd or .bat entry file.'
    }
    return $ResolvedPath
}

function Assert-RdpClientPeerSshEntryIsSeparate {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][string]$RdpEntryPath
    )

    if ([string]::Equals(
        $SshEntryPath,
        $RdpEntryPath,
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw 'RDP_PEER_SSH_ENTRY cannot point to this RDP entry itself.'
    }
}

function ConvertTo-RdpClientEncodedCommand {
    param([Parameter(Mandatory = $true)][string]$Source)

    $Encoded = [Convert]::ToBase64String(
        [Text.Encoding]::Unicode.GetBytes($Source)
    )
    while ($Encoded.EndsWith('=')) {
        $Source += ' '
        $Encoded = [Convert]::ToBase64String(
            [Text.Encoding]::Unicode.GetBytes($Source)
        )
    }
    return $Encoded
}

function Invoke-RdpClientPeerSshProcess {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][string]$Arguments,
        [Parameter(Mandatory = $true)][string]$Operation,
        [AllowNull()][Collections.IDictionary]$EnvironmentVariables,
        [Parameter(Mandatory = $true)][pscustomobject]$TimeoutBudget
    )

    $Utf8 = New-Object Text.UTF8Encoding($false)
    $Process = New-Object Diagnostics.Process
    $ProcessJob = $null
    $Started = $false
    try {
        $Process.StartInfo.FileName = $env:ComSpec
        $Process.StartInfo.Arguments = $Arguments
        $Process.StartInfo.UseShellExecute = $false
        $Process.StartInfo.CreateNoWindow = $true
        $Process.StartInfo.RedirectStandardInput = $false
        $Process.StartInfo.RedirectStandardOutput = $true
        $Process.StartInfo.RedirectStandardError = $true
        $Process.StartInfo.StandardOutputEncoding = $Utf8
        $Process.StartInfo.StandardErrorEncoding = $Utf8
        [void]$Process.StartInfo.EnvironmentVariables
        $ChildEnvironment = $Process.StartInfo.EnvironmentVariables
        foreach ($Item in [Environment]::GetEnvironmentVariables().GetEnumerator()) {
            $ChildEnvironment[[string]$Item.Key] = [string]$Item.Value
        }
        $ChildEnvironment['RDP_CLIENT_PEER_SSH_ENTRY'] = $SshEntryPath
        if ($null -ne $EnvironmentVariables) {
            foreach ($Name in $EnvironmentVariables.Keys) {
                $ChildEnvironment[[string]$Name] = [string](
                    $EnvironmentVariables[$Name]
                )
            }
        }

        $TimeoutMilliseconds = Get-RdpClientTimeoutBudgetRemainingMilliseconds `
            -Budget $TimeoutBudget
        if ($TimeoutMilliseconds -eq 0) {
            throw (
                "SSH peer $Operation timed out after " +
                "$($TimeoutBudget.TimeoutSeconds) seconds."
            )
        }

        $ProcessJob = New-Object SwawKit.RdpClient.ProcessJob
        $Process.Start() | Out-Null
        $Started = $true
        try {
            $ProcessJob.Assign($Process)
        } catch {
            $ProcessJob.Dispose()
            $ProcessJob = $null
        }
        $StdOutTask = $Process.StandardOutput.ReadToEndAsync()
        $StdErrTask = $Process.StandardError.ReadToEndAsync()
        if (-not $Process.WaitForExit($TimeoutMilliseconds)) {
            Stop-RdpClientProcessTree -Process $Process -Job $ProcessJob
            throw (
                "SSH peer $Operation timed out after " +
                "$($TimeoutBudget.TimeoutSeconds) seconds."
            )
        }
        foreach ($ReadTask in @($StdOutTask, $StdErrTask)) {
            if (-not $ReadTask.Wait(5000)) {
                Stop-RdpClientProcessTree -Process $Process -Job $ProcessJob
                throw "SSH peer $Operation timed out while collecting output."
            }
        }
        $StdOut = $StdOutTask.Result
        $StdErr = $StdErrTask.Result
        $ExitCode = $Process.ExitCode
    } finally {
        if ($Started) {
            Stop-RdpClientProcessTree -Process $Process -Job $ProcessJob
        } elseif ($null -ne $ProcessJob) {
            try { $ProcessJob.Dispose() } catch { }
        }
        $Process.Dispose()
    }

    return [pscustomobject]@{
        ExitCode = $ExitCode
        Output   = @(
            foreach ($Text in @($StdOut, $StdErr)) {
                @($Text -split '\r?\n' | Where-Object { $_.Length -gt 0 })
            }
        )
    }
}

function Invoke-RdpClientPeerSshEncodedCommand {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][string]$EncodedCommand,
        [ValidateRange(1, 1800)][int]$TimeoutSeconds = 120,
        [AllowNull()][pscustomobject]$TimeoutBudget
    )

    if ($null -eq $TimeoutBudget) {
        $TimeoutBudget = New-RdpClientTimeoutBudget `
            -TimeoutSeconds $TimeoutSeconds
    }
    return Invoke-RdpClientPeerSshProcess `
        -SshEntryPath $SshEntryPath `
        -Arguments (
            '/d /s /c ""%RDP_CLIENT_PEER_SSH_ENTRY%" -- ' +
            'powershell.exe -NoLogo -NoProfile -NonInteractive ' +
            '-OutputFormat Text -EncodedCommand ' + $EncodedCommand + '"'
        ) `
        -Operation command `
        -TimeoutBudget $TimeoutBudget
}

function Invoke-RdpClientPeerSshPowerShell {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][string]$RemoteSource,
        [ValidateRange(1, 1800)][int]$TimeoutSeconds = 120
    )

    $TimeoutBudget = New-RdpClientTimeoutBudget -TimeoutSeconds $TimeoutSeconds
    $LocalDirectory = [IO.Path]::GetFullPath(
        (Join-Path $PSScriptRoot '..\..\data\rdp-client')
    )
    $RemoteName = '.swaw-kit-rdp-peer-' + [Guid]::NewGuid().ToString('N') + '.ps1'
    $LocalPath = Join-Path $LocalDirectory $RemoteName
    $RemoteMayExist = $false
    try {
        [IO.Directory]::CreateDirectory($LocalDirectory) | Out-Null
        [IO.File]::WriteAllText(
            $LocalPath,
            $RemoteSource,
            (New-Object Text.UTF8Encoding($false))
        )

        $Remaining = Get-RdpClientTimeoutBudgetRemainingMilliseconds `
            -Budget $TimeoutBudget
        if ($Remaining -eq 0) {
            throw "SSH peer command timed out after $TimeoutSeconds seconds."
        }
        $Copy = Invoke-RdpClientPeerSshCopy `
            -SshEntryPath $SshEntryPath `
            -SourcePath $LocalPath `
            -RemoteName $RemoteName `
            -TimeoutBudget $TimeoutBudget
        if ($Copy.ExitCode -ne 0) {
            throw ('SSH peer script upload failed. ' + ($Copy.Output -join ' '))
        }
        [IO.File]::Delete($LocalPath)
        $RemoteMayExist = $true

        $Remaining = Get-RdpClientTimeoutBudgetRemainingMilliseconds `
            -Budget $TimeoutBudget
        if ($Remaining -eq 0) {
            throw "SSH peer command timed out after $TimeoutSeconds seconds."
        }
        $RemoteTimeoutMilliseconds = [Math]::Max(100, $Remaining - 100)
        $Bootstrap = (
            '$ProgressPreference=''SilentlyContinue'';' +
            '$p=Join-Path $HOME ''' + $RemoteName + ''';$x=$null;$e=1;' +
            'try{$a=''-NoLogo -NoProfile -NonInteractive -OutputFormat Text ' +
            '-ExecutionPolicy Bypass -File "''+$p+''"'';' +
            '$x=Start-Process -FilePath (Join-Path $PSHOME ''powershell.exe'') ' +
            '-ArgumentList $a -NoNewWindow -PassThru;' +
            'if(-not $x.WaitForExit(' + $RemoteTimeoutMilliseconds + ')){' +
            '& taskkill.exe /PID $x.Id /T /F 2>$null|Out-Null;' +
            'throw ''RDP peer script timed out.''};$e=$x.ExitCode}' +
            'finally{if($null-ne $x){$x.Dispose()};' +
            'Remove-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue};' +
            'exit $e'
        )
        $Invocation = Invoke-RdpClientPeerSshEncodedCommand `
            -SshEntryPath $SshEntryPath `
            -EncodedCommand (ConvertTo-RdpClientEncodedCommand -Source $Bootstrap) `
            -TimeoutBudget $TimeoutBudget
        $RemoteMayExist = $false
        return $Invocation
    } finally {
        $TimeoutBudget.Stopwatch.Stop()
        if ([IO.File]::Exists($LocalPath)) {
            [IO.File]::Delete($LocalPath)
        }
        if ($RemoteMayExist) {
            try {
                $Cleanup = (
                    'Remove-Item -LiteralPath (Join-Path $HOME ''' +
                    $RemoteName + ''') -Force -ErrorAction SilentlyContinue'
                )
                $null = Invoke-RdpClientPeerSshEncodedCommand `
                    -SshEntryPath $SshEntryPath `
                    -EncodedCommand (ConvertTo-RdpClientEncodedCommand -Source $Cleanup) `
                    -TimeoutSeconds 1
            } catch {
            }
        }
    }
}

function Invoke-RdpClientPeerSshCopy {
    param(
        [Parameter(Mandatory = $true)][string]$SshEntryPath,
        [Parameter(Mandatory = $true)][string]$SourcePath,
        [Parameter(Mandatory = $true)][string]$RemoteName,
        [ValidateRange(1, 1800)][int]$TimeoutSeconds = 120,
        [AllowNull()][pscustomobject]$TimeoutBudget
    )

    if (-not [IO.File]::Exists($SourcePath)) {
        throw "SSH copy source was not found: $SourcePath"
    }
    if ($RemoteName -notmatch '^[A-Za-z0-9._-]+$') {
        throw 'SSH copy remote name contains unsupported characters.'
    }

    if ($null -eq $TimeoutBudget) {
        $TimeoutBudget = New-RdpClientTimeoutBudget `
            -TimeoutSeconds $TimeoutSeconds
    }
    return Invoke-RdpClientPeerSshProcess `
        -SshEntryPath $SshEntryPath `
        -Arguments (
            '/d /s /c ""%RDP_CLIENT_PEER_SSH_ENTRY%" copy ' +
            '"%RDP_CLIENT_PEER_COPY_SOURCE%" ' +
            '":%RDP_CLIENT_PEER_COPY_NAME%""'
        ) `
        -Operation copy `
        -EnvironmentVariables @{
            RDP_CLIENT_PEER_COPY_SOURCE = $SourcePath
            RDP_CLIENT_PEER_COPY_NAME   = $RemoteName
        } `
        -TimeoutBudget $TimeoutBudget
}
