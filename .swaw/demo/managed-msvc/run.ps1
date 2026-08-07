$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if ($args.Count -ne 0) {
    throw 'demo.managed-msvc does not accept dynamic arguments.'
}

$KernelRoot = [IO.Path]::GetFullPath(
    (Join-Path ([string]$env:SWAWKIT_HOME) '_lib\proj')
)
. (Join-Path $KernelRoot '_toolchain\_modules\msvc\runtime.ps1')
[void](Import-ProjDevMsvcCommandEnvironment)

# Command-owned precondition policy:
# This module requires project-managed MSVC. A different module may
# intentionally accept an ambient tool instead; Core does not decide for it.
if ([string]$env:SWAWKIT_PROJ_DEV_ENV_SCHEMA -cne
        'swawkit.proj-dev.environment.v0' -or
    [string]$env:SWAWKIT_PROJ_DEV_MSVC_MODE -cne 'managed' -or
    [string]::IsNullOrWhiteSpace(
        [string]$env:SWAWKIT_PROJ_DEV_MSVC_HOME
    ) -or
    [string]::IsNullOrWhiteSpace(
        [string]$env:SWAWKIT_PROJ_DEV_MSVC_SIGNATURE
    )) {
    throw (
        'demo.managed-msvc requires the project-managed MSVC environment. ' +
        "Enable it and run " +
        "'$($env:SWAWKIT_PROJ_ENTRY_COMMAND) .dev.setup'."
    )
}

$ManagedRoot = [IO.Path]::GetFullPath(
    [string]$env:SWAWKIT_PROJ_DEV_MSVC_HOME
).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
$ResolvedTools = foreach ($Name in @('cl.exe', 'link.exe')) {
    $Command = Get-Command $Name `
        -CommandType Application `
        -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $Command) {
        throw "Managed MSVC does not expose $Name."
    }
    $ExecutablePath = [IO.Path]::GetFullPath([string]$Command.Source)
    if (-not $ExecutablePath.StartsWith(
        $ManagedRoot,
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw (
            "$Name resolved outside the project-managed MSVC environment: " +
            "$ExecutablePath"
        )
    }
    [pscustomobject]@{
        Name = $Name
        Path = $ExecutablePath
    }
}

[Console]::WriteLine('policy=managed-only')
foreach ($Tool in $ResolvedTools) {
    [Console]::WriteLine("$($Tool.Name)=$($Tool.Path)")
}
$global:LASTEXITCODE = 0
