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

$CommandEntries = [ordered]@{
    bun = '.dev\bun\run.ps1'
    cargo = '.dev\rust\cargo\run.ps1'
    cl = '.dev\msvc\cl\run.ps1'
    rustc = '.dev\rust\rustc\run.ps1'
    cmd = '.dev\cmd\run.ps1'
    exec = '.dev\exec\run.ps1'
    pwsh = '.dev\pwsh\run.ps1'
}
foreach ($Name in $CommandEntries.Keys) {
    $LegacyPath = Join-Path $ProjRoot ".$Name"
    $EntryPath = Join-Path $ProjRoot $CommandEntries[$Name]

    Assert-ProjDevelopmentCommandLayout `
        -Condition (-not (Test-Path -LiteralPath $LegacyPath)) `
        -Message "legacy command .$Name still exists"
    Assert-ProjDevelopmentCommandLayout `
        -Condition ([IO.File]::Exists($EntryPath)) `
        -Message "$Name does not have its declared PowerShell entry"
}
foreach ($OldAddress in @('cargo', 'cl', 'rustc')) {
    Assert-ProjDevelopmentCommandLayout `
        -Condition (-not (Test-Path -LiteralPath (Join-Path $ProjRoot ".dev\$OldAddress"))) `
        -Message "old flat .dev.$OldAddress command still exists"
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

Assert-ProjDevelopmentCommandLayout `
    -Condition (-not (Test-Path -LiteralPath (Join-Path $ProjRoot '.runtime'))) `
    -Message 'the removed .runtime Kernel command still exists'

$RuntimeContracts = @(
    @{ Path = '..runtime\run.core.json'; Handler = 'runtime.status' },
    @{ Path = '..runtime\host\exit\run.core.json'; Handler = 'host.exit' },
    @{ Path = '..runtime\host\restart\run.core.json'; Handler = 'host.restart' },
    @{ Path = '..runtime\cleanup\run.core.json'; Handler = 'runtime.cleanup' }
)
foreach ($RuntimeContract in $RuntimeContracts) {
    $RuntimeManifest = Join-Path $ProjRoot $RuntimeContract.Path
    Assert-ProjDevelopmentCommandLayout `
        -Condition ([IO.File]::Exists($RuntimeManifest)) `
        -Message "Runtime Control manifest is missing: $($RuntimeContract.Path)"
    $RuntimeDocument = Get-Content -LiteralPath $RuntimeManifest -Raw |
        ConvertFrom-Json
    Assert-ProjDevelopmentCommandLayout `
        -Condition ($RuntimeDocument.schema -ceq 'swawkit.core-command/v1' -and
            $RuntimeDocument.handler -ceq $RuntimeContract.Handler) `
        -Message "Runtime Control manifest is invalid: $($RuntimeContract.Path)"
}

$DependencyContracts = @(
    @{
        Name = '.dev.bun toolchain root'
        Script = '.dev\bun\_lib\runtime.ps1'
        Relative = '..\..\..\_toolchain'
        PathType = 'Container'
    },
    @{
        Name = '.dev.rust.cargo runtime'
        Script = '.dev\rust\cargo\run.ps1'
        Relative = '..\..\..\_toolchain\_modules\rust\runtime.ps1'
        PathType = 'Leaf'
    },
    @{
        Name = '.dev.msvc.cl runtime'
        Script = '.dev\msvc\cl\run.ps1'
        Relative = '..\..\..\_toolchain\_modules\msvc\runtime.ps1'
        PathType = 'Leaf'
    },
    @{
        Name = '.dev.rust.rustc runtime'
        Script = '.dev\rust\rustc\run.ps1'
        Relative = '..\..\..\_toolchain\_modules\rust\runtime.ps1'
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
        Name = '.dev.exec development environment runtime'
        Script = '.dev\exec\run.ps1'
        Relative = '..\..\_toolchain\runtime.ps1'
        SourceMarker = '_toolchain\runtime.ps1'
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

$PwshEntry = Join-Path $ProjRoot '.dev\pwsh\run.ps1'
$PwshSource = [IO.File]::ReadAllText($PwshEntry)
Assert-ProjDevelopmentCommandLayout `
    -Condition ($PwshSource.Contains('PSEdition') -and
        $PwshSource.Contains('PSVersionTable') -and
        -not [IO.File]::Exists((Join-Path $ProjRoot '.dev\ps\run.ps1'))) `
    -Message '.dev.pwsh does not own the PowerShell 7-only shell contract'

Write-Host '[PASS] Proj development command layout test' `
    -ForegroundColor Green
$global:LASTEXITCODE = 0
