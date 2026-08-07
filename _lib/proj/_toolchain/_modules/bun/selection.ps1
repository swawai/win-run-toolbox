Set-StrictMode -Version 2.0

function Get-ProjDevBunSelectionPath {
    param([Parameter(Mandatory = $true)][object]$Context)

    return Join-Path $Context.EnvironmentRoot `
        'bun\.swawkit-dev-selection.json'
}

function Read-ProjDevBunSelection {
    param([Parameter(Mandatory = $true)][object]$Context)

    $Path = Get-ProjDevBunSelectionPath -Context $Context
    if (-not [IO.File]::Exists($Path)) {
        return $null
    }
    try {
        $Selection = Get-Content `
            -LiteralPath $Path `
            -Raw `
            -Encoding UTF8 | ConvertFrom-Json
    } catch {
        throw "Cannot parse the Bun version selection: $($_.Exception.Message)"
    }
    $Schema = [string](Get-ProjDevBunReleaseProperty `
        -Value $Selection `
        -Name 'schema')
    $Selector = [string](Get-ProjDevBunReleaseProperty `
        -Value $Selection `
        -Name 'selector')
    $Version = [string](Get-ProjDevBunReleaseProperty `
        -Value $Selection `
        -Name 'version')
    $Sha256 = ([string](Get-ProjDevBunReleaseProperty `
        -Value $Selection `
        -Name 'sourceSha256')).Trim().ToLowerInvariant()
    $Verification = [string](Get-ProjDevBunReleaseProperty `
        -Value $Selection `
        -Name 'sourceVerification')
    if ($Schema -cne
            'swawkit.proj-dev.bun-selection.v0' -or
        $Selector -cne 'latest' -or
        $Version -cnotmatch
            '^\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?$' -or
        $Sha256 -cnotmatch '^[a-f0-9]{64}$' -or
        $Verification -notin @(
            'github',
            'unverified'
        )) {
        throw "The Bun version selection is invalid: $Path"
    }
    return [pscustomobject][ordered]@{
        Version = $Version
        Sha256 = $Sha256
        Verification = $Verification
    }
}

function Set-ProjDevBunSelectionOnDefinition {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Selection
    )

    [void](Set-ProjDevBunResolvedVersion `
        -Definition $Definition `
        -Version ([string]$Selection.Version))
    $Definition.Sha256 = [string]$Selection.Sha256
    $Definition.Verification = [string]$Selection.Verification
    $Definition.ReleaseResolved = $true
    $Definition.SelectionStatus = 'loaded'
    return $Definition
}

function Find-ProjDevBunResolvedDefinition {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    if ([string]$Definition.RequestedVersion -cne 'latest') {
        return $Definition
    }
    $Selection = Read-ProjDevBunSelection -Context $Context
    if ($null -eq $Selection) {
        return $null
    }
    return Set-ProjDevBunSelectionOnDefinition `
        -Definition $Definition `
        -Selection $Selection
}

function Get-ProjDevBunResolvedDefinition {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $Resolved = Find-ProjDevBunResolvedDefinition `
        -Context $Context `
        -Definition $Definition
    if ($null -eq $Resolved) {
        throw (
            'Bun latest has not been resolved for this project. Run ' +
            "'$($Context.EntryCommand) .dev.setup'."
        )
    }
    return $Resolved
}

function Resolve-ProjDevBunDefinitionForSetup {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [AllowNull()][object]$LatestRelease = $null
    )

    $Resolved = Find-ProjDevBunResolvedDefinition `
        -Context $Context `
        -Definition $Definition
    if ($null -ne $Resolved) {
        return $Resolved
    }
    return Resolve-ProjDevBunLatestRelease `
        -Definition $Definition `
        -Release $LatestRelease
}

function Write-ProjDevBunSelection {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    if ([string]$Definition.RequestedVersion -cne 'latest' -or
        [string]$Definition.Version -ceq 'latest') {
        throw 'Only a resolved Bun latest definition can be selected.'
    }
    $Sha256 = Get-ProjDevExpectedSha256 -Definition $Definition
    if ([string]::IsNullOrWhiteSpace($Sha256)) {
        throw 'The resolved Bun latest archive SHA-256 was not recorded.'
    }
    $Selection = [ordered]@{
        schema = 'swawkit.proj-dev.bun-selection.v0'
        selector = 'latest'
        version = [string]$Definition.Version
        sourceSha256 = $Sha256
        sourceVerification = [string]$Definition.Verification
    }
    Write-ProjDevTextAtomic `
        -Path (Get-ProjDevBunSelectionPath -Context $Context) `
        -Content (ConvertTo-ProjDevJsonText -Value $Selection) `
        -ControlledRoot $Context.DataRoot
    $Definition.SelectionStatus = 'loaded'
}
