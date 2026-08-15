$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

[string[]]$Invocation = @($args)
if ($Invocation.Count -lt 2) {
    throw '.dev.pwsh requires -File <path.ps1> or -Command <script>.'
}
if ($PSVersionTable.PSEdition -cne 'Core' -or
    $PSVersionTable.PSVersion.Major -lt 7) {
    throw '.dev.pwsh requires PowerShell 7 or newer.'
}

$Mode = [string]$Invocation[0]
if ($Mode -ieq '-File') {
    $DeclaredPath = [string]$Invocation[1]
    if ([string]::IsNullOrWhiteSpace($DeclaredPath)) {
        throw '.dev.pwsh -File requires a non-empty script path.'
    }
    $Target = if ([IO.Path]::IsPathRooted($DeclaredPath)) {
        [IO.Path]::GetFullPath($DeclaredPath)
    } else {
        [IO.Path]::GetFullPath(
            (Join-Path ([string]$env:SWAWKIT_PROJ_TARGET_PROJECT_ROOT) $DeclaredPath)
        )
    }
    if ([IO.Path]::GetExtension($Target) -ine '.ps1') {
        throw ".dev.pwsh -File requires a .ps1 script: $Target"
    }
    if (-not [IO.File]::Exists($Target)) {
        throw "PowerShell script does not exist: $Target"
    }
    Set-StrictMode -Off
    $ErrorActionPreference = 'Continue'
    $global:LASTEXITCODE = 0
    if ($Invocation.Count -gt 2) {
        [string[]]$Arguments = $Invocation[2..($Invocation.Count - 1)]
        & $Target @Arguments
    } else {
        & $Target
    }
} elseif ($Mode -ieq '-Command') {
    $CommandText = [string]::Join(
        ' ',
        [string[]]$Invocation[1..($Invocation.Count - 1)]
    )
    if ([string]::IsNullOrWhiteSpace($CommandText)) {
        throw '.dev.pwsh -Command requires non-empty script text.'
    }
    $Command = [ScriptBlock]::Create($CommandText)
    Set-StrictMode -Off
    $ErrorActionPreference = 'Continue'
    $global:LASTEXITCODE = 0
    & $Command
} else {
    throw ".dev.pwsh accepts only -File or -Command; received: $Mode"
}

$Succeeded = $?
$ExitCode = [int]$global:LASTEXITCODE
if (-not $Succeeded -and $ExitCode -eq 0) {
    $ExitCode = 1
}
exit $ExitCode
