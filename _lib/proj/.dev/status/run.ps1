$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if (@($args).Count -gt 0) {
    throw '.dev.status does not accept dynamic arguments.'
}

. (Join-Path $PSScriptRoot '..\..\_toolchain\setup.ps1')

$Context = New-ProjDevContextFromEnvironment
try {
    $GenerationId = Get-ProjDevelopmentEnvironmentGeneration `
        -EnvironmentRoot $Context.EnvironmentRoot `
        -EntryCommand $Context.EntryCommand
    if ($null -eq $GenerationId) {
        $Enabled = @(
            Get-ProjEnabledDevelopmentDeclarationNames `
                -Declarations (Get-ProjDevelopmentDeclarationSnapshot)
        )
        if ($Enabled.Count -gt 0) {
            Write-Host (
                '[OUTDATED] no environment has been published; run ' +
                "'$($Context.EntryCommand) .dev.setup'"
            ) -ForegroundColor Red
        }
    }
} catch {
    Write-Host "[OUTDATED] $($_.Exception.Message)" -ForegroundColor Red
}

$BunDefinition = Get-ProjDevBunDefinition
if ($null -eq $BunDefinition) {
    Write-Host '[OFF] bun is disabled.' -ForegroundColor DarkGray
} else {
    $DeclaredBunDefinition = $BunDefinition
    $BunDefinition = Find-ProjDevBunResolvedDefinition `
        -Context $Context `
        -Definition $BunDefinition
    if ($null -eq $BunDefinition) {
        Write-Host (
            '[MISSING] bun latest unresolved; run ' +
            "'$($Context.EntryCommand) .dev.setup'"
        ) -ForegroundColor Yellow
    } else {
        $Trust = Get-ProjDevBunTrustStatus `
            -Context $Context `
            -Definition $BunDefinition
        $Ready = $null -ne $Trust.Metadata -and
            (Test-ProjDevInstalled `
                -Context $Context `
                -Definition $BunDefinition)
        $State = if ($Ready) { 'ready' } else { 'missing' }
        $Color = if ($Ready) { 'Green' } else { 'Yellow' }
        $VersionLabel = if (
            [string]$DeclaredBunDefinition.RequestedVersion -ceq 'latest'
        ) {
            "latest -> $($BunDefinition.Version)"
        } else {
            [string]$BunDefinition.Version
        }
        Write-Host (
            "[{0}] bun {1}  {2}  {3}" -f
                $State.ToUpperInvariant(),
                $VersionLabel,
                $Trust.Level,
                $Trust.Message
        ) -ForegroundColor $Color
        Write-ProjDevBunTrustWarning `
            -Context $Context `
            -Definition $BunDefinition
    }
}

$PwshDefinition = Get-ProjDevPwshDefinition
if ($null -eq $PwshDefinition) {
    Write-Host '[OFF] pwsh is disabled.' -ForegroundColor DarkGray
} else {
    $DeclaredPwshDefinition = $PwshDefinition
    $PwshDefinition = Find-ProjDevPwshResolvedDefinition `
        -Context $Context `
        -Definition $PwshDefinition
    if ($null -eq $PwshDefinition) {
        Write-Host (
            '[MISSING] pwsh latest unresolved; run ' +
            "'$($Context.EntryCommand) .dev.setup'"
        ) -ForegroundColor Yellow
    } else {
        $Trust = Get-ProjDevPwshTrustStatus `
            -Context $Context `
            -Definition $PwshDefinition
        $Ready = $null -ne $Trust.Metadata -and
            (Test-ProjDevInstalled `
                -Context $Context `
                -Definition $PwshDefinition)
        $State = if ($Ready) { 'READY' } else { 'MISSING' }
        $Color = if ($Ready) { 'Green' } else { 'Yellow' }
        $VersionLabel = if (
            [string]$DeclaredPwshDefinition.RequestedVersion -ceq 'latest'
        ) {
            "latest -> $($PwshDefinition.Version)"
        } else {
            [string]$PwshDefinition.Version
        }
        Write-Host (
            "[$State] pwsh $VersionLabel  $($Trust.Level)  $($Trust.Message)"
        ) -ForegroundColor $Color
        Write-ProjDevPwshTrustWarning `
            -Context $Context `
            -Definition $PwshDefinition
    }
}

$MsvcDefinition = Get-ProjDevMsvcDefinition
if ($null -eq $MsvcDefinition) {
    Write-Host '[OFF] msvc is disabled.' -ForegroundColor DarkGray
} else {
    $MsvcMetadata = Get-ProjDevMsvcValidMetadata `
        -Context $Context `
        -Definition $MsvcDefinition
    $MsvcReady = $null -ne $MsvcMetadata -and
        (Test-ProjDevMsvcInstalled `
            -Context $Context `
            -Definition $MsvcDefinition)
    $MsvcState = if ($MsvcReady) { 'READY' } else { 'MISSING' }
    $MsvcColor = if ($MsvcReady) { 'Green' } else { 'Yellow' }
    $MsvcVersion = if ($MsvcReady) {
        "tool $($MsvcMetadata.toolVersion), SDK $($MsvcMetadata.sdkVersion)"
    } else {
        'not installed'
    }
    Write-Host (
        "[$MsvcState] msvc channel $($MsvcDefinition.Channel)  " +
        "microsoft-manifest  $MsvcVersion"
    ) -ForegroundColor $MsvcColor
}

$RustDefinition = Get-ProjDevRustDefinition
if ($null -eq $RustDefinition) {
    Write-Host '[OFF] rust is disabled.' -ForegroundColor DarkGray
} else {
    $RustMetadata = Get-ProjDevRustValidMetadata `
        -Context $Context `
        -Definition $RustDefinition
    $RustReady = $null -ne $RustMetadata -and
        (Test-ProjDevRustInstalled `
            -Context $Context `
            -Definition $RustDefinition)
    $RustState = if ($RustReady) { 'READY' } else { 'MISSING' }
    $RustColor = if ($RustReady) { 'Green' } else { 'Yellow' }
    $RustVersion = if ($RustReady) {
        "rustc $($RustMetadata.rustcVersion), cargo $($RustMetadata.cargoVersion)"
    } else {
        'not installed'
    }
    Write-Host (
        "[$RustState] rust $($RustDefinition.Toolchain)  " +
        "rust-static-sha256  $RustVersion"
    ) -ForegroundColor $RustColor
}

$global:LASTEXITCODE = 0
