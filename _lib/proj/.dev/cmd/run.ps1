$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

[string[]]$CommandArguments = @($args)
if ($CommandArguments.Count -eq 0) {
    throw '.dev.cmd requires non-empty command text.'
}
[string]$CommandText = [string]::Join(' ', $CommandArguments)
if ([string]::IsNullOrWhiteSpace($CommandText)) {
    throw '.dev.cmd requires non-empty command text.'
}

$KernelRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
. (Join-Path $KernelRoot '_toolchain\runtime.ps1')
. (Join-Path $KernelRoot '_shell\runtime.ps1')

$Context = New-ProjDevContextFromEnvironment
[void](Import-ProjDevOptionalGeneratedEnvironment -Context $Context)
[void](Initialize-ProjShellCommandEnvironment -KernelRoot $KernelRoot)
$CmdPath = Get-ProjSystemCmdPath
& $CmdPath /d /s /v:off /c $CommandText
exit ([int]$LASTEXITCODE)
