[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjDevelopmentCommandLayout {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "Development command layout test failed: $Message"
    }
}

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Assert-ProjDevelopmentCommandLayout `
    -Condition (-not [IO.Directory]::Exists((Join-Path $ProjRoot '_global'))) `
    -Message 'the removed no-op global guard directory still exists'

foreach ($Name in @('bun', 'cargo', 'cl', 'rustc', 'cmd', 'ps')) {
    $LegacyPath = Join-Path $ProjRoot ".$Name"
    $EntryPath = Join-Path $ProjRoot ".dev\$Name\run.ps1"

    Assert-ProjDevelopmentCommandLayout `
        -Condition (-not (Test-Path -LiteralPath $LegacyPath)) `
        -Message "legacy command .$Name still exists"
    Assert-ProjDevelopmentCommandLayout `
        -Condition ([IO.File]::Exists($EntryPath)) `
        -Message ".dev.$Name does not have a PowerShell entry"
}

$SetupRoot = Join-Path $ProjRoot '.dev\setup'
$SetupManifest = Join-Path $SetupRoot 'run.toolchain.json'
Assert-ProjDevelopmentCommandLayout `
    -Condition ([IO.File]::Exists($SetupManifest) -and
        -not [IO.File]::Exists((Join-Path $SetupRoot 'run.ps1'))) `
    -Message '.dev.setup did not converge to one native Toolchain entry'
$SetupContract = Get-Content -LiteralPath $SetupManifest -Raw |
    ConvertFrom-Json
Assert-ProjDevelopmentCommandLayout `
    -Condition ($SetupContract.schema -ceq 'swawkit.toolchain-command/v1' -and
        $SetupContract.handler -ceq 'dev.setup') `
    -Message '.dev.setup Toolchain manifest is invalid'

$DependencyContracts = @(
    @{
        Name = '.dev.bun toolchain root'
        Script = '.dev\bun\_lib\runtime.ps1'
        Relative = '..\..\..\_toolchain'
        PathType = 'Container'
    },
    @{
        Name = '.dev.cargo runtime'
        Script = '.dev\cargo\run.ps1'
        Relative = '..\..\_toolchain\_modules\rust\runtime.ps1'
        PathType = 'Leaf'
    },
    @{
        Name = '.dev.cl runtime'
        Script = '.dev\cl\run.ps1'
        Relative = '..\..\_toolchain\_modules\msvc\runtime.ps1'
        PathType = 'Leaf'
    },
    @{
        Name = '.dev.rustc runtime'
        Script = '.dev\rustc\run.ps1'
        Relative = '..\..\_toolchain\_modules\rust\runtime.ps1'
        PathType = 'Leaf'
    },
    @{
        Name = '.dev.cmd shell runtime'
        Script = '.dev\cmd\run.ps1'
        Relative = '..\..\_shell\runtime.ps1'
        SourceMarker = '_shell\runtime.ps1'
        PathType = 'Leaf'
    },
    @{
        Name = '.dev.ps shell runtime'
        Script = '.dev\ps\run.ps1'
        Relative = '..\..\_shell\runtime.ps1'
        SourceMarker = '_shell\runtime.ps1'
        PathType = 'Leaf'
    },
    @{
        Name = '.dev.ps isolated runner'
        Script = '.dev\ps\run.ps1'
        Relative = '..\..\_shell\powershell-command.ps1'
        SourceMarker = '_shell\powershell-command.ps1'
        PathType = 'Leaf'
    }
)
foreach ($Contract in $DependencyContracts) {
    $ScriptPath = Join-Path $ProjRoot $Contract.Script
    $Source = [IO.File]::ReadAllText($ScriptPath)
    $TargetPath = [IO.Path]::GetFullPath((Join-Path `
        (Split-Path -Parent $ScriptPath) `
        $Contract.Relative
    ))
    $SourceMarker = if ($Contract.ContainsKey('SourceMarker')) {
        [string]$Contract.SourceMarker
    } else {
        [string]$Contract.Relative
    }

    Assert-ProjDevelopmentCommandLayout `
        -Condition ($Source.Contains($SourceMarker)) `
        -Message "$($Contract.Name) no longer declares its expected relative path"
    Assert-ProjDevelopmentCommandLayout `
        -Condition (Test-Path `
            -LiteralPath $TargetPath `
            -PathType $Contract.PathType) `
        -Message "$($Contract.Name) resolves to a missing target"
}

Write-Host '[PASS] Proj development command layout test' `
    -ForegroundColor Green
$global:LASTEXITCODE = 0
