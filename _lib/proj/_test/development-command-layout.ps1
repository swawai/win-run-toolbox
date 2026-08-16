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
$RepoRoot = [IO.Path]::GetFullPath((Join-Path $ProjRoot '..\..'))
Assert-ProjDevelopmentCommandLayout `
    -Condition (-not [IO.Directory]::Exists((Join-Path $ProjRoot '_global'))) `
    -Message 'the removed no-op global guard directory still exists'

$ModuleManifests = @(
    Get-ChildItem -LiteralPath $ProjRoot -Recurse -File -Filter '_module.json'
    Get-ChildItem -LiteralPath (Join-Path $RepoRoot '.swaw') `
        -Recurse -File -Filter '_module.json'
)
foreach ($ModuleManifest in $ModuleManifests) {
    $ModuleDocument = Get-Content -LiteralPath $ModuleManifest.FullName -Raw -Encoding UTF8 |
        ConvertFrom-Json
    Assert-ProjDevelopmentCommandLayout `
        -Condition ($ModuleDocument.schema -ceq 'swawkit.command-module/v4') `
        -Message "legacy module contract remains: $($ModuleManifest.FullName)"
}

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
$SetupContract = Get-Content -LiteralPath $SetupManifest -Raw -Encoding UTF8 |
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
    $RuntimeDocument = Get-Content -LiteralPath $RuntimeManifest -Raw -Encoding UTF8 |
        ConvertFrom-Json
    Assert-ProjDevelopmentCommandLayout `
        -Condition ($RuntimeDocument.schema -ceq 'swawkit.core-command/v1' -and
            $RuntimeDocument.handler -ceq $RuntimeContract.Handler) `
        -Message "Runtime Control manifest is invalid: $($RuntimeContract.Path)"
}

$ContextContracts = @(
    @{ Name = 'new'; Handler = 'context.new' },
    @{ Name = 'add'; Handler = 'context.add' },
    @{ Name = 'remove'; Handler = 'context.remove' },
    @{ Name = 'note'; Handler = 'context.note' },
    @{ Name = 'prompt'; Handler = 'context.prompt' },
    @{ Name = 'render'; Handler = 'context.render' },
    @{ Name = 'show'; Handler = 'context.show' },
    @{ Name = 'list'; Handler = 'context.list' },
    @{ Name = 'delete'; Handler = 'context.delete' }
)
foreach ($ContextContract in $ContextContracts) {
    $ContextManifest = Join-Path $ProjRoot (
        ".context\$($ContextContract.Name)\run.core.json"
    )
    Assert-ProjDevelopmentCommandLayout `
        -Condition ([IO.File]::Exists($ContextManifest)) `
        -Message "Context manifest is missing: $ContextManifest"
    $ContextDocument = Get-Content -LiteralPath $ContextManifest -Raw -Encoding UTF8 |
        ConvertFrom-Json
    Assert-ProjDevelopmentCommandLayout `
        -Condition ($ContextDocument.schema -ceq 'swawkit.core-command/v1' -and
            $ContextDocument.handler -ceq $ContextContract.Handler) `
        -Message "Context manifest is invalid: $ContextManifest"
}

$ContextModuleManifest = Join-Path $ProjRoot '.context\_module.json'
Assert-ProjDevelopmentCommandLayout `
    -Condition ([IO.File]::Exists($ContextModuleManifest)) `
    -Message '.context does not declare its module facets'
$ContextModule = Get-Content -LiteralPath $ContextModuleManifest -Raw -Encoding UTF8 |
    ConvertFrom-Json
$ContextFacet = @($ContextModule.facets)[0]
$ContextSubjectKind = @($ContextModule.subjectKinds)[0]
$ContextOverviewFacet = @($ContextSubjectKind.facets) |
    Where-Object { $_.id -ceq 'overview' } |
    Select-Object -First 1
Assert-ProjDevelopmentCommandLayout `
    -Condition ($ContextModule.schema -ceq 'swawkit.command-module/v4' -and
        @($ContextModule.facets).Count -eq 1 -and
        $ContextFacet.id -ceq 'contexts' -and
        $ContextFacet.kind -ceq 'collection' -and
        $ContextFacet.subjectKind.kind -ceq 'context' -and
        $ContextFacet.subjectKind.provider.type -ceq 'command' -and
        $ContextFacet.subjectKind.provider.source -ceq 'kernel' -and
        $ContextFacet.subjectKind.provider.address -ceq '.context' -and
        $ContextFacet.resolver.type -ceq 'command' -and
        $ContextFacet.resolver.address -ceq '.context.list' -and
        @($ContextFacet.resolver.arguments).Count -eq 1 -and
        $ContextFacet.resolver.arguments[0] -ceq '--json' -and
        $ContextFacet.resolver.returns -ceq 'swawkit.subject-collection/v2' -and
        $ContextSubjectKind.kind -ceq 'context' -and
        @($ContextSubjectKind.facets).Count -eq 7 -and
        $ContextOverviewFacet.resolver.address -ceq '.context.show' -and
        $ContextOverviewFacet.resolver.arguments[0].bind -ceq 'subject.id') `
    -Message '.context collection facet declaration is invalid'
Assert-ProjDevelopmentCommandLayout `
    -Condition (-not (Test-Path -LiteralPath (
        Join-Path $ProjRoot '.context\resource.core.json'
    ))) `
    -Message 'the removed Context resource provider manifest still exists'

$RunsModuleManifest = Join-Path $ProjRoot '.runs\_module.json'
Assert-ProjDevelopmentCommandLayout `
    -Condition ([IO.File]::Exists($RunsModuleManifest)) `
    -Message '.runs does not declare its Run Subject facets'
$RunsModule = Get-Content -LiteralPath $RunsModuleManifest -Raw -Encoding UTF8 |
    ConvertFrom-Json
$AllRunsFacet = @($RunsModule.facets)[0]
$RunSubjectKind = @($RunsModule.subjectKinds)[0]
$RunOverviewFacet = @($RunSubjectKind.facets) |
    Where-Object { $_.id -ceq 'overview' } |
    Select-Object -First 1
$RunOpenFacet = @($RunSubjectKind.facets) |
    Where-Object { $_.id -ceq 'open' } |
    Select-Object -First 1
$RunsContractChecks = @(
    ($RunsModule.schema -ceq 'swawkit.command-module/v4')
    (@($RunsModule.facets).Count -eq 1)
    ($AllRunsFacet.id -ceq 'all')
    ($AllRunsFacet.kind -ceq 'collection')
    (-not [string]::IsNullOrWhiteSpace($AllRunsFacet.label.'zh-CN'))
    ($AllRunsFacet.label.en -ceq 'All Runs')
    ($AllRunsFacet.subjectKind.kind -ceq 'run')
    ($AllRunsFacet.subjectKind.provider.type -ceq 'command')
    ($AllRunsFacet.subjectKind.provider.source -ceq 'kernel')
    ($AllRunsFacet.subjectKind.provider.address -ceq '.runs')
    ($AllRunsFacet.resolver.address -ceq '.runs')
    (@($AllRunsFacet.resolver.arguments).Count -eq 1)
    ($AllRunsFacet.resolver.arguments[0] -ceq '--json')
    ($AllRunsFacet.resolver.returns -ceq 'swawkit.subject-collection/v2')
    ($RunSubjectKind.kind -ceq 'run')
    (@($RunSubjectKind.facets).Count -eq 2)
    ($RunOverviewFacet.resolver.address -ceq '.runs')
    ($RunOverviewFacet.resolver.arguments[0] -ceq '--run')
    ($RunOverviewFacet.resolver.arguments[1].bind -ceq 'subject.id')
    ($RunOverviewFacet.resolver.returns -ceq 'swawkit.command-run-journal/v1')
    ($RunOpenFacet.resolver.address -ceq '.runs')
    ($RunOpenFacet.resolver.arguments[0] -ceq '--open')
    ($RunOpenFacet.resolver.arguments[1].bind -ceq 'subject.id')
)
Assert-ProjDevelopmentCommandLayout `
    -Condition ($RunsContractChecks -notcontains $false) `
    -Message '.runs Run collection facet declaration is invalid'

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
