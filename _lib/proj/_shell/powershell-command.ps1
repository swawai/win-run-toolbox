$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ModeName = 'SWAWKIT_PROJ_INTERNAL_PS_EXEC_MODE'
$TargetName = 'SWAWKIT_PROJ_INTERNAL_PS_EXEC_TARGET'
$CountName = 'SWAWKIT_PROJ_INTERNAL_PS_EXEC_ARGC'
$ArgumentPrefix = 'SWAWKIT_PROJ_INTERNAL_PS_EXEC_ARG_'

try {
    $Mode = [Environment]::GetEnvironmentVariable($ModeName, 'Process')
    $Target = [Environment]::GetEnvironmentVariable($TargetName, 'Process')
    $CountText = [Environment]::GetEnvironmentVariable($CountName, 'Process')
    $ArgumentCount = 0
    if (($Mode -cne 'file' -and $Mode -cne 'command') -or
        [string]::IsNullOrEmpty($Target) -or
        -not [int]::TryParse($CountText, [ref]$ArgumentCount) -or
        $ArgumentCount -lt 0 -or
        $ArgumentCount -gt 4096) {
        throw 'Invalid internal PowerShell command protocol.'
    }

    [string[]]$InvocationArguments = @()
    for ($Index = 0; $Index -lt $ArgumentCount; $Index++) {
        $Value = [Environment]::GetEnvironmentVariable(
            ($ArgumentPrefix + $Index),
            'Process'
        )
        if ($null -eq $Value) {
            $Value = ''
        }
        $InvocationArguments += [string]$Value
    }

    foreach ($Name in @($ModeName, $TargetName, $CountName)) {
        [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
    }
    $ProcessEnvironment = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    foreach ($Name in [string[]]@($ProcessEnvironment.Keys)) {
        if ($Name.StartsWith(
            $ArgumentPrefix,
            [StringComparison]::Ordinal
        )) {
            [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
        }
    }

    Set-StrictMode -Off
    $ErrorActionPreference = 'Continue'
    $global:LASTEXITCODE = 0
    if ($Mode -ceq 'file') {
        & $Target @InvocationArguments
    } else {
        $Command = [ScriptBlock]::Create($Target)
        & $Command
    }
    $Succeeded = $?
    $ExitCode = [int]$global:LASTEXITCODE
    if (-not $Succeeded -and $ExitCode -eq 0) {
        $ExitCode = 1
    }
    exit $ExitCode
} catch {
    [Console]::Error.WriteLine(
        ('PowerShell command failed: {0}' -f $_.Exception.Message)
    )
    exit 1
}
