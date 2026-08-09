[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjProviderActivationTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "Provider activation test failed: $Message"
    }
}

function Get-ProjProviderActivationFailure {
    param([Parameter(Mandatory = $true)][scriptblock]$Action)

    try {
        & $Action
    } catch {
        return $_.Exception.Message
    }
    return ''
}

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\_lib\runtime.ps1')
. (Join-Path $ProjRoot '_toolchain\_lib\environment.ps1')

$ProviderModeNames = [string[]]@(
    Get-ProjDevelopmentModuleDeclarationDescriptors |
        ForEach-Object { [string]$_.Mode }
)
$PreviousProviderModes = @{}
foreach ($Name in $ProviderModeNames) {
    $PreviousProviderModes[$Name] = [Environment]::GetEnvironmentVariable(
        $Name,
        [EnvironmentVariableTarget]::Process
    )
}
$InheritedMetadataName =
    'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_INHERITED_TEST'
$PreviousInheritedMetadata = [Environment]::GetEnvironmentVariable(
    $InheritedMetadataName,
    [EnvironmentVariableTarget]::Process
)

$TemporaryBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TemporaryBase)
$TemporaryRoot = Join-Path $TemporaryBase (
    "swawkit-provider-activation-$([Guid]::NewGuid().ToString('N'))"
)

