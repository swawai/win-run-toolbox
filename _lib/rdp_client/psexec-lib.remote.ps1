function Get-RdpClientPsExecSignature {
    param([Parameter(Mandatory = $true)][string]$Path)

    $Signature = Get-AuthenticodeSignature -LiteralPath $Path
    $Subject = ''
    if ($null -ne $Signature.SignerCertificate) {
        $Subject = [string]$Signature.SignerCertificate.Subject
    }
    return [pscustomobject]@{
        Status    = [string]$Signature.Status
        Subject   = $Subject
        IsTrusted = (
            [string]$Signature.Status -eq 'Valid' -and
            $Subject -match '(?:^|,\s*)O=Microsoft Corporation(?:,|$)'
        )
    }
}

function Invoke-RdpClientCapturedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [AllowNull()][object[]]$Arguments = @()
    )

    $Process = New-Object Diagnostics.Process
    $Started = $false
    try {
        $Process.StartInfo.FileName = $FilePath
        $Process.StartInfo.Arguments = Join-RdpClientProcessArguments $Arguments
        $Process.StartInfo.UseShellExecute = $false
        $Process.StartInfo.CreateNoWindow = $true
        $Process.StartInfo.RedirectStandardInput = $false
        $Process.StartInfo.RedirectStandardOutput = $true
        $Process.StartInfo.RedirectStandardError = $true
        $Process.StartInfo.StandardOutputEncoding = $Utf8
        $Process.StartInfo.StandardErrorEncoding = $Utf8
        $Process.Start() | Out-Null
        $Started = $true
        $StdOutTask = $Process.StandardOutput.ReadToEndAsync()
        $StdErrTask = $Process.StandardError.ReadToEndAsync()
        $Process.WaitForExit()
        return [pscustomobject]@{
            ExitCode = $Process.ExitCode
            StdOut   = $StdOutTask.Result
            StdErr   = $StdErrTask.Result
        }
    } finally {
        if ($Started -and -not $Process.HasExited) {
            try { $Process.Kill() } catch { }
        }
        $Process.Dispose()
    }
}

function Invoke-RdpClientUncapturedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [AllowNull()][object[]]$Arguments = @(),
        [ValidateRange(1, 60)][int]$TimeoutSeconds = 30
    )

    $Process = New-Object Diagnostics.Process
    $Started = $false
    try {
        $Process.StartInfo.FileName = $FilePath
        $Process.StartInfo.Arguments = Join-RdpClientProcessArguments $Arguments
        $Process.StartInfo.UseShellExecute = $false
        $Process.StartInfo.CreateNoWindow = $true
        $Process.StartInfo.RedirectStandardInput = $false
        $Process.StartInfo.RedirectStandardOutput = $false
        $Process.StartInfo.RedirectStandardError = $false
        $Process.Start() | Out-Null
        $Started = $true
        if (-not $Process.WaitForExit($TimeoutSeconds * 1000)) {
            try { $Process.Kill() } catch { }
            throw "Detached process launcher timed out after $TimeoutSeconds seconds."
        }
        return $Process.ExitCode
    } finally {
        if ($Started -and -not $Process.HasExited) {
            try { $Process.Kill() } catch { }
        }
        $Process.Dispose()
    }
}

function Get-RdpClientManagedScriptState {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ExpectedHash
    )

    $Present = [IO.File]::Exists($Path)
    $Hash = if ($Present) {
        (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToUpperInvariant()
    } else {
        ''
    }
    return [pscustomobject]@{
        Present = $Present
        Hash    = $Hash
        Ready   = $Present -and $Hash -eq $ExpectedHash
    }
}

function Install-RdpClientManagedScript {
    param(
        [Parameter(Mandatory = $true)][string]$UploadPath,
        [Parameter(Mandatory = $true)][string]$DestinationPath,
        [Parameter(Mandatory = $true)][string]$ExpectedHash,
        [Parameter(Mandatory = $true)][string]$Label
    )

    if (-not [IO.File]::Exists($UploadPath)) {
        throw "The uploaded $Label was not found."
    }
    $SourceHash = (
        Get-FileHash -LiteralPath $UploadPath -Algorithm SHA256
    ).Hash.ToUpperInvariant()
    if ($SourceHash -ne $ExpectedHash) {
        throw "The $Label source failed SHA-256 verification."
    }
    $TemporaryPath = Join-Path ([IO.Path]::GetDirectoryName($DestinationPath)) (
        '.managed-script-' + [Guid]::NewGuid().ToString('N') + '.ps1'
    )
    try {
        Copy-Item -LiteralPath $UploadPath -Destination $TemporaryPath
        Move-Item -LiteralPath $TemporaryPath -Destination $DestinationPath -Force
    } finally {
        if ([IO.File]::Exists($TemporaryPath)) {
            Remove-Item -LiteralPath $TemporaryPath -Force
        }
    }
}

