Set-StrictMode -Version 2.0

function Get-ProjDevPwshSelectionPath {
    param([Parameter(Mandatory = $true)][object]$Context)

    return Join-Path $Context.EnvironmentRoot `
        'pwsh\.swawkit-dev-selection.json'
}

function Read-ProjDevPwshSelection {
    param([Parameter(Mandatory = $true)][object]$Context)

    $Path = Get-ProjDevPwshSelectionPath -Context $Context
    if (-not [IO.File]::Exists($Path)) {
        return $null
    }
    try {
        $Selection = Get-Content `
            -LiteralPath $Path `
            -Raw `
            -Encoding UTF8 | ConvertFrom-Json
    } catch {
        throw (
            'Cannot parse the PowerShell version selection: ' +
            $_.Exception.Message
        )
    }
    $Schema = [string](Get-ProjDevPwshReleaseProperty `
        -Value $Selection `
        -Name 'schema')
    $Selector = [string](Get-ProjDevPwshReleaseProperty `
        -Value $Selection `
        -Name 'selector')
    $Version = [string](Get-ProjDevPwshReleaseProperty `
        -Value $Selection `
        -Name 'version')
    $Sha256 = ([string](Get-ProjDevPwshReleaseProperty `
        -Value $Selection `
        -Name 'sourceSha256')).Trim().ToLowerInvariant()
    $Verification = [string](Get-ProjDevPwshReleaseProperty `
        -Value $Selection `
        -Name 'sourceVerification')
    if ($Schema -cne 'swawkit.proj-dev.pwsh-selection.v0' -or
        $Selector -cne 'latest' -or
        $Version -cnotmatch '^\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?$' -or
        $Sha256 -cnotmatch '^[a-f0-9]{64}$' -or
        $Verification -notin @('github', 'unverified')) {
        throw "The PowerShell version selection is invalid: $Path"
    }
    return [pscustomobject][ordered]@{
        Version = $Version
        Sha256 = $Sha256
        Verification = $Verification
    }
}

function Set-ProjDevPwshSelectionOnDefinition {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Selection
    )

    [void](Set-ProjDevPwshResolvedVersion `
        -Definition $Definition `
        -Version ([string]$Selection.Version))
    $Definition.Sha256 = [string]$Selection.Sha256
    $Definition.Verification = [string]$Selection.Verification
    $Definition.ReleaseResolved = $true
    $Definition.SelectionStatus = 'loaded'
    return $Definition
}

function Find-ProjDevPwshResolvedDefinition {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    if ([string]$Definition.RequestedVersion -cne 'latest') {
        return $Definition
    }
    $Selection = Read-ProjDevPwshSelection -Context $Context
    if ($null -eq $Selection) {
        return $null
    }
    return Set-ProjDevPwshSelectionOnDefinition `
        -Definition $Definition `
        -Selection $Selection
}

function Resolve-ProjDevPwshDefinitionForSetup {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [AllowNull()][object]$LatestRelease = $null
    )

    $Resolved = Find-ProjDevPwshResolvedDefinition `
        -Context $Context `
        -Definition $Definition
    if ($null -ne $Resolved) {
        return $Resolved
    }
    return Resolve-ProjDevPwshLatestRelease `
        -Definition $Definition `
        -Release $LatestRelease
}

function Write-ProjDevPwshSelection {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    if ([string]$Definition.RequestedVersion -cne 'latest' -or
        [string]$Definition.Version -ceq 'latest') {
        throw 'Only a resolved PowerShell latest definition can be selected.'
    }
    $Sha256 = Get-ProjDevExpectedSha256 -Definition $Definition
    if ([string]::IsNullOrWhiteSpace($Sha256)) {
        throw 'The resolved PowerShell latest archive SHA-256 was not recorded.'
    }
    $Selection = [ordered]@{
        schema = 'swawkit.proj-dev.pwsh-selection.v0'
        selector = 'latest'
        version = [string]$Definition.Version
        sourceSha256 = $Sha256
        sourceVerification = [string]$Definition.Verification
    }
    Write-ProjDevTextAtomic `
        -Path (Get-ProjDevPwshSelectionPath -Context $Context) `
        -Content (ConvertTo-ProjDevJsonText -Value $Selection) `
        -ControlledRoot $Context.DataRoot
    $Definition.SelectionStatus = 'loaded'
}
