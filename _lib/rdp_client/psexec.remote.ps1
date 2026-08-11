$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Set-StrictMode -Version 2.0

$Utf8 = New-Object Text.UTF8Encoding($false)
[Console]::InputEncoding = $Utf8
[Console]::OutputEncoding = $Utf8
$OutputEncoding = $Utf8
$HelperUploadPath = ''
$DesktopWorkerUploadPath = ''

function Get-RdpClientNativeArchitecture {
    $Architecture = $env:PROCESSOR_ARCHITEW6432
    if ([string]::IsNullOrWhiteSpace($Architecture)) {
        $Architecture = $env:PROCESSOR_ARCHITECTURE
    }
    if ([string]::IsNullOrWhiteSpace($Architecture)) {
        throw 'Windows architecture environment variables are unavailable.'
    }
    switch ($Architecture.ToUpperInvariant()) {
        'AMD64' { return 'AMD64' }
        'ARM64' { return 'ARM64' }
        'X86' { return 'x86' }
        default { throw "Unsupported Windows architecture: $Architecture" }
    }
}

function Get-RdpClientPsExecDownload {
    param([Parameter(Mandatory = $true)][string]$Architecture)

    switch ($Architecture) {
        'AMD64' {
            return [pscustomobject]@{
                FileName = 'PsExec64.exe'
                Uri      = 'https://live.sysinternals.com/PsExec64.exe'
            }
        }
        'ARM64' {
            return [pscustomobject]@{
                FileName = 'PsExec64a.exe'
                Uri      = 'https://live.sysinternals.com/ARM64/PsExec64a.exe'
            }
        }
        'x86' {
            return [pscustomobject]@{
                FileName = 'PsExec.exe'
                Uri      = 'https://live.sysinternals.com/PsExec.exe'
            }
        }
        default { throw "Unsupported Windows architecture: $Architecture" }
    }
}

function Get-RdpClientSshServerAddress {
    $Parts = @([string]$env:SSH_CONNECTION -split '\s+' | Where-Object {
        $_.Length -gt 0
    })
    if ($Parts.Count -lt 4) {
        throw 'SSH_CONNECTION is unavailable; the peer identity cannot be verified.'
    }
    $Address = $null
    if (-not [Net.IPAddress]::TryParse($Parts[2], [ref]$Address)) {
        throw "SSH_CONNECTION contains an invalid peer address: $($Parts[2])"
    }
    if ($Address.IsIPv4MappedToIPv6) {
        return $Address.MapToIPv4().ToString()
    }
    return $Address.ToString()
}

function Assert-RdpClientExpectedPeer {
    param(
        [Parameter(Mandatory = $true)][string]$PeerAddress,
        [Parameter(Mandatory = $true)][object[]]$ExpectedAddresses
    )

    if (-not @($ExpectedAddresses | Where-Object {
        [string]::Equals(
            [string]$_,
            $PeerAddress,
            [StringComparison]::OrdinalIgnoreCase
        )
    }).Count) {
        throw (
            "SSH peer $PeerAddress does not match the RDP full address " +
            "($($ExpectedAddresses -join ', '))."
        )
    }
}

