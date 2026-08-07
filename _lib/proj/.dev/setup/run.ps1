$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if (@($args).Count -gt 0) {
    throw '.dev.setup does not accept dynamic arguments.'
}

. (Join-Path $PSScriptRoot '..\..\_toolchain\setup.ps1')

$Context = New-ProjDevContextFromEnvironment
$BunDefinition = Get-ProjDevBunDefinition
$PwshDefinition = Get-ProjDevPwshDefinition
$MsvcDefinition = Get-ProjDevMsvcDefinition
$RustDefinition = Get-ProjDevRustDefinition
if ($null -ne $RustDefinition -and $null -eq $MsvcDefinition) {
    throw (
        'Rust V0 with the x86_64-pc-windows-msvc host requires the managed ' +
        'MSVC module.'
    )
}
if ($null -ne $BunDefinition -or
    $null -ne $PwshDefinition -or
    $null -ne $MsvcDefinition -or
    $null -ne $RustDefinition) {
    Assert-ProjDevWindowsX64 -ToolName 'Managed development tools'
}
$ActiveGenerationId = [string]$env:SWAWKIT_PROJ_DEV_GENERATION_ID
$ActiveEnvironment = Assert-ProjDevActiveEnvironmentCompatible `
    -Context $Context

$Declarations = Get-ProjDevelopmentDeclarationSnapshot
$PendingModules = @(
    Get-ProjPendingDevelopmentSetupModuleNames -Declarations $Declarations
)
if (@($PendingModules).Count -gt 0) {
    throw (
        '.dev.setup does not yet handle these enabled declarations: ' +
        "$([string]::Join(', ', $PendingModules))."
    )
}

$SetupLock = Enter-ProjDevFileLock `
    -Path $Context.SetupLockPath `
    -ControlledRoot $Context.DataRoot
try {
    if ($null -ne $BunDefinition) {
        $BunDefinition = Resolve-ProjDevBunDefinitionForSetup `
            -Context $Context `
            -Definition $BunDefinition
    }
    if ($null -ne $PwshDefinition) {
        $PwshDefinition = Resolve-ProjDevPwshDefinitionForSetup `
            -Context $Context `
            -Definition $PwshDefinition
    }
    $Plan = New-ProjDevEnvironmentPlan -Context $Context
    $BunChanged = $false
    $PwshChanged = $false
    $MsvcChanged = $false
    $RustChanged = $false
    if ($null -ne $BunDefinition) {
        $BunChanged = Install-ProjDevBun `
            -Context $Context `
            -Definition $BunDefinition
        if ([string]$BunDefinition.SelectionStatus -ceq 'pending') {
            Write-ProjDevBunSelection `
                -Context $Context `
                -Definition $BunDefinition
        }
        Add-ProjDevBunEnvironment `
            -Context $Context `
            -Definition $BunDefinition `
            -Plan $Plan
    }
    if ($null -ne $PwshDefinition) {
        $PwshChanged = Install-ProjDevPwsh `
            -Context $Context `
            -Definition $PwshDefinition
        if ([string]$PwshDefinition.SelectionStatus -ceq 'pending') {
            Write-ProjDevPwshSelection `
                -Context $Context `
                -Definition $PwshDefinition
        }
        Add-ProjDevPwshEnvironment `
            -Context $Context `
            -Definition $PwshDefinition `
            -Plan $Plan
    }
    if ($null -ne $MsvcDefinition) {
        $MsvcChanged = Install-ProjDevMsvc `
            -Context $Context `
            -Definition $MsvcDefinition
        Add-ProjDevMsvcEnvironment `
            -Context $Context `
            -Definition $MsvcDefinition `
            -Plan $Plan
    }
    if ($null -ne $RustDefinition) {
        $RustChanged = Install-ProjDevRust `
            -Context $Context `
            -Definition $RustDefinition
        Add-ProjDevRustEnvironment `
            -Context $Context `
            -Definition $RustDefinition `
            -Plan $Plan
    }

    $Scripts = ConvertTo-ProjDevEnvironmentScripts -Plan $Plan
    $EnvironmentChanged = Publish-ProjDevEnvironmentScripts `
        -Context $Context `
        -Scripts $Scripts
    $StateChanged = Publish-ProjDevEnvironmentState `
        -Context $Context `
        -GenerationId ([string]$Scripts.GenerationId)
    $EnvironmentChanged = $EnvironmentChanged -or $StateChanged

    $BunVersionLabel = if ($null -ne $BunDefinition -and
        [string]$BunDefinition.RequestedVersion -ceq 'latest') {
        "latest -> $($BunDefinition.Version)"
    } elseif ($null -ne $BunDefinition) {
        [string]$BunDefinition.Version
    } else {
        ''
    }
    $PwshVersionLabel = if ($null -ne $PwshDefinition -and
        [string]$PwshDefinition.RequestedVersion -ceq 'latest') {
        "latest -> $($PwshDefinition.Version)"
    } elseif ($null -ne $PwshDefinition) {
        [string]$PwshDefinition.Version
    } else {
        ''
    }
    if ($null -ne $BunDefinition -and $BunChanged) {
        Write-Host "[OK] Bun $BunVersionLabel installed and configured." `
            -ForegroundColor Green
    } elseif ($null -ne $BunDefinition) {
        Write-Host "[OK] Bun $BunVersionLabel is ready." `
            -ForegroundColor Green
    }
    if ($null -ne $PwshDefinition -and $PwshChanged) {
        Write-Host (
            "[OK] PowerShell $PwshVersionLabel installed and configured."
        ) -ForegroundColor Green
    } elseif ($null -ne $PwshDefinition) {
        Write-Host "[OK] PowerShell $PwshVersionLabel is ready." `
            -ForegroundColor Green
    }
    if ($null -ne $MsvcDefinition -and $MsvcChanged) {
        Write-Host (
            "[OK] MSVC channel $($MsvcDefinition.Channel) installed and configured."
        ) -ForegroundColor Green
    } elseif ($null -ne $MsvcDefinition) {
        Write-Host (
            "[OK] MSVC channel $($MsvcDefinition.Channel) is ready."
        ) -ForegroundColor Green
    }
    if ($null -ne $RustDefinition -and $RustChanged) {
        Write-Host (
            "[OK] Rust $($RustDefinition.Toolchain) installed and configured."
        ) -ForegroundColor Green
    } elseif ($null -ne $RustDefinition) {
        Write-Host (
            "[OK] Rust $($RustDefinition.Toolchain) is ready."
        ) -ForegroundColor Green
    }
    if ($null -eq $BunDefinition -and
        $null -eq $PwshDefinition -and
        $null -eq $MsvcDefinition -and
        $null -eq $RustDefinition) {
        Write-Host '[OK] The base development environment is ready.' `
            -ForegroundColor Green
    }
    if ($null -ne $BunDefinition) {
        Write-ProjDevBunTrustWarning `
            -Context $Context `
            -Definition $BunDefinition
    }
    if ($null -ne $PwshDefinition) {
        Write-ProjDevPwshTrustWarning `
            -Context $Context `
            -Definition $PwshDefinition
    }
    if ($EnvironmentChanged) {
        Write-Host "[ENV] $($Context.EnvCmdPath)" -ForegroundColor DarkGray
        Write-Host "[ENV] $($Context.EnvPs1Path)" -ForegroundColor DarkGray
    }
    if ($ActiveEnvironment -and
        $ActiveGenerationId -cne [string]$Scripts.GenerationId) {
        Write-Warning (
            'The parent shell still has an older environment generation. ' +
            'Start a new project shell to use the published environment.'
        )
    }
} finally {
    $SetupLock.Dispose()
}

$global:LASTEXITCODE = 0
