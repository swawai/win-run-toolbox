[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$BootstrapRoot = [IO.Path]::GetFullPath(
    (Join-Path $PSScriptRoot '..\_bootstrap')
)
. (Join-Path $BootstrapRoot '_lib\layout.ps1')
. (Join-Path $BootstrapRoot 'toolchains\runtime.ps1')

function Invoke-ProjLauncherBuildEnvironmentIsolated {
    param([Parameter(Mandatory = $true)][scriptblock]$Action)

    $Snapshot = [Collections.Generic.Dictionary[string, string]]::new(
        [StringComparer]::Ordinal
    )
    $Before = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    foreach ($Name in [string[]]@($Before.Keys)) {
        $Snapshot[$Name] = [string]$Before[$Name]
    }

    try {
        & $Action
    } finally {
        $After = [Environment]::GetEnvironmentVariables(
            [EnvironmentVariableTarget]::Process
        )
        foreach ($Name in [string[]]@($After.Keys)) {
            if (-not $Snapshot.ContainsKey($Name)) {
                [Environment]::SetEnvironmentVariable(
                    $Name,
                    $null,
                    [EnvironmentVariableTarget]::Process
                )
            }
        }
        $Restored = [Environment]::GetEnvironmentVariables(
            [EnvironmentVariableTarget]::Process
        )
        foreach ($Pair in $Snapshot.GetEnumerator()) {
            $Name = [string]$Pair.Key
            $Value = [string]$Pair.Value
            if ($Restored.Contains($Name) -and
                [string]$Restored[$Name] -ceq $Value) {
                continue
            }
            [Environment]::SetEnvironmentVariable(
                $Name,
                $Value,
                [EnvironmentVariableTarget]::Process
            )
        }
    }
}

function Initialize-ProjLauncherBuildCore {
    param([Parameter(Mandatory = $true)][object]$Layout)

    if ([IO.File]::Exists($Layout.RuntimePath)) {
        return
    }
    $CoreBootstrapPath = Join-Path $Layout.BootstrapRoot 'run.ps1'
    if (-not [IO.File]::Exists($CoreBootstrapPath)) {
        throw "The Proj Core Bootstrap is missing: $CoreBootstrapPath"
    }

    $global:LASTEXITCODE = 0
    & $CoreBootstrapPath
    $BootstrapExitCode = $LASTEXITCODE
    if ($BootstrapExitCode -ne 0) {
        throw "Proj Core Bootstrap failed with exit code $BootstrapExitCode."
    }
    if (-not [IO.File]::Exists($Layout.RuntimePath)) {
        throw "Proj Core Bootstrap did not publish its runtime: $($Layout.RuntimePath)"
    }
}

function Resolve-ProjLauncherBuildMsvcExecutable {
    param([Parameter(Mandatory = $true)][string]$Name)

    $ManagedRootValue = [string]$env:SWAWKIT_PROJ_DEV_MSVC_HOME
    if ([string]$env:SWAWKIT_PROJ_DEV_ENV_SCHEMA -cne
            'swawkit.proj-dev.environment.v0' -or
        [string]$env:SWAWKIT_PROJ_DEV_MSVC_MODE -cne 'managed' -or
        [string]::IsNullOrWhiteSpace($ManagedRootValue) -or
        [string]::IsNullOrWhiteSpace(
            [string]$env:SWAWKIT_PROJ_DEV_MSVC_SIGNATURE
        )) {
        throw 'The Bootstrap managed MSVC environment is incomplete.'
    }

    $Command = Get-Command $Name `
        -CommandType Application `
        -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $Command) {
        throw "The Bootstrap managed MSVC environment does not expose $Name."
    }

    $ManagedRoot = [IO.Path]::GetFullPath($ManagedRootValue).TrimEnd(
        '\', '/'
    ) + [IO.Path]::DirectorySeparatorChar
    $ExecutablePath = [IO.Path]::GetFullPath([string]$Command.Source)
    if (-not $ExecutablePath.StartsWith(
        $ManagedRoot,
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw "$Name resolved outside Bootstrap managed MSVC: $ExecutablePath"
    }
    return $ExecutablePath
}

function Assert-ProjLauncherBuildPhysicalDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Description
    )

    $FullPath = [IO.Path]::GetFullPath($Path)
    if (-not [IO.Directory]::Exists($FullPath)) {
        [void][IO.Directory]::CreateDirectory($FullPath)
    }
    $Item = Get-Item -LiteralPath $FullPath -Force
    if (-not $Item.PSIsContainer -or
        ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Description is unsafe: $FullPath"
    }
    return $Item
}

$Layout = Get-ProjBootstrapLayout

