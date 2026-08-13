$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if ($args.Count -ne 0) {
    throw 'demo.managed-msvc does not accept dynamic arguments.'
}

if ([string]$env:SWAWKIT_PROJ_MSVC_MODE -cne 'managed') {
    throw (
        'demo.managed-msvc requires the project-managed MSVC environment. ' +
        "Enable it and run " +
        "'$($env:SWAWKIT_PROJ_ENTRY_COMMAND) .dev.setup'."
    )
}

if ([string]::IsNullOrWhiteSpace([string]$env:VCToolsInstallDir)) {
    throw 'Core did not publish the managed MSVC installation environment.'
}
$ManagedRoot = [IO.Path]::GetFullPath(
    [string]$env:VCToolsInstallDir
).TrimEnd('\', '/') +
    [IO.Path]::DirectorySeparatorChar
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
