Set-StrictMode -Version 2.0

function Get-ProjDevFullPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    $FullPath = [IO.Path]::GetFullPath($Path)
    $PathRoot = [IO.Path]::GetPathRoot($FullPath)
    if ($FullPath.Length -gt $PathRoot.Length) {
        $FullPath = $FullPath.TrimEnd([char[]]@(
            [IO.Path]::DirectorySeparatorChar,
            [IO.Path]::AltDirectorySeparatorChar
        ))
    }
    return $FullPath
}

. (Join-Path $PSScriptRoot 'controlled-path.ps1')

function Get-ProjDevCanonicalPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    return (Get-ProjDevFullPath -Path $Path).ToUpperInvariant()
}

function Get-ProjDevSafeSegment {
    param(
        [Parameter(Mandatory = $true)][string]$Value,
        [Parameter(Mandatory = $true)][string]$Description
    )

    if ($Value -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._+-]*$') {
        throw "Invalid $Description '$Value'."
    }
    return $Value
}

function Get-ProjDevRequiredEnvironmentValue {
    param([Parameter(Mandatory = $true)][string]$Name)

    $Value = [Environment]::GetEnvironmentVariable(
        $Name,
        [EnvironmentVariableTarget]::Process
    )
    if ([string]::IsNullOrWhiteSpace($Value)) {
        throw "Required project declaration is missing: $Name"
    }
    return [string]$Value
}

function New-ProjDevContext {
    param(
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$CacheDataRoot,
        [string]$EntryCommand = 'swawkit',
        [AllowNull()][string]$InvocationDirectory = $null
    )

    $ResolvedProjectRoot = Get-ProjDevFullPath -Path $ProjectRoot
    $ResolvedDataRoot = Assert-ProjDevControlledRoot `
        -Root $DataRoot `
        -Description 'Project development data root'
    $ResolvedCacheDataRoot = Assert-ProjDevControlledRoot `
        -Root $CacheDataRoot `
        -Description 'Shared project cache data root'
    if (-not [IO.Directory]::Exists($ResolvedProjectRoot)) {
        throw "Declared project directory does not exist: $ResolvedProjectRoot"
    }

    if ([string]::IsNullOrWhiteSpace($InvocationDirectory)) {
        $ResolvedInvocationDirectory = $ResolvedProjectRoot
    } else {
        $ResolvedInvocationDirectory = Get-ProjDevFullPath -Path $InvocationDirectory
    }
    if (-not [IO.Directory]::Exists($ResolvedInvocationDirectory)) {
        throw "Invocation directory does not exist: $ResolvedInvocationDirectory"
    }

    $EnvironmentRoot = Assert-ProjDevelopmentEnvironmentControlledRoot `
        -EnvironmentRoot (Join-Path $ResolvedDataRoot 'dev_env')
    $LockRoot = Join-Path $ResolvedDataRoot '_locks'
    return [pscustomobject][ordered]@{
        ProjectRoot = $ResolvedProjectRoot
        CanonicalProjectRoot = Get-ProjDevCanonicalPath -Path $ResolvedProjectRoot
        DataRoot = $ResolvedDataRoot
        CacheDataRoot = $ResolvedCacheDataRoot
        EnvironmentRoot = $EnvironmentRoot
        EnvCmdPath = Join-Path $EnvironmentRoot 'env.cmd'
        EnvPs1Path = Join-Path $EnvironmentRoot 'env.ps1'
        EnvironmentStatePath = Get-ProjDevelopmentEnvironmentStatePath `
            -EnvironmentRoot $EnvironmentRoot
        CacheRoot = Join-Path $ResolvedCacheDataRoot 'downloads'
        LockRoot = $LockRoot
        SetupLockPath = Join-Path $LockRoot 'dev-setup.lock'
        ArtifactLockRoot = Join-Path $ResolvedCacheDataRoot '_locks'
        EntryCommand = $EntryCommand
        InvocationDirectory = $ResolvedInvocationDirectory
    }
}

