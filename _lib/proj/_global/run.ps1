$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if (@($args).Count -ne 0) {
    throw 'The global execution guard does not accept arguments.'
}

$KernelRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $KernelRoot '.dev\setup\_lib\runtime.ps1')

$Context = New-ProjDevContextFromEnvironment
# Global policy only protects process identity: a shell may not mix projects
# or keep using an environment after another process publishes a new generation.
# Declaration freshness remains command-owned, so .dev.setup can repair changes
# without a Core whitelist and environment-agnostic commands remain independent.
[void](Assert-ProjDevActiveEnvironmentPublished -Context $Context)

$global:LASTEXITCODE = 0
