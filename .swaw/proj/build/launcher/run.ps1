$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if ($args.Count -ne 0) {
    throw 'proj.build.launcher does not accept dynamic arguments.'
}

$KernelRoot = [IO.Path]::GetFullPath(
    (Join-Path ([string]$env:SWAWKIT_HOME) '_lib\proj')
)
$BuildScript = Join-Path $KernelRoot '_launcher\build.ps1'
if (-not [IO.File]::Exists($BuildScript)) {
    throw "Launcher build script not found: $BuildScript"
}

& $BuildScript