function Write-RdpClientPsExecHeader {
    param(
        [Parameter(Mandatory = $true)][string]$Title,
        [Parameter(Mandatory = $true)][string]$PeerAddress,
        [Parameter(Mandatory = $true)][string]$Architecture
    )

    Write-Output "[RDP] Peer PsExec $Title"
    Write-RdpClientPsExecField `
        -Name 'Peer:' `
        -Value "$env:COMPUTERNAME ($PeerAddress via SSH)" `
        -Width 14
    Write-RdpClientPsExecField `
        -Name 'SSH account:' `
        -Value ([Security.Principal.WindowsIdentity]::GetCurrent().Name) `
        -Width 14
    Write-RdpClientPsExecField `
        -Name 'Architecture:' `
        -Value $Architecture `
        -Width 14
    Write-Output '  ---'
}

function Write-RdpClientPsExecField {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Value,
        [ValidateRange(1, 80)][int]$Width = 15
    )

    Write-Output ('  ' + $Name.PadRight($Width) + $Value)
}

function Write-RdpClientPsExecFile {
    param(
        [Parameter(Mandatory = $true)][string]$Action,
        [Parameter(Mandatory = $true)][string]$Path
    )

    Write-Output ('  {0,-9}{1}' -f $Action, $Path)
}

function Join-RdpClientProcessArguments {
    param([AllowNull()][object[]]$Arguments = @())

    $Quoted = foreach ($Argument in @($Arguments)) {
        $Value = [string]$Argument
        if ($Value.Length -eq 0) {
            '""'
        } elseif ($Value -notmatch '[\s"]') {
            $Value
        } else {
            '"' +
                ($Value -replace '(\\*)"', '$1$1\"' -replace '(\\+)$', '$1$1') +
                '"'
        }
    }
    return $Quoted -join ' '
}

__RDP_CLIENT_PSEXEC_LIBRARY__

try {
    $PayloadBase64 = '__RDP_CLIENT_PSEXEC_PAYLOAD__'
    $PayloadJson = $Utf8.GetString([Convert]::FromBase64String($PayloadBase64))
    $Request = $PayloadJson | ConvertFrom-Json
    $PeerAddress = Get-RdpClientSshServerAddress
    Assert-RdpClientExpectedPeer `
        -PeerAddress $PeerAddress `
        -ExpectedAddresses @($Request.ExpectedAddresses)

    $Architecture = Get-RdpClientNativeArchitecture
    $Download = Get-RdpClientPsExecDownload -Architecture $Architecture
    $LocalAppData = [Environment]::GetFolderPath(
        [Environment+SpecialFolder]::LocalApplicationData
    )
    if ([string]::IsNullOrWhiteSpace($LocalAppData)) {
        throw 'The peer SSH account has no LocalApplicationData directory.'
    }
    $ManagedDirectory = Join-Path $LocalAppData 'swaw-kit\rdp-client'
    $ManagedPath = Join-Path $ManagedDirectory 'psexec.exe'
    $HelperPath = Join-Path $ManagedDirectory 'helper.ps1'
    $DesktopWorkerPath = Join-Path $ManagedDirectory 'desktop-task.ps1'
    $ExpectedHelperHash = [string]$Request.HelperSha256
    if ($ExpectedHelperHash -notmatch '^[A-Fa-f0-9]{64}$') {
        throw 'The expected PsExec session helper hash is invalid.'
    }
    $ExpectedHelperHash = $ExpectedHelperHash.ToUpperInvariant()
    $ExpectedDesktopWorkerHash = [string]$Request.DesktopWorkerSha256
    if ($ExpectedDesktopWorkerHash -notmatch '^[A-Fa-f0-9]{64}$') {
        throw 'The expected PsExec desktop worker hash is invalid.'
    }
    $ExpectedDesktopWorkerHash = $ExpectedDesktopWorkerHash.ToUpperInvariant()
    if ($Request.Action -eq 'add' -and -not [bool]$Request.DryRun) {
        $HelperUploadName = [string]$Request.HelperUploadName
        if ($HelperUploadName -notmatch
            '^\.swaw-kit-psexec-helper-[A-Fa-f0-9]{32}\.ps1$') {
            throw 'The PsExec session helper upload name is invalid.'
        }
        $HelperUploadPath = Join-Path $HOME $HelperUploadName
        $DesktopWorkerUploadName = [string]$Request.DesktopWorkerUploadName
        if ($DesktopWorkerUploadName -notmatch
            '^\.swaw-kit-psexec-desktop-[A-Fa-f0-9]{32}\.ps1$') {
            throw 'The PsExec desktop worker upload name is invalid.'
        }
        $DesktopWorkerUploadPath = Join-Path $HOME $DesktopWorkerUploadName
    }
    $Present = [IO.File]::Exists($ManagedPath)
    $Signature = $null
    if ($Present) {
        $Signature = Get-RdpClientPsExecSignature -Path $ManagedPath
    }
    $Helper = Get-RdpClientManagedScriptState `
        -Path $HelperPath `
        -ExpectedHash $ExpectedHelperHash
    $DesktopWorker = Get-RdpClientManagedScriptState `
        -Path $DesktopWorkerPath `
        -ExpectedHash $ExpectedDesktopWorkerHash

    if ($Request.Action -eq 'status') {
        Write-RdpClientPsExecHeader `
            -Title 'status' `
            -PeerAddress $PeerAddress `
            -Architecture $Architecture
        $Ready = (
            $Present -and
            $Signature.IsTrusted -and
            $Helper.Ready -and
            $DesktopWorker.Ready
        )
        if ($Present) {
            Write-RdpClientPsExecFile -Action 'PRESENT' -Path $ManagedPath
            $Version = [Diagnostics.FileVersionInfo]::GetVersionInfo(
                $ManagedPath
            ).FileVersion
        } else {
            Write-RdpClientPsExecFile -Action 'ABSENT' -Path $ManagedPath
        }
        if ($Helper.Present) {
            Write-RdpClientPsExecFile -Action 'PRESENT' -Path $HelperPath
        } else {
            Write-RdpClientPsExecFile -Action 'ABSENT' -Path $HelperPath
        }
        if ($DesktopWorker.Present) {
            Write-RdpClientPsExecFile -Action 'PRESENT' -Path $DesktopWorkerPath
        } else {
            Write-RdpClientPsExecFile -Action 'ABSENT' -Path $DesktopWorkerPath
        }
        if ($Present) {
            $Signer = if ($Signature.IsTrusted) {
                'Microsoft Corporation'
            } elseif ([string]::IsNullOrWhiteSpace($Signature.Subject)) {
                '<none>'
            } else {
                $Signature.Subject
            }
            $SignatureDetail = "version=$Version  signer=$Signer"
            if (-not $Signature.IsTrusted) {
                $SignatureDetail += "  status=$($Signature.Status)"
            }
            Write-RdpClientPsExecField `
                -Name 'PsExec VERIFY' `
                -Value $SignatureDetail
        }
        if ($Helper.Present) {
            $HelperDetail = "sha256=$($Helper.Hash)"
            if (-not $Helper.Ready) {
                $HelperDetail += "  expected=$ExpectedHelperHash"
            }
            Write-RdpClientPsExecField `
                -Name 'Helper VERIFY' `
                -Value $HelperDetail
        }
        if ($DesktopWorker.Present) {
            $WorkerDetail = "sha256=$($DesktopWorker.Hash)"
            if (-not $DesktopWorker.Ready) {
                $WorkerDetail += "  expected=$ExpectedDesktopWorkerHash"
            }
            Write-RdpClientPsExecField `
                -Name 'Desktop VERIFY' `
                -Value $WorkerDetail
        }
        Write-RdpClientPsExecField `
            -Name 'State:' `
            -Value $(if ($Ready) { 'READY' } else { 'INCOMPLETE' }) `
            -Width 14
        exit 0
    }

    if ($Request.Action -eq 'add') {
        if ([bool]$Request.DryRun) {
            Write-RdpClientPsExecHeader `
                -Title 'add plan' `
                -PeerAddress $PeerAddress `
                -Architecture $Architecture
            if ($Present -and $Signature.IsTrusted) {
                Write-RdpClientPsExecFile -Action 'PRESENT' -Path $ManagedPath
            } elseif ($Present) {
                Write-RdpClientPsExecFile -Action 'REPLACE' -Path $ManagedPath
            } else {
                Write-RdpClientPsExecFile -Action 'ADD' -Path $ManagedPath
            }
            if ($Helper.Ready) {
                Write-RdpClientPsExecFile -Action 'PRESENT' -Path $HelperPath
            } elseif ($Helper.Present) {
                Write-RdpClientPsExecFile -Action 'REPLACE' -Path $HelperPath
            } else {
                Write-RdpClientPsExecFile -Action 'ADD' -Path $HelperPath
            }
            if ($DesktopWorker.Ready) {
                Write-RdpClientPsExecFile -Action 'PRESENT' -Path $DesktopWorkerPath
            } elseif ($DesktopWorker.Present) {
                Write-RdpClientPsExecFile -Action 'REPLACE' -Path $DesktopWorkerPath
            } else {
                Write-RdpClientPsExecFile -Action 'ADD' -Path $DesktopWorkerPath
            }
            Write-RdpClientPsExecField `
                -Name 'PsExec SOURCE' `
                -Value $Download.Uri
            Write-RdpClientPsExecField `
                -Name 'PsExec VERIFY' `
                -Value 'Authenticode signer=Microsoft Corporation'
            Write-RdpClientPsExecField `
                -Name 'Helper VERIFY' `
                -Value "sha256=$ExpectedHelperHash"
            Write-RdpClientPsExecField `
                -Name 'Desktop VERIFY' `
                -Value "sha256=$ExpectedDesktopWorkerHash"
            Write-Output '[RDP] Dry run: no peer changes were made.'
            exit 0
        }

        [IO.Directory]::CreateDirectory($ManagedDirectory) | Out-Null
        $PsExecChanged = $false
        $PsExecChangeAction = ''
        if (-not ($Present -and $Signature.IsTrusted)) {
            $PsExecChangeAction = if ($Present) { 'REPLACED' } else { 'ADDED' }
            $TemporaryPath = Join-Path $ManagedDirectory (
                '.psexec-' + [Guid]::NewGuid().ToString('N') + '.exe'
            )
            try {
                [Net.ServicePointManager]::SecurityProtocol =
                    [Net.ServicePointManager]::SecurityProtocol -bor
                    [Net.SecurityProtocolType]::Tls12
                Invoke-WebRequest `
                    -UseBasicParsing `
                    -Uri $Download.Uri `
                    -OutFile $TemporaryPath
                $DownloadedSignature = Get-RdpClientPsExecSignature `
                    -Path $TemporaryPath
                if (-not $DownloadedSignature.IsTrusted) {
                    throw (
                        'Downloaded PsExec failed Microsoft Authenticode ' +
                        "verification: $($DownloadedSignature.Status), " +
                        $DownloadedSignature.Subject
                    )
                }
                Move-Item `
                    -LiteralPath $TemporaryPath `
                    -Destination $ManagedPath `
                    -Force
                $PsExecChanged = $true
            } finally {
                if ([IO.File]::Exists($TemporaryPath)) {
                    Remove-Item -LiteralPath $TemporaryPath -Force
                }
            }
        }

        $HelperChanged = $false
        $HelperChangeAction = ''
        if (-not $Helper.Ready) {
            $HelperChangeAction = if ($Helper.Present) { 'REPLACED' } else { 'ADDED' }
            Install-RdpClientManagedScript `
                -UploadPath $HelperUploadPath `
                -DestinationPath $HelperPath `
                -ExpectedHash $ExpectedHelperHash `
                -Label 'PsExec session helper'
            $HelperChanged = $true
        }
        $DesktopWorkerChanged = $false
        $DesktopWorkerChangeAction = ''
        if (-not $DesktopWorker.Ready) {
            $DesktopWorkerChangeAction = if ($DesktopWorker.Present) {
                'REPLACED'
            } else {
                'ADDED'
            }
            Install-RdpClientManagedScript `
                -UploadPath $DesktopWorkerUploadPath `
                -DestinationPath $DesktopWorkerPath `
                -ExpectedHash $ExpectedDesktopWorkerHash `
                -Label 'PsExec desktop worker'
            $DesktopWorkerChanged = $true
        }
        foreach ($UploadPath in @($HelperUploadPath, $DesktopWorkerUploadPath)) {
            if ([IO.File]::Exists($UploadPath)) {
                Remove-Item -LiteralPath $UploadPath -Force
            }
        }
        $HelperUploadPath = ''
        $DesktopWorkerUploadPath = ''
        Write-RdpClientPsExecHeader `
            -Title 'add' `
            -PeerAddress $PeerAddress `
            -Architecture $Architecture
        if ($PsExecChanged) {
            Write-RdpClientPsExecFile `
                -Action $PsExecChangeAction `
                -Path $ManagedPath
        } else {
            Write-RdpClientPsExecFile -Action 'PRESENT' -Path $ManagedPath
        }
        if ($HelperChanged) {
            Write-RdpClientPsExecFile `
                -Action $HelperChangeAction `
                -Path $HelperPath
        } else {
            Write-RdpClientPsExecFile -Action 'PRESENT' -Path $HelperPath
        }
        if ($DesktopWorkerChanged) {
            Write-RdpClientPsExecFile `
                -Action $DesktopWorkerChangeAction `
                -Path $DesktopWorkerPath
        } else {
            Write-RdpClientPsExecFile -Action 'PRESENT' -Path $DesktopWorkerPath
        }
        Write-Output '[RDP] Peer PsExec is ready.'
        exit 0
    }

    if ($Request.Action -eq 'remove') {
        if ([bool]$Request.DryRun) {
            Write-RdpClientPsExecHeader `
                -Title 'remove plan' `
                -PeerAddress $PeerAddress `
                -Architecture $Architecture
            if ($Present) {
                Write-RdpClientPsExecFile -Action 'REMOVE' -Path $ManagedPath
            } else {
                Write-RdpClientPsExecFile -Action 'ABSENT' -Path $ManagedPath
            }
            if ($Helper.Present) {
                Write-RdpClientPsExecFile -Action 'REMOVE' -Path $HelperPath
            } else {
                Write-RdpClientPsExecFile -Action 'ABSENT' -Path $HelperPath
            }
            if ($DesktopWorker.Present) {
                Write-RdpClientPsExecFile -Action 'REMOVE' -Path $DesktopWorkerPath
            } else {
                Write-RdpClientPsExecFile -Action 'ABSENT' -Path $DesktopWorkerPath
            }
            Write-Output '[RDP] Dry run: no peer changes were made.'
            exit 0
        }
        if ($Present) {
            Remove-Item -LiteralPath $ManagedPath -Force
        }
        if ($Helper.Present) {
            Remove-Item -LiteralPath $HelperPath -Force
        }
        if ($DesktopWorker.Present) {
            Remove-Item -LiteralPath $DesktopWorkerPath -Force
        }
        if ([IO.Directory]::Exists($ManagedDirectory) -and
            @(Get-ChildItem -LiteralPath $ManagedDirectory -Force).Count -eq 0) {
            [IO.Directory]::Delete($ManagedDirectory)
        }
        Write-RdpClientPsExecHeader `
            -Title 'remove' `
            -PeerAddress $PeerAddress `
            -Architecture $Architecture
        Write-RdpClientPsExecFile `
            -Action $(if ($Present) { 'REMOVED' } else { 'ABSENT' }) `
            -Path $ManagedPath
        Write-RdpClientPsExecFile `
            -Action $(if ($Helper.Present) { 'REMOVED' } else { 'ABSENT' }) `
            -Path $HelperPath
        Write-RdpClientPsExecFile `
            -Action $(if ($DesktopWorker.Present) { 'REMOVED' } else { 'ABSENT' }) `
            -Path $DesktopWorkerPath
        if ($Present -or $Helper.Present -or $DesktopWorker.Present) {
            Write-Output '[RDP] Removed the managed PsExec files.'
        } else {
            Write-Output '[RDP] Managed PsExec files were already absent.'
        }
        exit 0
    }

    if (-not $Present) {
        throw 'Managed PsExec is absent. Run .peer psexec add first.'
    }
    if (-not $Signature.IsTrusted) {
        throw (
            'Managed PsExec does not have a valid Microsoft signature. ' +
            'Run .peer psexec add to replace it.'
        )
    }
    if ($Request.Action -eq 'desktop') {
        if (-not $DesktopWorker.Ready) {
            throw (
                'Managed PsExec desktop worker is absent or outdated. ' +
                'Run .peer psexec add first.'
            )
        }
        $DesktopSessionId = [int]0
        if (-not [int]::TryParse(
            [string]$Request.SessionId,
            [ref]$DesktopSessionId
        ) -or $DesktopSessionId -le 0) {
            throw 'Desktop task session ID must be a positive integer.'
        }
        $DesktopRequestBase64 = [string]$Request.DesktopRequestBase64
        if ($DesktopRequestBase64 -notmatch '^[A-Za-z0-9+/=]+$' -or
            $DesktopRequestBase64.Length -gt 8192) {
            throw 'Desktop task request is invalid or too large.'
        }
        $DesktopTimeoutSeconds = [int]0
        if (-not [int]::TryParse(
            [string]$Request.DesktopTimeoutSeconds,
            [ref]$DesktopTimeoutSeconds
        ) -or $DesktopTimeoutSeconds -lt 1 -or
            $DesktopTimeoutSeconds -gt 600) {
            throw 'Desktop task timeout must be between 1 and 600 seconds.'
        }

        Write-RdpClientPsExecHeader `
            -Title 'desktop task' `
            -PeerAddress $PeerAddress `
            -Architecture $Architecture
        $DesktopResultPath = Join-Path $ManagedDirectory (
            '.desktop-result-' + [Guid]::NewGuid().ToString('N') + '.txt'
        )
        $DesktopProcessIdentityPath = Join-Path $ManagedDirectory (
            '.desktop-identity-' + [Guid]::NewGuid().ToString('N') + '.json'
        )
        $DesktopWorkerFinished = $false
        try {
            $LauncherExitCode = Invoke-RdpClientUncapturedProcess `
                -FilePath $ManagedPath `
                -TimeoutSeconds ([Math]::Min(30, $DesktopTimeoutSeconds)) `
                -Arguments @(
                    '-accepteula',
                    '-nobanner',
                    '-i',
                    [string]$DesktopSessionId,
                    '-s',
                    '-d',
                    'powershell.exe',
                    '-NoLogo',
                    '-NoProfile',
                    '-NonInteractive',
                    '-WindowStyle',
                    'Hidden',
                    '-ExecutionPolicy',
                    'Bypass',
                    '-File',
                    $DesktopWorkerPath,
                    '-RequestBase64',
                    $DesktopRequestBase64,
                    '-ResultPath',
                    $DesktopResultPath,
                    '-ProcessIdentityPath',
                    $DesktopProcessIdentityPath
                )
            # PsExec 2.43 may expose the detached child PID as its process exit
            # code. The worker-created identity file is the start authority.
            Wait-RdpClientDesktopWorkerIdentityFile `
                -Path $DesktopProcessIdentityPath `
                -TimeoutSeconds ([Math]::Min(10, $DesktopTimeoutSeconds)) `
                -LauncherExitCode $LauncherExitCode
            $DesktopResult = Wait-RdpClientDesktopResultFile `
                -Path $DesktopResultPath `
                -Encoding $Utf8 `
                -TimeoutSeconds $DesktopTimeoutSeconds
            $DesktopWorkerFinished = $true
            Write-Output $DesktopResult.Marker
            exit $(if ($DesktopResult.Success) { 0 } else { 1 })
        } finally {
            if (-not $DesktopWorkerFinished) {
                Stop-RdpClientDesktopWorkerProcess `
                    -IdentityPath $DesktopProcessIdentityPath
            }
            if ([IO.File]::Exists($DesktopResultPath)) {
                Remove-Item -LiteralPath $DesktopResultPath -Force
            }
            if ([IO.File]::Exists($DesktopProcessIdentityPath)) {
                Remove-Item -LiteralPath $DesktopProcessIdentityPath -Force
            }
        }
    }
    if ($Request.Action -eq 'launch') {
        if (-not $Helper.Ready) {
            throw (
                'Managed PsExec session helper is absent or outdated. ' +
                'Run .peer psexec add first.'
            )
        }
        $LaunchSessionId = [int]$Request.SessionId
        if ($LaunchSessionId -le 0) {
            throw 'PsExec session ID must be a positive integer.'
        }
        $LaunchPayloadJson = [ordered]@{
            Arguments = @($Request.Arguments)
        } | ConvertTo-Json -Compress -Depth 3
        $LaunchPayloadBase64 = [Convert]::ToBase64String(
            $Utf8.GetBytes($LaunchPayloadJson)
        )
        Write-RdpClientPsExecHeader `
            -Title 'session launch' `
            -PeerAddress $PeerAddress `
            -Architecture $Architecture
        $Invocation = Invoke-RdpClientCapturedProcess `
            -FilePath $ManagedPath `
            -Arguments @(
                '-accepteula',
                '-nobanner',
                '-s',
                'powershell.exe',
                '-NoLogo',
                '-NoProfile',
                '-NonInteractive',
                '-ExecutionPolicy',
                'Bypass',
                '-File',
                $HelperPath,
                '-SessionId',
                [string]$LaunchSessionId,
                '-PayloadBase64',
                $LaunchPayloadBase64
            )
        foreach ($Text in @($Invocation.StdOut, $Invocation.StdErr)) {
            $Text -split '[\r\n]+' | Where-Object {
                $_.Length -gt 0
            } | ForEach-Object { Write-Output $_ }
        }
        exit $Invocation.ExitCode
    }

    Write-RdpClientPsExecHeader `
        -Title 'run' `
        -PeerAddress $PeerAddress `
        -Architecture $Architecture
    $PsExecArguments = @($Request.Arguments)
    if (-not @($PsExecArguments | Where-Object {
        [string]::Equals(
            [string]$_,
            '-accepteula',
            [StringComparison]::OrdinalIgnoreCase
        )
    }).Count) {
        $PsExecArguments = @('-accepteula') + $PsExecArguments
    }
    $Invocation = Invoke-RdpClientCapturedProcess `
        -FilePath $ManagedPath `
        -Arguments $PsExecArguments
    $CombinedOutput = $Invocation.StdOut + "`n" + $Invocation.StdErr
    $Detached = @($PsExecArguments | Where-Object {
        [string]::Equals(
            [string]$_,
            '-d',
            [StringComparison]::OrdinalIgnoreCase
        )
    }).Count -gt 0
    $ExitCode = $Invocation.ExitCode
    if ($Detached -and $ExitCode -ne 0 -and $CombinedOutput -match
        '(?m)^.+ started on .+ with process ID [0-9]+\.\s*$') {
        # PsExec 2.43 returns 1 after a successful detached launch. Its stable
        # PID confirmation distinguishes that case from a start failure.
        $ExitCode = 0
    }
    foreach ($Text in @($Invocation.StdOut, $Invocation.StdErr)) {
        $Text -split '[\r\n]+' | Where-Object { $_.Length -gt 0 } | ForEach-Object {
            Write-Output $_
        }
    }
    exit $ExitCode
} catch {
    foreach ($UploadPath in @($HelperUploadPath, $DesktopWorkerUploadPath)) {
        if (-not [string]::IsNullOrWhiteSpace($UploadPath) -and
            [IO.File]::Exists($UploadPath)) {
            try { Remove-Item -LiteralPath $UploadPath -Force } catch { }
        }
    }
    Write-Output "[ERROR] $($_.Exception.Message)"
    exit 1
}
