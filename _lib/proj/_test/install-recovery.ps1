[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\setup.ps1')

function Assert-ProjRecoveryTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "Proj install recovery test failed: $Message"
    }
}

function Write-ProjRecoveryCandidate {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Identity
    )

    [void][IO.Directory]::CreateDirectory($Path)
    [IO.File]::WriteAllText(
        (Join-Path $Path 'identity.txt'),
        $Identity,
        [Text.UTF8Encoding]::new($false)
    )
}

$TestBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TestBase)
$TemporaryRoot = Join-Path $TestBase (
    "swawkit-proj-recovery-$([Guid]::NewGuid().ToString('N'))"
)
$LockHolder = [pscustomobject]@{ Stream = $null }
$BoundaryEnvironmentLink = ''
$BoundaryCacheLink = ''

try {
    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    $DataRoot = Join-Path $TemporaryRoot 'data'
    [void][IO.Directory]::CreateDirectory($ProjectRoot)
    [void][IO.Directory]::CreateDirectory($DataRoot)
    $Context = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $DataRoot `
        -CacheDataRoot (Join-Path $TemporaryRoot 'shared cache')
    $Definition = [pscustomobject]@{ Name = 'fixture' }
    $Target = Join-Path $Context.EnvironmentRoot 'fixture\installs\v1'
    $Parent = Split-Path -Path $Target -Parent
    [void][IO.Directory]::CreateDirectory($Parent)
    $Validate = {
        param($ValidationContext, $ValidationDefinition, $InstallRoot)

        $IdentityPath = Join-Path $InstallRoot 'identity.txt'
        return [IO.File]::Exists($IdentityPath) -and
            [IO.File]::ReadAllText($IdentityPath) -ceq 'valid'
    }

    Write-ProjRecoveryCandidate -Path $Target -Identity 'valid'
    $StaleBackup = "$Target.backup-stale"
    $StalePartial = Join-Path $Parent '.partial-stale'
    Write-ProjRecoveryCandidate -Path $StaleBackup -Identity 'invalid'
    Write-ProjRecoveryCandidate -Path $StalePartial -Identity 'partial'
    $Healthy = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target `
        -ValidateCandidate $Validate
    Assert-ProjRecoveryTest `
        -Condition (
            $Healthy.Ready -and
            -not (Test-ProjDevPathExists -Path $StaleBackup) -and
            -not (Test-ProjDevPathExists -Path $StalePartial)
        ) `
        -Message 'a healthy target did not clean interrupted residues'

    # Work/partial directories are disposable protocol artifacts. A declaration
    # change alters the target leaf, so recovery must still find strictly named
    # residues from an older version, MSVC channel, or Rust toolchain. Backups
    # remain leaf-scoped and must not be claimed by the new target.
    $CrossLeafResidues = @(
        (Join-Path $Parent (
            '.1.2.15.partial-11111111111111111111111111111111'
        )),
        (Join-Path $Parent (
            '.16.work-22222222222222222222222222222222'
        )),
        (Join-Path $Parent (
            '.nightly-2026-07-31.partial-33333333333333333333333333333333'
        ))
    )
    foreach ($Residue in $CrossLeafResidues) {
        Write-ProjRecoveryCandidate -Path $Residue -Identity 'orphan'
    }
    $ForeignBackup = Join-Path $Parent (
        'old.backup-20260801T0101010000000Z-' +
        '44444444444444444444444444444444'
    )
    Write-ProjRecoveryCandidate -Path $ForeignBackup -Identity 'valid'
    $CrossLeafCleanup = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target `
        -ValidateCandidate $Validate
    Assert-ProjRecoveryTest `
        -Condition ($CrossLeafCleanup.Ready -and
            @($CrossLeafResidues | Where-Object {
                Test-ProjDevPathExists -Path $_
            }).Count -eq 0 -and
            (Test-ProjDevPathExists -Path $ForeignBackup)) `
        -Message 'cross-leaf work cleanup removed a backup or missed an orphan'
    Remove-ProjDevControlledPathWithRetry `
        -Path $ForeignBackup `
        -DataRoot $Context.DataRoot `
        -Activity 'cleaning the foreign-backup boundary fixture'

    Remove-ProjDevControlledPathWithRetry `
        -Path $Target `
        -DataRoot $Context.DataRoot `
        -Activity 'preparing the missing-target recovery test'
    $MissingBackup = "$Target.backup-valid"
    Write-ProjRecoveryCandidate -Path $MissingBackup -Identity 'valid'
    $RestoredMissing = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target `
        -ValidateCandidate $Validate
    Assert-ProjRecoveryTest `
        -Condition (
            $RestoredMissing.Ready -and
            $RestoredMissing.Restored -and
            (& $Validate $Context $Definition $Target)
        ) `
        -Message 'a valid backup was not restored for a missing target'

    [IO.File]::WriteAllText((Join-Path $Target 'identity.txt'), 'invalid')
    $InvalidBackup = "$Target.backup-valid"
    Write-ProjRecoveryCandidate -Path $InvalidBackup -Identity 'valid'
    $RestoredInvalid = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target `
        -ValidateCandidate $Validate
    Assert-ProjRecoveryTest `
        -Condition (
            $RestoredInvalid.Ready -and
            $RestoredInvalid.Restored -and
            (& $Validate $Context $Definition $Target)
        ) `
        -Message 'a valid backup did not replace an invalid target'

    [IO.File]::WriteAllText((Join-Path $Target 'identity.txt'), 'invalid')
    $InterruptedWork = Join-Path $Parent '.work-interrupted'
    Write-ProjRecoveryCandidate -Path $InterruptedWork -Identity 'partial'
    $CleanState = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target `
        -ValidateCandidate $Validate
    Assert-ProjRecoveryTest `
        -Condition (
            -not $CleanState.Ready -and
            -not (Test-ProjDevPathExists -Path $Target) -and
            -not (Test-ProjDevPathExists -Path $InterruptedWork)
        ) `
        -Message 'invalid state was not reset for a clean reinstall'

    $OlderBackup = "$Target.backup-20260801T0101010000000Z-" +
        '11111111111111111111111111111111'
    $NewerBackup = "$Target.backup-20260801T0101020000000Z-" +
        '22222222222222222222222222222222'
    Write-ProjRecoveryCandidate -Path $OlderBackup -Identity 'valid'
    Write-ProjRecoveryCandidate -Path $NewerBackup -Identity 'valid'
    [IO.File]::WriteAllText((Join-Path $OlderBackup 'order.txt'), 'older')
    [IO.File]::WriteAllText((Join-Path $NewerBackup 'order.txt'), 'newer')
    $OrderedRecovery = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target `
        -ValidateCandidate $Validate
    Assert-ProjRecoveryTest `
        -Condition ($OrderedRecovery.Ready -and
            $OrderedRecovery.Restored -and
            [IO.File]::ReadAllText((Join-Path $Target 'order.txt')) -ceq
                'newer') `
        -Message 'recovery did not select the newest timestamped valid backup'

    Write-ProjRecoveryCandidate -Path $Target -Identity 'invalid'
    $LockedFile = Join-Path $Target 'locked.bin'
    [IO.File]::WriteAllText($LockedFile, 'locked')
    $LockedBackup = "$Target.backup-valid"
    Write-ProjRecoveryCandidate -Path $LockedBackup -Identity 'valid'
    $LockHolder.Stream = [IO.File]::Open(
        $LockedFile,
        [IO.FileMode]::Open,
        [IO.FileAccess]::Read,
        [IO.FileShare]::None
    )
    $LockRejected = $false
    try {
        [void](Repair-ProjDevInstallState `
            -Context $Context `
            -Definition $Definition `
            -TargetPath $Target `
            -ValidateCandidate $Validate)
    } catch {
        $LockRejected = $_.Exception.Message -like '*Release processes*'
    } finally {
        $LockHolder.Stream.Dispose()
        $LockHolder.Stream = $null
    }
    Assert-ProjRecoveryTest `
        -Condition (
            $LockRejected -and
            (Test-ProjDevPathExists -Path $LockedBackup)
        ) `
        -Message 'a file lock did not preserve the valid backup'
    $AfterUnlock = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target `
        -ValidateCandidate $Validate
    Assert-ProjRecoveryTest `
        -Condition ($AfterUnlock.Ready -and $AfterUnlock.Restored) `
        -Message 'recovery did not succeed after the file lock was released'

    $Staged = New-ProjDevInstallWorkPath `
        -TargetPath $Target `
        -Kind 'partial'
    Write-ProjRecoveryCandidate -Path $Staged -Identity 'new'
    $LockingValidator = {
        param($ValidationContext, $ValidationDefinition, $InstallRoot)

        $IdentityPath = Join-Path $InstallRoot 'identity.txt'
        $Identity = [IO.File]::ReadAllText($IdentityPath)
        if ($Identity -ceq 'new') {
            $LockHolder.Stream = [IO.File]::Open(
                $IdentityPath,
                [IO.FileMode]::Open,
                [IO.FileAccess]::Read,
                [IO.FileShare]::None
            )
            return $false
        }
        return $Identity -ceq 'valid'
    }.GetNewClosure()
    $RollbackPending = $false
    try {
        Publish-ProjDevInstallDirectory `
            -Context $Context `
            -Definition $Definition `
            -StagedPath $Staged `
            -TargetPath $Target `
            -ValidatePublished $LockingValidator
    } catch {
        $RollbackPending = $_.Exception.Message -like '*rollback is pending*'
    } finally {
        if ($null -ne $LockHolder.Stream) {
            $LockHolder.Stream.Dispose()
            $LockHolder.Stream = $null
        }
    }
    $PendingPaths = Get-ProjDevInstallRecoveryPaths -TargetPath $Target
    Assert-ProjRecoveryTest `
        -Condition (
            $RollbackPending -and
            @($PendingPaths.Backups).Count -eq 1
        ) `
        -Message 'failed publish did not preserve a recoverable backup'
    $RecoveredPublish = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target `
        -ValidateCandidate $Validate
    Assert-ProjRecoveryTest `
        -Condition ($RecoveredPublish.Ready -and $RecoveredPublish.Restored) `
        -Message 'a rollback-pending publish was not recovered'

    $MoveDestination = Join-Path $Parent 'move-destination'
    Write-ProjRecoveryCandidate -Path $MoveDestination -Identity 'existing'
    $MissingMoveSourceRejected = $false
    try {
        Move-ProjDevControlledPathWithRetry `
            -Source (Join-Path $Parent 'missing-move-source') `
            -Destination $MoveDestination `
            -DataRoot $Context.DataRoot `
            -Activity 'testing a missing move source'
    } catch {
        $MissingMoveSourceRejected =
            $_.Exception.Message -like '*source is missing*'
    }
    Assert-ProjRecoveryTest `
        -Condition $MissingMoveSourceRejected `
        -Message 'a missing move source was mistaken for an existing target'
    Remove-ProjDevControlledPathWithRetry `
        -Path $MoveDestination `
        -DataRoot $Context.DataRoot `
        -Activity 'cleaning the move contract test'

    Remove-ProjDevControlledPathWithRetry `
        -Path $Target `
        -DataRoot $Context.DataRoot `
        -Activity 'preparing the first-install rollback test'
    $FirstInstallStaged = New-ProjDevInstallWorkPath `
        -TargetPath $Target `
        -Kind 'partial'
    Write-ProjRecoveryCandidate -Path $FirstInstallStaged -Identity 'new'
    $FirstInstallMessageCorrect = $false
    try {
        Publish-ProjDevInstallDirectory `
            -Context $Context `
            -Definition $Definition `
            -StagedPath $FirstInstallStaged `
            -TargetPath $Target `
            -ValidatePublished $LockingValidator
    } catch {
        $FirstInstallMessageCorrect =
            $_.Exception.Message -like (
                '*No previous installation backup was available*'
            ) -and
            $_.Exception.Message -notlike '*valid backup*'
    } finally {
        if ($null -ne $LockHolder.Stream) {
            $LockHolder.Stream.Dispose()
            $LockHolder.Stream = $null
        }
    }
    $FirstInstallPaths = Get-ProjDevInstallRecoveryPaths -TargetPath $Target
    Assert-ProjRecoveryTest `
        -Condition (
            $FirstInstallMessageCorrect -and
            (Test-ProjDevPathExists -Path $Target) -and
            @($FirstInstallPaths.Backups).Count -eq 0
        ) `
        -Message 'first-install rollback reported a nonexistent backup'
    $RecoveredFirstInstall = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target `
        -ValidateCandidate $Validate
    Assert-ProjRecoveryTest `
        -Condition (
            -not $RecoveredFirstInstall.Ready -and
            -not $RecoveredFirstInstall.Restored -and
            -not (Test-ProjDevPathExists -Path $Target)
        ) `
        -Message 'first-install failure was not reset after its lock released'

    # A controlled root is not enough by itself: every existing segment down to
    # a destructive target must remain physical. Reject a module junction before
    # recovery can validate or remove anything below its external target.
    $BoundaryDataRoot = Join-Path $TemporaryRoot 'boundary-data'
    $BoundaryCacheRoot = Join-Path $TemporaryRoot 'boundary-cache'
    [void][IO.Directory]::CreateDirectory($BoundaryDataRoot)
    [void][IO.Directory]::CreateDirectory($BoundaryCacheRoot)
    $BoundaryContext = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $BoundaryDataRoot `
        -CacheDataRoot $BoundaryCacheRoot
    [void][IO.Directory]::CreateDirectory($BoundaryContext.EnvironmentRoot)
    $ExternalEnvironment = Join-Path $TemporaryRoot 'external-environment'
    [void][IO.Directory]::CreateDirectory(
        (Join-Path $ExternalEnvironment 'installs\v1')
    )
    $ExternalEnvironmentSentinel = Join-Path `
        $ExternalEnvironment `
        'installs\v1\sentinel.txt'
    [IO.File]::WriteAllText($ExternalEnvironmentSentinel, 'preserve')
    $BoundaryEnvironmentLink = Join-Path `
        $BoundaryContext.EnvironmentRoot `
        'fixture'
    [void](New-Item `
        -ItemType Junction `
        -Path $BoundaryEnvironmentLink `
        -Target $ExternalEnvironment)
    $DescendantJunctionRejected = $false
    try {
        [void](Repair-ProjDevInstallState `
            -Context $BoundaryContext `
            -Definition $Definition `
            -TargetPath (Join-Path `
                $BoundaryEnvironmentLink `
                'installs\v1') `
            -ValidateCandidate $Validate)
    } catch {
        $DescendantJunctionRejected =
            $_.Exception.Message -like '*reparse point*'
    }
    Assert-ProjRecoveryTest `
        -Condition ($DescendantJunctionRejected -and
            [IO.File]::Exists($ExternalEnvironmentSentinel)) `
        -Message 'install recovery crossed a descendant environment junction'
    [IO.Directory]::Delete($BoundaryEnvironmentLink)
    $BoundaryEnvironmentLink = ''

    # The shared cache is an independent destructive boundary. Context creation
    # must reject a junction root before a lock, download, or cleanup is created.
    $ExternalCache = Join-Path $TemporaryRoot 'external-cache'
    [void][IO.Directory]::CreateDirectory($ExternalCache)
    $ExternalCacheSentinel = Join-Path $ExternalCache 'sentinel.txt'
    [IO.File]::WriteAllText($ExternalCacheSentinel, 'preserve')
    $BoundaryCacheLink = Join-Path $TemporaryRoot 'linked-cache'
    [void](New-Item `
        -ItemType Junction `
        -Path $BoundaryCacheLink `
        -Target $ExternalCache)
    $CacheRootJunctionRejected = $false
    try {
        [void](New-ProjDevContext `
            -ProjectRoot $ProjectRoot `
            -DataRoot (Join-Path $TemporaryRoot 'cache-boundary-data') `
            -CacheDataRoot $BoundaryCacheLink)
    } catch {
        $CacheRootJunctionRejected =
            $_.Exception.Message -like '*cache data root*reparse point*'
    }
    Assert-ProjRecoveryTest `
        -Condition ($CacheRootJunctionRejected -and
            [IO.File]::Exists($ExternalCacheSentinel)) `
        -Message 'setup accepted a shared cache root junction'
    [IO.Directory]::Delete($BoundaryCacheLink)
    $BoundaryCacheLink = ''

    $PhysicalCacheRoot = Join-Path $TemporaryRoot 'physical-cache'
    [void][IO.Directory]::CreateDirectory($PhysicalCacheRoot)
    $BoundaryCacheLink = Join-Path $PhysicalCacheRoot 'downloads'
    [void](New-Item `
        -ItemType Junction `
        -Path $BoundaryCacheLink `
        -Target $ExternalCache)
    $CacheDescendantJunctionRejected = $false
    try {
        Remove-ProjDevControlledPath `
            -Path (Join-Path $BoundaryCacheLink 'sentinel.txt') `
            -DataRoot $PhysicalCacheRoot `
            -Activity 'testing shared cache containment'
    } catch {
        $CacheDescendantJunctionRejected =
            $_.Exception.Message -like '*reparse point*'
    }
    Assert-ProjRecoveryTest `
        -Condition ($CacheDescendantJunctionRejected -and
            [IO.File]::Exists($ExternalCacheSentinel)) `
        -Message 'cache cleanup crossed a descendant cache junction'
    [IO.Directory]::Delete($BoundaryCacheLink)
    $BoundaryCacheLink = ''

    Write-Host '[PASS] Proj install recovery test' -ForegroundColor Green
} finally {
    if ($null -ne $LockHolder.Stream) {
        $LockHolder.Stream.Dispose()
    }
    foreach ($Link in @($BoundaryEnvironmentLink, $BoundaryCacheLink)) {
        if (-not [string]::IsNullOrWhiteSpace($Link) -and
            [IO.Directory]::Exists($Link)) {
            [IO.Directory]::Delete($Link)
        }
    }
    $ResolvedTemporaryRoot = [IO.Path]::GetFullPath($TemporaryRoot)
    $TestPrefix = $TestBase.TrimEnd('\') + '\'
    if ($ResolvedTemporaryRoot.StartsWith(
        $TestPrefix,
        [StringComparison]::OrdinalIgnoreCase
    ) -and
        [IO.Path]::GetFileName($ResolvedTemporaryRoot).StartsWith(
            'swawkit-proj-recovery-',
            [StringComparison]::Ordinal
        ) -and
        [IO.Directory]::Exists($ResolvedTemporaryRoot)) {
        Remove-Item -LiteralPath $ResolvedTemporaryRoot -Recurse -Force
    }
}