try {
    foreach ($Name in $ProviderModeNames) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            'disabled',
            [EnvironmentVariableTarget]::Process
        )
    }

    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    $DataRoot = Join-Path $TemporaryRoot 'data'
    $CacheRoot = Join-Path $TemporaryRoot 'cache'
    [void][IO.Directory]::CreateDirectory($ProjectRoot)
    [void][IO.Directory]::CreateDirectory($DataRoot)
    $ProfilePath = Join-Path $DataRoot '_profile.json'
    $OriginalProfileText = '{"revision":1}'
    [IO.File]::WriteAllText($ProfilePath, $OriginalProfileText)
    $InputRevision = 'sha256-' + ('a' * 64)
    $ProfileRevision = 'sha256-' + (
        Get-ProjDevFileSha256 -Path $ProfilePath
    )
    $Context = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $DataRoot `
        -CacheDataRoot $CacheRoot `
        -EntryCommand 'fixture' `
        -EnvironmentInputRevision $InputRevision `
        -CommandProfileRevision $ProfileRevision

    $Attempt = Start-ProjDevSetupProviderPublication -Context $Context
    $Plan = New-ProjDevEnvironmentPlan
    Set-ProjDevEnvironmentVariable `
        -Plan $Plan `
        -Name (Get-ProjDevSetupPublicationTokenVariable) `
        -Value ([string]$Attempt.Token)
    $Scripts = ConvertTo-ProjDevEnvironmentScripts -Plan $Plan
    [void](Publish-ProjDevEnvironmentScripts `
        -Context $Context `
        -Scripts $Scripts)
    Complete-ProjDevSetupProviderPublication `
        -Context $Context `
        -Attempt $Attempt

    $ReplacementToken = 'e' * 32
    $HeldStateStream = Open-ProjDevReplaceableReadStream `
        -Path $Context.ProviderStatePath
    $HeldStateReader = $null
    try {
        Write-ProjCommandProviderState `
            -Context $Context `
            -State (New-ProjCommandProviderReadyState `
                -InputRevision $InputRevision `
                -Token $ReplacementToken `
                -ProducerContract (Get-ProjDevSetupProducerContract))
        $HeldStateReader = [IO.StreamReader]::new(
            $HeldStateStream,
            [Text.Encoding]::UTF8,
            $true
        )
        $HeldStateText = $HeldStateReader.ReadToEnd()
    } finally {
        if ($null -ne $HeldStateReader) {
            $HeldStateReader.Dispose()
        } else {
            $HeldStateStream.Dispose()
        }
    }
    $HeldState = $HeldStateText | ConvertFrom-Json
    $ReplacedState = Read-ProjCommandProviderState `
        -Path $Context.ProviderStatePath `
        -DataRoot $DataRoot
    Assert-ProjProviderActivationTest `
        -Condition (
            [string]$HeldState.token -ceq [string]$Attempt.Token -and
            [string]$ReplacedState.Token -ceq $ReplacementToken
        ) `
        -Message (
            'a state reader blocked atomic replacement or lost its ' +
            'opened-file snapshot'
        )
    Write-ProjCommandProviderState `
        -Context $Context `
        -State (New-ProjCommandProviderReadyState `
            -InputRevision $InputRevision `
            -Token ([string]$Attempt.Token) `
            -ProducerContract (Get-ProjDevSetupProducerContract))

    $NewProfileText = '{"revision":2}'
    $HeldProfileStream = Open-ProjDevReplaceableReadStream `
        -Path $ProfilePath
    $HeldProfileReader = $null
    try {
        Write-ProjDevTextAtomic `
            -Path $ProfilePath `
            -Content $NewProfileText `
            -ControlledRoot $DataRoot
        $CurrentProfileRevision = Get-ProjDevCurrentProfileRevision `
            -Context $Context
        $HeldProfileReader = [IO.StreamReader]::new(
            $HeldProfileStream,
            [Text.Encoding]::UTF8,
            $true
        )
        $HeldProfileText = $HeldProfileReader.ReadToEnd()
    } finally {
        if ($null -ne $HeldProfileReader) {
            $HeldProfileReader.Dispose()
        } else {
            $HeldProfileStream.Dispose()
        }
    }
    Assert-ProjProviderActivationTest `
        -Condition (
            $HeldProfileText -ceq $OriginalProfileText -and
            $CurrentProfileRevision -ceq (
                'sha256-' + (Get-ProjDevSha256Text -Value $NewProfileText)
            )
        ) `
        -Message (
            'Profile revision reading blocked atomic replacement or did not ' +
            'hash one opened-file snapshot'
        )

    [Environment]::SetEnvironmentVariable(
        $InheritedMetadataName,
        'stale',
        [EnvironmentVariableTarget]::Process
    )
    $OptionalImported = Import-ProjDevOptionalGeneratedEnvironment `
        -Context $Context
    Assert-ProjProviderActivationTest `
        -Condition (
            -not $OptionalImported -and
            $null -eq [Environment]::GetEnvironmentVariable(
                $InheritedMetadataName,
                [EnvironmentVariableTarget]::Process
            )
        ) `
        -Message 'optional activation retained inherited provider metadata'

    [Environment]::SetEnvironmentVariable(
        $InheritedMetadataName,
        'stale',
        [EnvironmentVariableTarget]::Process
    )
    $env:SWAWKIT_PROJ_GO_MODE = 'managed'
    $PendingMessage = Get-ProjProviderActivationFailure {
        [void](Import-ProjDevOptionalGeneratedEnvironment -Context $Context)
    }
    Assert-ProjProviderActivationTest `
        -Condition (
            $PendingMessage -ceq (
                '.dev.setup does not yet handle these enabled ' +
                'declarations: go.'
            ) -and
            $null -eq [Environment]::GetEnvironmentVariable(
                $InheritedMetadataName,
                [EnvironmentVariableTarget]::Process
            )
        ) `
        -Message (
            'a pending declaration reused a previously ready provider or ' +
            'retained inherited metadata'
        )

    Write-Host '[PASS] Proj provider activation test' -ForegroundColor Green
} finally {
    Clear-ProjDevSetupExportMetadata
    foreach ($Name in $ProviderModeNames) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            $PreviousProviderModes[$Name],
            [EnvironmentVariableTarget]::Process
        )
    }
    [Environment]::SetEnvironmentVariable(
        $InheritedMetadataName,
        $PreviousInheritedMetadata,
        [EnvironmentVariableTarget]::Process
    )
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        Remove-Item -LiteralPath $TemporaryRoot -Recurse -Force
    }
}

$global:LASTEXITCODE = 0