function Wait-RdpClientDesktopResultFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][Text.Encoding]$Encoding,
        [ValidateRange(1, 600)][int]$TimeoutSeconds
    )

    $Stopwatch = [Diagnostics.Stopwatch]::StartNew()
    while ($Stopwatch.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        if ([IO.File]::Exists($Path)) {
            $Length = (Get-Item -LiteralPath $Path).Length
            if ($Length -gt 104857600) {
                throw 'The interactive desktop worker result file is too large.'
            }
            if ($Length -gt 0) {
                try {
                    $Marker = [IO.File]::ReadAllText($Path, $Encoding).Trim()
                } catch [IO.IOException] {
                    Start-Sleep -Milliseconds 200
                    continue
                }
                if ($Marker -notmatch
                    '^RDP_CLIENT_DESKTOP_RESULT_V1:(?<Payload>[A-Za-z0-9+/=]+)$') {
                    throw 'The interactive desktop worker result marker is invalid.'
                }
                try {
                    $Json = $Encoding.GetString(
                        [Convert]::FromBase64String($Matches.Payload)
                    )
                    $Result = $Json | ConvertFrom-Json
                } catch {
                    throw 'The interactive desktop worker result JSON is invalid.'
                }
                if ($null -eq $Result -or $Result -is [Array] -or
                    $null -eq $Result.PSObject.Properties['Success']) {
                    throw 'The interactive desktop worker result is unsupported.'
                }
                return [pscustomobject]@{
                    Marker  = $Marker
                    Success = [bool]$Result.Success
                }
            }
        }
        Start-Sleep -Milliseconds 200
    }
    throw "The interactive desktop worker timed out after $TimeoutSeconds seconds."
}

function Wait-RdpClientDesktopWorkerIdentityFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [ValidateRange(1, 10)][int]$TimeoutSeconds,
        [Parameter(Mandatory = $true)][int]$LauncherExitCode
    )

    $Stopwatch = [Diagnostics.Stopwatch]::StartNew()
    while ($Stopwatch.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        if ([IO.File]::Exists($Path) -and
            (Get-Item -LiteralPath $Path).Length -gt 0) {
            return
        }
        Start-Sleep -Milliseconds 100
    }
    throw (
        'PsExec did not start the interactive desktop worker within ' +
        "$TimeoutSeconds seconds; launcher exit=$LauncherExitCode."
    )
}

function Stop-RdpClientDesktopWorkerProcess {
    param([Parameter(Mandatory = $true)][string]$IdentityPath)

    if (-not [IO.File]::Exists($IdentityPath)) {
        return
    }

    $Process = $null
    try {
        $Identity = [IO.File]::ReadAllText($IdentityPath) | ConvertFrom-Json
        $ProcessId = [int]0
        $StartTimeUtcTicks = [int64]0
        if ($null -eq $Identity -or $Identity -is [Array] -or
            $null -eq $Identity.PSObject.Properties['Version'] -or
            [int]$Identity.Version -ne 1 -or
            -not [int]::TryParse(
                [string]$Identity.ProcessId,
                [ref]$ProcessId
            ) -or $ProcessId -le 0 -or
            -not [int64]::TryParse(
                [string]$Identity.StartTimeUtcTicks,
                [ref]$StartTimeUtcTicks
            ) -or $StartTimeUtcTicks -le 0) {
            return
        }
        try {
            $Process = [Diagnostics.Process]::GetProcessById($ProcessId)
        } catch [ArgumentException] {
            return
        }
        if ($Process.StartTime.ToUniversalTime().Ticks -ne $StartTimeUtcTicks) {
            return
        }
        Stop-Process -InputObject $Process -Force -ErrorAction SilentlyContinue
    } catch {
        return
    } finally {
        if ($null -ne $Process) {
            $Process.Dispose()
        }
    }
}
