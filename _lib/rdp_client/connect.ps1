[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$EntryFile,

    [string]$CommandName = 'rdp',

    [switch]$Launch,

    [switch]$Force,

    [AllowEmptyString()]
    [string]$SshEntryFile = '',

    [AllowNull()][AllowEmptyString()]
    [string]$SessionId,

    [switch]$ReportMstscProcessId
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'entry.ps1')
. (Join-Path $PSScriptRoot 'signing-core.ps1')
. (Join-Path $PSScriptRoot 'connect-cache.ps1')
. (Join-Path $PSScriptRoot 'peer-ssh.ps1')
. (Join-Path $PSScriptRoot 'session.ps1')
. (Join-Path $PSScriptRoot 'session-connect.ps1')
. (Join-Path $PSScriptRoot 'launch-ui.ps1')

function Resolve-RdpClientOutputPath {
    param(
        [AllowNull()][AllowEmptyString()][string]$ConfiguredPath,
        [Parameter(Mandatory = $true)][string]$EntryName
    )

    if ([string]::IsNullOrWhiteSpace($ConfiguredPath)) {
        $DesktopDirectory = [Environment]::GetFolderPath(
            [Environment+SpecialFolder]::DesktopDirectory
        )
        if ([string]::IsNullOrWhiteSpace($DesktopDirectory)) {
            throw 'Windows did not provide a Desktop directory for the current user.'
        }
        return Join-Path $DesktopDirectory ($EntryName + '.rdp')
    }

    $ExpandedPath = [Environment]::ExpandEnvironmentVariables(
        $ConfiguredPath.Trim()
    )
    if (-not [IO.Path]::IsPathRooted($ExpandedPath)) {
        throw 'RDP_OUTPUT_PATH must be an absolute path.'
    }
    if (-not [string]::Equals(
        [IO.Path]::GetExtension($ExpandedPath),
        '.rdp',
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw 'RDP_OUTPUT_PATH must name an .rdp file.'
    }
    return [IO.Path]::GetFullPath($ExpandedPath)
}

function Invoke-RdpClientSetupScript {
    param(
        [Parameter(Mandatory = $true)][string]$ScriptPath,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$Description
    )

    if (-not [IO.File]::Exists($ScriptPath)) {
        throw "$Description script was not found: $ScriptPath"
    }
    $PowerShellPath = Join-Path `
        (Get-RdpClientSigningSystemDirectory) `
        'WindowsPowerShell\v1.0\powershell.exe'
    if (-not [IO.File]::Exists($PowerShellPath)) {
        throw "Native Windows PowerShell not found: $PowerShellPath"
    }

    $PreviousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Output = @(& $PowerShellPath `
            -NoLogo `
            -NoProfile `
            -ExecutionPolicy Bypass `
            -File $ScriptPath `
            @Arguments 2>&1)
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousPreference
    }
    foreach ($Line in $Output) {
        Write-Host ([string]$Line)
    }
    if ($ExitCode -ne 0) {
        throw "$Description failed with exit code $ExitCode."
    }
}

function Assert-RdpClientHostAliasReadyForLaunch {
    param(
        [AllowEmptyString()][string]$HostAlias,
        [Parameter(Mandatory = $true)][string]$EntryPath,
        [Parameter(Mandatory = $true)][string]$CommandName,
        [Parameter(Mandatory = $true)][bool]$ExplorerLaunch
    )

    try {
        Assert-RdpClientHostAliasResolves `
            -HostAlias $HostAlias `
            -CommandName $CommandName
        return
    } catch {
        if (-not $ExplorerLaunch -or
            [string]::IsNullOrWhiteSpace($HostAlias)) {
            throw
        }
    }

    $Decision = Request-RdpClientHostsInstall `
        -HostAlias $HostAlias `
        -CommandName $CommandName
    if ($Decision -ne 'Install') {
        throw [OperationCanceledException]::new(
            'Remote Desktop connection was cancelled.'
        )
    }
    Invoke-RdpClientSetupScript `
        -ScriptPath (Join-Path $PSScriptRoot 'hosts.ps1') `
        -Arguments @(
            '-EntryFile', $EntryPath,
            '-Action', 'install',
            '-HostAlias', $HostAlias,
            '-CommandName', $CommandName,
            '-Uac'
        ) `
        -Description 'RDP hosts installation'
    Assert-RdpClientHostAliasResolves `
        -HostAlias $HostAlias `
        -CommandName $CommandName
}

function Resolve-RdpClientSigningStateForLaunch {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$State,
        [Parameter(Mandatory = $true)][pscustomobject]$Configuration,
        [Parameter(Mandatory = $true)][string]$CommandName,
        [Parameter(Mandatory = $true)][bool]$ExplorerLaunch
    )

    if (-not $ExplorerLaunch -or $State.Name -ne 'Missing') {
        return $State
    }
    $Decision = Request-RdpClientSigningSetup -CommandName $CommandName
    if ($Decision -eq 'ContinueUnsigned') {
        return $State
    }
    if ($Decision -ne 'Install') {
        throw [OperationCanceledException]::new(
            'Remote Desktop connection was cancelled.'
        )
    }
    Invoke-RdpClientSetupScript `
        -ScriptPath (Join-Path $PSScriptRoot 'signing.ps1') `
        -Arguments @(
            '-Action', 'install',
            '-CommandName', $CommandName
        ) `
        -Description 'RDP signing installation'
    $InstalledState = Get-RdpClientSigningState `
        -Configuration $Configuration
    if ($InstalledState.Name -ne 'Ready') {
        throw (
            'RDP signing installation did not reach Ready state: ' +
            "$($InstalledState.Name)."
        )
    }
    return $InstalledState
}

$ExplorerLaunch = $false
try {
    $Utf8NoBom = New-Object Text.UTF8Encoding($false)
    [Console]::OutputEncoding = $Utf8NoBom
    $OutputEncoding = $Utf8NoBom

    $ResolvedEntry = [IO.Path]::GetFullPath($EntryFile)
    $ExplorerLaunch = $Launch -and (Test-RdpClientExplorerLaunch)
    $HostAlias = Resolve-RdpClientHostAlias -Value $env:RDP_HOST_ALIAS
    $Document = Read-RdpClientEntryDocument -Path $ResolvedEntry
    $HasSessionId = $PSBoundParameters.ContainsKey('SessionId')
    if ($HasSessionId -and -not $Launch) {
        throw 'Session selectors can only be used when launching Remote Desktop.'
    }
    if ($ReportMstscProcessId -and -not $Launch) {
        throw 'An mstsc process ID can only be reported when launching Remote Desktop.'
    }
    if ($Launch) {
        Assert-RdpClientHostAliasReadyForLaunch `
            -HostAlias $HostAlias `
            -EntryPath $ResolvedEntry `
            -CommandName $CommandName `
            -ExplorerLaunch $ExplorerLaunch
    }

    $SigningConfiguration = Get-RdpClientSigningConfiguration
    $SigningState = Get-RdpClientSigningState `
        -Configuration $SigningConfiguration
    if ($Launch) {
        $SigningState = Resolve-RdpClientSigningStateForLaunch `
            -State $SigningState `
            -Configuration $SigningConfiguration `
            -CommandName $CommandName `
            -ExplorerLaunch $ExplorerLaunch
    }
    $SigningIdentity = Get-RdpClientSigningIdentity `
        -State $SigningState `
        -CommandName $CommandName

    $SelectedSession = $null
    $SessionState = $null
    $ResolvedSshEntry = $null
    $ResolvedSessionId = $null
    if ($Launch -and $HasSessionId) {
        $ResolvedSshEntry = Resolve-RdpClientPeerSshEntryPath -Value $SshEntryFile
        Assert-RdpClientPeerSshEntryIsSeparate `
            -SshEntryPath $ResolvedSshEntry `
            -RdpEntryPath $ResolvedEntry
        $SessionState = Get-RdpClientPeerSessionState `
            -SshEntryPath $ResolvedSshEntry
        $ResolvedSessionId = Resolve-RdpClientSessionId `
            -Value $SessionId
        if ($null -eq $ResolvedSessionId) {
            throw 'Session ID is required.'
        }
        $SelectedSession = Resolve-RdpClientSessionSelection `
            -State $SessionState `
            -EntryUserName $Document.Username `
            -SessionId $ResolvedSessionId
    }
    $RdpLines = ConvertTo-RdpClientOutputLines `
        -Document $Document `
        -HostAlias $HostAlias

    $EntryName = [IO.Path]::GetFileNameWithoutExtension($ResolvedEntry)
    if ([string]::IsNullOrWhiteSpace($EntryName)) {
        throw 'Could not derive an RDP output name from the entry file.'
    }
    $OutputPath = Resolve-RdpClientOutputPath `
        -ConfiguredPath $env:RDP_OUTPUT_PATH `
        -EntryName $EntryName
    $OutputDirectory = [IO.Path]::GetDirectoryName($OutputPath)
    if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
        throw "Could not derive the RDP output directory from: $OutputPath"
    }
    [IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null

    if ([IO.File]::Exists($OutputPath) -and -not $Launch -and -not $Force) {
        throw "RDP file already exists: $OutputPath Run `"$CommandName .rdp create --force`" to overwrite it."
    }

    $SourceHash = Get-RdpClientSourceHash -Lines $RdpLines
    $ManifestPath = Get-RdpClientManifestPath `
        -RuntimeDirectory $PSScriptRoot `
        -EntryName $EntryName
    $ReuseArtifact = $Launch -and (Test-RdpClientArtifactIsCurrent `
        -ManifestPath $ManifestPath `
        -EntryPath $ResolvedEntry `
        -OutputPath $OutputPath `
        -SourceHash $SourceHash `
        -SigningIdentity $SigningIdentity)

    if ($ReuseArtifact) {
        Write-Host "[RDP] Reused:    $OutputPath"
        if ($SigningState.Name -eq 'Ready') {
            Write-Host "[RDP] Signed:     $($SigningConfiguration.FriendlyName) (unchanged)"
        } else {
            Write-RdpClientSigningMissingNotice `
                -CommandName $CommandName `
                -Cached
        }
    } else {
        # mstsc and rdpsign use the native Windows RDP text representation.
        [IO.File]::WriteAllLines($OutputPath, $RdpLines, [Text.Encoding]::Unicode)
        Write-Host "[RDP] Generated: $OutputPath"
        $null = Invoke-RdpClientFileSigning `
            -Path $OutputPath `
            -CommandName $CommandName `
            -Configuration $SigningConfiguration
        Write-RdpClientArtifactManifest `
            -ManifestPath $ManifestPath `
            -EntryPath $ResolvedEntry `
            -OutputPath $OutputPath `
            -SourceHash $SourceHash `
            -SigningIdentity $SigningIdentity
    }

    Write-Host "[RDP] Target:    $($RdpLines | Where-Object { $_ -like 'full address:*' } | Select-Object -First 1)"
    if ($null -ne $SelectedSession) {
        $SelectedUser = Get-RdpClientSessionDisplayUserName `
            -Session $SelectedSession
        Write-Host (
            '[RDP] Requested: session {0} ({1}; {2}; {3})' -f `
                $SelectedSession.Id,
                $SelectedUser,
                $SelectedSession.State,
                $SelectedSession.Terminal
        )
    }

    if ($Launch) {
        Assert-RdpClientHostAliasResolves `
            -HostAlias $HostAlias `
            -CommandName $CommandName

        $Mstsc = Get-Command 'mstsc.exe' -ErrorAction Stop
        $MstscProcess = Start-Process `
            -FilePath $Mstsc.Source `
            -ArgumentList ('"{0}"' -f $OutputPath) `
            -PassThru
        Write-Host '[RDP] Started mstsc.exe.'
        if ($ReportMstscProcessId) {
            Write-Output ('RDP_CLIENT_MSTSC_PROCESS_V1:' + $MstscProcess.Id)
        }
        if ($null -ne $SelectedSession) {
            $null = Connect-RdpClientSessionById `
                -SshEntryPath $ResolvedSshEntry `
                -BeforeState $SessionState `
                -EntryUserName $Document.Username `
                -TargetSessionId $ResolvedSessionId `
                -MstscProcess $MstscProcess
        }
    }

    exit 0
} catch {
    $FailureMessage = [string]$_.Exception.Message
    [Console]::Error.WriteLine("[ERROR] $FailureMessage")
    if ($ExplorerLaunch -and
        $_.Exception -isnot [OperationCanceledException]) {
        Show-RdpClientLaunchError `
            -Message $FailureMessage `
            -CommandName $CommandName
    }
    exit 1
}