Invoke-ProjLauncherBuildEnvironmentIsolated -Action {
    Initialize-ProjLauncherBuildCore -Layout $Layout
    $Toolchain = Initialize-ProjBootstrapToolchain
    $CompilerPath = Resolve-ProjLauncherBuildMsvcExecutable -Name 'cl.exe'
    $LinkerPath = Resolve-ProjLauncherBuildMsvcExecutable -Name 'link.exe'
    $BuildRoot = Assert-ProjDevPathInsideDataRoot `
        -Path $Layout.LauncherBuildRoot `
        -DataRoot $Toolchain.Context.DataRoot `
        -Activity 'building the Proj Launcher'
    $CandidatePath = Assert-ProjDevPathInsideDataRoot `
        -Path $Layout.LauncherCandidatePath `
        -DataRoot $Toolchain.Context.DataRoot `
        -Activity 'publishing the Proj Launcher build candidate'
    [void](Assert-ProjLauncherBuildPhysicalDirectory `
        -Path $BuildRoot `
        -Description 'The Launcher build directory')
    [void](Assert-ProjLauncherBuildPhysicalDirectory `
        -Path (Split-Path -Path $CandidatePath -Parent) `
        -Description 'The Launcher candidate directory')

    $BuildLock = Enter-ProjDevFileLock `
        -Path (Join-Path $Layout.LockRoot 'launcher-build.lock') `
        -ControlledRoot $Toolchain.Context.DataRoot `
        -TimeoutSeconds 1800
    try {
        $SourcePath = Join-Path $PSScriptRoot 'launcher.c'
        $ObjectPath = Assert-ProjDevPathInsideDataRoot `
            -Path (Join-Path $BuildRoot 'launcher.obj') `
            -DataRoot $Toolchain.Context.DataRoot `
            -Activity 'writing the Proj Launcher object file'
        $StagedPath = Assert-ProjDevPathInsideDataRoot `
            -Path (Join-Path $BuildRoot 'template.proj1.exe') `
            -DataRoot $Toolchain.Context.DataRoot `
            -Activity 'writing the staged Proj Launcher executable'

        [string[]]$CompileArguments = @(
            '/nologo'
            '/Brepro'
            '/W4'
            '/WX'
            '/TC'
            '/c'
            '/O1'
            '/Os'
            '/Oi'
            '/Gy'
            '/Gw'
            '/Zl'
            "/Fo$ObjectPath"
            $SourcePath
        )
        & $CompilerPath @CompileArguments
        if ($LASTEXITCODE -ne 0) {
            throw "cl.exe failed with exit code $LASTEXITCODE."
        }

        [string[]]$LinkArguments = @(
            '/nologo'
            '/Brepro'
            "/OUT:$StagedPath"
            '/ENTRY:launcher_entry'
            '/SUBSYSTEM:CONSOLE'
            '/MACHINE:X64'
            '/NODEFAULTLIB'
            '/INCREMENTAL:NO'
            '/OPT:REF'
            '/OPT:ICF'
            '/DEBUG:NONE'
            '/MANIFEST:NO'
            '/DYNAMICBASE'
            '/HIGHENTROPYVA'
            '/NXCOMPAT'
            $ObjectPath
            'kernel32.lib'
            'user32.lib'
        )
        & $LinkerPath @LinkArguments
        if ($LASTEXITCODE -ne 0) {
            throw "link.exe failed with exit code $LASTEXITCODE."
        }

        $StagedItem = Get-Item -LiteralPath $StagedPath
        if ($StagedItem.Length -le 0 -or $StagedItem.Length -gt 64KB) {
            throw (
                "Unexpected launcher size $($StagedItem.Length) bytes; expected a " +
                'non-empty thin executable no larger than 64 KiB.'
            )
        }

        $RuntimeTest = Join-Path $PSScriptRoot '..\_test\launcher-runtime.ps1'
        if (-not [IO.File]::Exists($RuntimeTest)) {
            throw "Launcher runtime test not found: $RuntimeTest"
        }
        & $RuntimeTest -LauncherPath $StagedPath

        $CandidateParent = Split-Path -Path $CandidatePath -Parent
        $PublishPath = Join-Path $CandidateParent (
            ".$([IO.Path]::GetFileName($CandidatePath))." +
            "$([Guid]::NewGuid().ToString('N')).tmp"
        )
        $BackupPath = Join-Path $CandidateParent (
            ".$([IO.Path]::GetFileName($CandidatePath))." +
            "$([Guid]::NewGuid().ToString('N')).backup"
        )
        try {
            [IO.File]::Copy($StagedPath, $PublishPath, $false)
            if ([IO.File]::Exists($CandidatePath)) {
                [IO.File]::Replace(
                    $PublishPath,
                    $CandidatePath,
                    $BackupPath,
                    $true
                )
            } else {
                [IO.File]::Move($PublishPath, $CandidatePath)
            }
        } finally {
            if ([IO.File]::Exists($PublishPath)) {
                [IO.File]::Delete($PublishPath)
            }
            if ([IO.File]::Exists($BackupPath)) {
                [IO.File]::Delete($BackupPath)
            }
        }

        $OutputItem = Get-Item -LiteralPath $CandidatePath
        Write-Host (
            "[BUILT] $($OutputItem.FullName) ($($OutputItem.Length) bytes)"
        ) -ForegroundColor Green
        $OutputItem | Select-Object FullName, Length, LastWriteTime
    } finally {
        $BuildLock.Dispose()
    }
}

$global:LASTEXITCODE = 0
