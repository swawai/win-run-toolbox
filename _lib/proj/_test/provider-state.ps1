[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjProviderStateTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "Provider state test failed: $Message"
    }
}

function Get-ProjProviderTestFailure {
    param([Parameter(Mandatory = $true)][scriptblock]$Action)

    try {
        & $Action
    } catch {
        return $_.Exception.Message
    }
    return ''
}

function Get-ProjProviderTestPublication {
    param(
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)][string]$InputRevision
    )

    return Get-ProjRequiredCommandExport `
        -DataRoot $DataRoot `
        -ProviderAddress '.dev.setup' `
        -EntryCommand 'fixture' `
        -InputRevision $InputRevision `
        -ProducerContract (Get-ProjDevSetupProducerContract)
}

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\_lib\runtime.ps1')
. (Join-Path $ProjRoot '_toolchain\_lib\environment.ps1')
$TemporaryBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TemporaryBase)
$TemporaryRoot = Join-Path $TemporaryBase (
    "swawkit-provider-state-$([Guid]::NewGuid().ToString('N'))"
)

try {
    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    $DataRoot = Join-Path $TemporaryRoot 'data'
    $CacheRoot = Join-Path $TemporaryRoot 'cache'
    [void][IO.Directory]::CreateDirectory($ProjectRoot)
    [void][IO.Directory]::CreateDirectory($DataRoot)
    $InputA = 'sha256-' + ('a' * 64)
    $InputB = 'sha256-' + ('b' * 64)
    $ProfilePath = Join-Path $DataRoot '_profile.json'
    [IO.File]::WriteAllText($ProfilePath, '{"revision":1}')
    $ProfileRevisionA = 'sha256-' + (
        Get-ProjDevFileSha256 -Path $ProfilePath
    )
    $Context = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $DataRoot `
        -CacheDataRoot $CacheRoot `
        -EntryCommand 'fixture' `
        -EnvironmentInputRevision $InputA `
        -CommandProfileRevision $ProfileRevisionA

    $MissingMessage = Get-ProjProviderTestFailure {
        [void](Get-ProjProviderTestPublication `
            -DataRoot $DataRoot `
            -InputRevision $InputA)
    }
    Assert-ProjProviderStateTest `
        -Condition (
            $MissingMessage -like "*'.dev.setup'*" -and
            $MissingMessage -like "*'fixture .dev.setup'*" -and
            -not [IO.Directory]::Exists($Context.SetupCommandRoot)
        ) `
        -Message 'missing state did not fail closed without side effects'

    [void][IO.Directory]::CreateDirectory($Context.EnvironmentRoot)
    [IO.File]::WriteAllText(
        $Context.LegacyEnvironmentStatePath,
        '{"schema":"swawkit.proj-dev.environment-state.v2"}'
    )
    $LegacyMessage = Get-ProjProviderTestFailure {
        [void](Get-ProjProviderTestPublication `
            -DataRoot $DataRoot `
            -InputRevision $InputA)
    }
    Assert-ProjProviderStateTest `
        -Condition ($LegacyMessage -like "*'fixture .dev.setup'*") `
        -Message 'legacy export state incorrectly granted readiness'

    [IO.File]::WriteAllText($Context.ProviderStatePath, '{broken')
    $CorruptMessage = Get-ProjProviderTestFailure {
        [void](Get-ProjProviderTestPublication `
            -DataRoot $DataRoot `
            -InputRevision $InputA)
    }
    Assert-ProjProviderStateTest `
        -Condition ($CorruptMessage -like "*'fixture .dev.setup'*") `
        -Message 'corrupt state did not fail closed with repair advice'

    $Attempt = Start-ProjDevSetupProviderPublication -Context $Context
    $Unavailable = Read-ProjCommandProviderState `
        -Path $Context.ProviderStatePath `
        -DataRoot $DataRoot
    $UnavailableJson = [IO.File]::ReadAllText($Context.ProviderStatePath) |
        ConvertFrom-Json
    $UnavailableNames = [string[]]@(
        $UnavailableJson.PSObject.Properties.Name
    )
    Assert-ProjProviderStateTest `
        -Condition (
            [string]$Unavailable.Status -ceq 'unavailable' -and
            [string]$Unavailable.InputRevision -ceq $InputA -and
            [string]$Unavailable.Token -ceq [string]$Attempt.Token -and
            $UnavailableNames.Count -eq 4 -and
            $UnavailableNames -cnotcontains 'producerContract' -and
            $UnavailableNames -cnotcontains 'exportRevision'
        ) `
        -Message 'setup did not repair corrupt state as unavailable'
    $UnavailableMessage = Get-ProjProviderTestFailure {
        [void](Get-ProjProviderTestPublication `
            -DataRoot $DataRoot `
            -InputRevision $InputA)
    }
    Assert-ProjProviderStateTest `
        -Condition ($UnavailableMessage -like '*unavailable or outdated*') `
        -Message 'an unavailable state admitted a consumer'

    $Plan = New-ProjDevEnvironmentPlan
    $Scripts = ConvertTo-ProjDevEnvironmentScripts `
        -Plan $Plan `
        -PublicationToken ([string]$Attempt.Token)
    Assert-ProjProviderStateTest `
        -Condition (
            $Scripts.Cmd -like "*PUBLICATION_TOKEN=$($Attempt.Token)*" -and
            $Scripts.Ps1 -like "*PUBLICATION_TOKEN*'$($Attempt.Token)'*"
        ) `
        -Message 'generated env scripts did not embed the publication token'
    [void](Publish-ProjDevEnvironmentScripts `
        -Context $Context `
        -Scripts $Scripts)
    Complete-ProjDevSetupProviderPublication `
        -Context $Context `
        -Attempt $Attempt

    $ReadyJson = [IO.File]::ReadAllText($Context.ProviderStatePath) |
        ConvertFrom-Json
    $ReadyNames = [string[]]@($ReadyJson.PSObject.Properties.Name)
    Assert-ProjProviderStateTest `
        -Condition (
            $ReadyNames.Count -eq 5 -and
            $ReadyNames -ccontains 'producerContract' -and
            $ReadyNames -cnotcontains 'exportRevision' -and
            $ReadyNames -cnotcontains 'projectRoot' -and
            $ReadyNames -cnotcontains 'declarations'
        ) `
        -Message 'ready state does not match the minimal v1 schema'
    $Publication = Get-ProjProviderTestPublication `
        -DataRoot $DataRoot `
        -InputRevision $InputA
    Assert-ProjProviderStateTest `
        -Condition (
            [string]$Publication.Token -ceq [string]$Attempt.Token
        ) `
        -Message 'ready provider publication was not resolved'
    Write-ProjCommandProviderState `
        -Context $Context `
        -State (New-ProjCommandProviderReadyState `
            -InputRevision $InputA `
            -Token ([string]$Attempt.Token) `
            -ProducerContract 'swawkit.proj.dev-setup/v2')
    $ContractMessage = Get-ProjProviderTestFailure {
        [void](Get-ProjProviderTestPublication `
            -DataRoot $DataRoot `
            -InputRevision $InputA)
    }
    Assert-ProjProviderStateTest `
        -Condition ($ContractMessage -like '*unavailable or outdated*') `
        -Message 'a mismatched producer contract granted readiness'
    Write-ProjCommandProviderState `
        -Context $Context `
        -State (New-ProjCommandProviderReadyState `
            -InputRevision $InputA `
            -Token ([string]$Attempt.Token) `
            -ProducerContract (Get-ProjDevSetupProducerContract))
    [void](Import-ProjDevGeneratedEnvironment -Context $Context)
    $LeakedMetadata = @(
        [Environment]::GetEnvironmentVariables('Process').Keys |
            Where-Object {
                ([string]$_).StartsWith(
                    'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_',
                    [StringComparison]::OrdinalIgnoreCase
                )
            }
    )
    Assert-ProjProviderStateTest `
        -Condition ($LeakedMetadata.Count -eq 0) `
        -Message 'activation leaked provider metadata'

    [IO.File]::AppendAllText($Context.EnvCmdPath, 'rem no-hash probe')
    [IO.File]::AppendAllText($Context.EnvPs1Path, '# no-hash probe')
    [void](Import-ProjDevGeneratedEnvironment -Context $Context)
    $OriginalPs1 = [IO.File]::ReadAllText($Context.EnvPs1Path)
    [IO.File]::WriteAllText(
        $Context.EnvPs1Path,
        $OriginalPs1.Replace([string]$Attempt.Token, ('0' * 32)),
        [Text.UTF8Encoding]::new($true)
    )
    $TokenMessage = Get-ProjProviderTestFailure {
        [void](Import-ProjDevGeneratedEnvironment -Context $Context)
    }
    Assert-ProjProviderStateTest `
        -Condition ($TokenMessage -like '*does not match*provider state*') `
        -Message 'env.ps1 publication-token mismatch was accepted'
    [IO.File]::WriteAllText(
        $Context.EnvPs1Path,
        $OriginalPs1,
        [Text.UTF8Encoding]::new($true)
    )

    $TransitionToken = 'c' * 32
    $TransitionScript = @"
`$ChangedState = New-ProjCommandProviderUnavailableState -InputRevision '$InputA' -Token '$TransitionToken'
Write-ProjCommandProviderState -Context `$Context -State `$ChangedState
"@
    [IO.File]::AppendAllText(
        $Context.EnvPs1Path,
        "`r`n$TransitionScript"
    )
    $TransitionMessage = Get-ProjProviderTestFailure {
        [void](Import-ProjDevGeneratedEnvironment -Context $Context)
    }
    Assert-ProjProviderStateTest `
        -Condition ($TransitionMessage -like '*changed while it was being loaded*') `
        -Message 'provider transition during env.ps1 load was not detected'
    [IO.File]::WriteAllText(
        $Context.EnvPs1Path,
        $OriginalPs1,
        [Text.UTF8Encoding]::new($true)
    )
    Write-ProjCommandProviderState `
        -Context $Context `
        -State (New-ProjCommandProviderReadyState `
            -InputRevision $InputA `
            -Token ([string]$Attempt.Token) `
            -ProducerContract (Get-ProjDevSetupProducerContract))

    $StaleAttempt = Start-ProjDevSetupProviderPublication -Context $Context
    $NewToken = 'd' * 32
    Write-ProjCommandProviderState `
        -Context $Context `
        -State (New-ProjCommandProviderUnavailableState `
            -InputRevision $InputB `
            -Token $NewToken)
    $CasMessage = Get-ProjProviderTestFailure {
        Complete-ProjDevSetupProviderPublication `
            -Context $Context `
            -Attempt $StaleAttempt
    }
    $AfterCas = Read-ProjCommandProviderState `
        -Path $Context.ProviderStatePath `
        -DataRoot $DataRoot
    Assert-ProjProviderStateTest `
        -Condition (
            $CasMessage -like '*stale build was not published*' -and
            [string]$AfterCas.InputRevision -ceq $InputB -and
            [string]$AfterCas.Token -ceq $NewToken
        ) `
        -Message 'stale CAS overwrote newer provider state'

    [IO.File]::WriteAllText($ProfilePath, '{"revision":2}')
    $StaleStartMessage = Get-ProjProviderTestFailure {
        [void](Start-ProjDevSetupProviderPublication -Context $Context)
    }
    $AfterStaleStart = Read-ProjCommandProviderState `
        -Path $Context.ProviderStatePath `
        -DataRoot $DataRoot
    Assert-ProjProviderStateTest `
        -Condition (
            $StaleStartMessage -like '*Entry Profile changed*' -and
            [string]$AfterStaleStart.InputRevision -ceq $InputB -and
            [string]$AfterStaleStart.Token -ceq $NewToken
        ) `
        -Message 'stale command repaired current provider state'

    $ProfileRevisionB = 'sha256-' + (
        Get-ProjDevFileSha256 -Path $ProfilePath
    )
    $ContextB = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $DataRoot `
        -CacheDataRoot $CacheRoot `
        -EntryCommand 'fixture' `
        -EnvironmentInputRevision $InputB `
        -CommandProfileRevision $ProfileRevisionB
    $AttemptB = Start-ProjDevSetupProviderPublication -Context $ContextB
    $ScriptsB = ConvertTo-ProjDevEnvironmentScripts `
        -Plan (New-ProjDevEnvironmentPlan) `
        -PublicationToken ([string]$AttemptB.Token)
    [void](Publish-ProjDevEnvironmentScripts `
        -Context $ContextB `
        -Scripts $ScriptsB)
    Complete-ProjDevSetupProviderPublication `
        -Context $ContextB `
        -Attempt $AttemptB
    $OldInputMessage = Get-ProjProviderTestFailure {
        [void](Get-ProjProviderTestPublication `
            -DataRoot $DataRoot `
            -InputRevision $InputA)
    }
    Assert-ProjProviderStateTest `
        -Condition (
            $OldInputMessage -like '*unavailable or outdated*' -and
            $null -ne (Get-ProjProviderTestPublication `
                -DataRoot $DataRoot `
                -InputRevision $InputB)
        ) `
        -Message 'readiness was not bound to the Core input revision'

    $AtomicPath = Join-Path $DataRoot 'atomic-recovery.txt'
    [IO.File]::WriteAllText($AtomicPath, 'old')
    $AtomicHandle = [IO.File]::Open(
        $AtomicPath,
        [IO.FileMode]::Open,
        [IO.FileAccess]::Read,
        [IO.FileShare]::None
    )
    try {
        $AtomicMessage = Get-ProjProviderTestFailure {
            Write-ProjDevTextAtomic `
                -Path $AtomicPath `
                -Content 'new' `
                -ControlledRoot $DataRoot
        }
    } finally {
        $AtomicHandle.Dispose()
    }
    $RecoveryFiles = @(Get-ChildItem `
        -LiteralPath $DataRoot `
        -File `
        -Force | Where-Object {
            $_.Name -like '.atomic-recovery.txt.*.tmp'
        })
    Assert-ProjProviderStateTest `
        -Condition (
            $AtomicMessage -like '*Recovery files were preserved*' -and
            $RecoveryFiles.Count -eq 1 -and
            [IO.File]::ReadAllText($RecoveryFiles[0].FullName) -ceq 'new'
        ) `
        -Message 'failed atomic commit deleted its recovery temp file'

    Write-Host '[PASS] Proj provider state test' -ForegroundColor Green
} finally {
    Clear-ProjDevSetupExportMetadata
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        Remove-Item -LiteralPath $TemporaryRoot -Recurse -Force
    }
}

$global:LASTEXITCODE = 0