function New-ProjDevContextFromEnvironment {
    if ([string]$env:SWAWKIT_PROJ_PROTOCOL -cne '1') {
        throw 'Unsupported or missing SWAWKIT_PROJ_PROTOCOL. Expected protocol version 1.'
    }

    $InvocationDirectory = [Environment]::GetEnvironmentVariable(
        'SWAWKIT_PROJ_INVOCATION_DIR',
        [EnvironmentVariableTarget]::Process
    )
    $ProjHome = Get-ProjDevFullPath -Path (
        Get-ProjDevRequiredEnvironmentValue -Name 'SWAWKIT_HOME'
    )
    if (-not [IO.Directory]::Exists($ProjHome)) {
        throw "Declared Swaw Kit Proj home does not exist: $ProjHome"
    }
    return New-ProjDevContext `
        -ProjectRoot (Get-ProjDevRequiredEnvironmentValue -Name 'SWAWKIT_PROJ_TARGET_PROJECT_ROOT') `
        -DataRoot (Get-ProjDevRequiredEnvironmentValue -Name 'SWAWKIT_PROJ_DATA_ROOT') `
        -CacheDataRoot (Join-Path $ProjHome 'data\proj_cache') `
        -EntryCommand (Get-ProjDevRequiredEnvironmentValue -Name 'SWAWKIT_PROJ_ENTRY_COMMAND') `
        -InvocationDirectory $InvocationDirectory
}

function Assert-ProjDevActiveEnvironmentCompatible {
    param([Parameter(Mandatory = $true)][object]$Context)

    $ActiveProjectRoot = [Environment]::GetEnvironmentVariable(
        'SWAWKIT_PROJ_DEV_PROJECT_ROOT',
        [EnvironmentVariableTarget]::Process
    )
    $ActiveEnvironmentRoot = [Environment]::GetEnvironmentVariable(
        'SWAWKIT_PROJ_DEV_ENV_ROOT',
        [EnvironmentVariableTarget]::Process
    )
    if ([string]::IsNullOrWhiteSpace($ActiveProjectRoot) -and
        [string]::IsNullOrWhiteSpace($ActiveEnvironmentRoot)) {
        $ProcessEnvironment = [Environment]::GetEnvironmentVariables(
            [EnvironmentVariableTarget]::Process
        )
        $HasOwnedState = @($ProcessEnvironment.Keys | Where-Object {
            ([string]$_).StartsWith(
                'SWAWKIT_PROJ_DEV_',
                [StringComparison]::OrdinalIgnoreCase
            )
        }).Count -gt 0
        if ($HasOwnedState) {
            throw (
                'An incomplete Swaw Kit development environment is already ' +
                'active. Exit that shell before continuing.'
            )
        }
        return $false
    }
    if ([string]::IsNullOrWhiteSpace($ActiveProjectRoot) -or
        [string]::IsNullOrWhiteSpace($ActiveEnvironmentRoot)) {
        throw (
            'An incomplete Swaw Kit development environment is already active. ' +
            'Exit that shell before continuing.'
        )
    }

    try {
        $SameProject = (Get-ProjDevCanonicalPath -Path $ActiveProjectRoot).
            Equals(
                $Context.CanonicalProjectRoot,
                [StringComparison]::OrdinalIgnoreCase
            )
        $SameEnvironment = (Get-ProjDevCanonicalPath `
            -Path $ActiveEnvironmentRoot).Equals(
                (Get-ProjDevCanonicalPath -Path $Context.EnvironmentRoot),
                [StringComparison]::OrdinalIgnoreCase
            )
    } catch {
        throw (
            'The active Swaw Kit development environment has invalid paths. ' +
            'Exit that shell before continuing.'
        )
    }
    if (-not $SameProject -or -not $SameEnvironment) {
        throw (
            "Another project's development environment is already active: " +
            "$ActiveProjectRoot. Exit that shell before continuing."
        )
    }
    return $true
}

function Get-ProjDevSha256Text {
    param([Parameter(Mandatory = $true)][string]$Value)

    $Algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        $Bytes = [Text.Encoding]::UTF8.GetBytes($Value)
        return ([BitConverter]::ToString(
            $Algorithm.ComputeHash($Bytes)
        )).Replace('-', '').ToLowerInvariant()
    } finally {
        $Algorithm.Dispose()
    }
}

function Get-ProjDevFileSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not [IO.File]::Exists($Path)) {
        throw "Cannot hash a missing file: $Path"
    }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).
        Hash.ToLowerInvariant()
}

function ConvertTo-ProjDevJsonText {
    param([Parameter(Mandatory = $true)][object]$Value)

    $Json = $Value | ConvertTo-Json -Depth 10
    $Json = $Json.Replace("`r`n", "`n").
        Replace("`r", "`n").
        Replace("`n", "`r`n")
    return "$Json`r`n"
}

function Write-ProjDevTextAtomic {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Content,
        [Parameter(Mandatory = $true)][string]$ControlledRoot,
        [Text.Encoding]$Encoding = ([Text.UTF8Encoding]::new($false))
    )

    $FullPath = Assert-ProjDevPathInsideDataRoot `
        -Path $Path `
        -DataRoot $ControlledRoot `
        -Activity 'publishing controlled development state'
    $Parent = Split-Path -Path $FullPath -Parent
    [void][IO.Directory]::CreateDirectory($Parent)
    $Token = [Guid]::NewGuid().ToString('N')
    $TemporaryPath = Join-Path $Parent ".$([IO.Path]::GetFileName($FullPath)).$Token.tmp"
    $BackupPath = Join-Path $Parent ".$([IO.Path]::GetFileName($FullPath)).$Token.bak"

    try {
        [IO.File]::WriteAllText($TemporaryPath, $Content, $Encoding)
        if ([IO.File]::Exists($FullPath)) {
            [IO.File]::Replace($TemporaryPath, $FullPath, $BackupPath)
        } else {
            [IO.File]::Move($TemporaryPath, $FullPath)
        }
    } finally {
        foreach ($CleanupPath in @($TemporaryPath, $BackupPath)) {
            if ([IO.File]::Exists($CleanupPath)) {
                try {
                    [IO.File]::Delete($CleanupPath)
                } catch {
                    Write-Warning "Temporary file could not be removed: $CleanupPath"
                }
            }
        }
    }
}

function Enter-ProjDevFileLock {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ControlledRoot,
        [int]$TimeoutSeconds = 600
    )

    $FullPath = Assert-ProjDevPathInsideDataRoot `
        -Path $Path `
        -DataRoot $ControlledRoot `
        -Activity 'creating a development lock'
    [void][IO.Directory]::CreateDirectory((Split-Path -Path $FullPath -Parent))
    $Deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $Deadline) {
        try {
            return [IO.File]::Open(
                $FullPath,
                [IO.FileMode]::OpenOrCreate,
                [IO.FileAccess]::ReadWrite,
                [IO.FileShare]::None
            )
        } catch [IO.IOException] {
            Start-Sleep -Milliseconds 200
        }
    }
    throw "Timed out waiting for the project development lock: $FullPath"
}

function Remove-ProjDevControlledPath {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$Activity
    )

    $FullPath = Assert-ProjDevPathInsideDataRoot `
        -Path $Path `
        -DataRoot $DataRoot `
        -Activity $Activity
    if ([IO.Directory]::Exists($FullPath)) {
        Remove-Item -LiteralPath $FullPath -Recurse -Force
    } elseif ([IO.File]::Exists($FullPath)) {
        [IO.File]::Delete($FullPath)
    }
}

function Resolve-ProjDevChildPath {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$RelativePath,
        [Parameter(Mandatory = $true)][string]$Description
    )

    if ([string]::IsNullOrWhiteSpace($RelativePath) -or
        [IO.Path]::IsPathRooted($RelativePath)) {
        throw "Invalid $Description '$RelativePath': expected a relative path."
    }
    $FullRoot = Get-ProjDevFullPath -Path $Root
    $RootPrefix = $FullRoot.TrimEnd('\', '/') +
        [IO.Path]::DirectorySeparatorChar
    $FullPath = Get-ProjDevFullPath -Path (Join-Path $FullRoot $RelativePath)
    if (-not $FullPath.StartsWith(
        $RootPrefix,
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw "Invalid $Description '$RelativePath': path escapes its root."
    }
    return $FullPath
}

function Assert-ProjDevWindowsX64 {
    param([Parameter(Mandatory = $true)][string]$ToolName)

    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw "$ToolName V0 supports Windows x64 only."
    }
    $Architecture = if (
        -not [string]::IsNullOrWhiteSpace($env:PROCESSOR_ARCHITEW6432)
    ) {
        [string]$env:PROCESSOR_ARCHITEW6432
    } else {
        [string]$env:PROCESSOR_ARCHITECTURE
    }
    if (-not [Environment]::Is64BitOperatingSystem -or
        $Architecture.Trim().ToUpperInvariant() -cne 'AMD64') {
        throw "$ToolName V0 supports Windows x64 only; detected '$Architecture'."
    }
}
