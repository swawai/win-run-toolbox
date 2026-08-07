$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

[string[]]$PowerShellArguments = @($args)
if ($PowerShellArguments.Count -lt 2) {
    throw '.dev.ps requires -File <path.ps1> or -Command <script>.'
}

$KernelRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
. (Join-Path $KernelRoot '_toolchain\runtime.ps1')
. (Join-Path $KernelRoot '_shell\runtime.ps1')

$Context = New-ProjDevContextFromEnvironment
[void](Import-ProjDevOptionalGeneratedEnvironment -Context $Context)
[void](Initialize-ProjShellCommandEnvironment -KernelRoot $KernelRoot)

$Mode = [string]$PowerShellArguments[0]
[string[]]$InvocationArguments = @()
if ($Mode -ieq '-File') {
    $DeclaredPath = [string]$PowerShellArguments[1]
    if ([string]::IsNullOrWhiteSpace($DeclaredPath)) {
        throw '.dev.ps -File requires a non-empty script path.'
    }
    if ([IO.Path]::IsPathRooted($DeclaredPath)) {
        $Target = [IO.Path]::GetFullPath($DeclaredPath)
    } else {
        $Target = [IO.Path]::GetFullPath(
            (Join-Path $Context.ProjectRoot $DeclaredPath)
        )
    }
    if ([IO.Path]::GetExtension($Target) -ine '.ps1') {
        throw ".dev.ps -File requires a .ps1 script: $Target"
    }
    if (-not [IO.File]::Exists($Target)) {
        throw "PowerShell script does not exist: $Target"
    }
    if ($PowerShellArguments.Count -gt 2) {
        $InvocationArguments = [string[]]$PowerShellArguments[
            2..($PowerShellArguments.Count - 1)
        ]
    }
    $InternalMode = 'file'
} elseif ($Mode -ieq '-Command') {
    $Target = [string]::Join(
        ' ',
        [string[]]$PowerShellArguments[1..(
            $PowerShellArguments.Count - 1
        )]
    )
    if ([string]::IsNullOrWhiteSpace($Target)) {
        throw '.dev.ps -Command requires non-empty script text.'
    }
    $InternalMode = 'command'
} else {
    throw ".dev.ps accepts only -File or -Command; received: $Mode"
}

$ModeName = 'SWAWKIT_PROJ_INTERNAL_PS_EXEC_MODE'
$TargetName = 'SWAWKIT_PROJ_INTERNAL_PS_EXEC_TARGET'
$CountName = 'SWAWKIT_PROJ_INTERNAL_PS_EXEC_ARGC'
$ArgumentPrefix = 'SWAWKIT_PROJ_INTERNAL_PS_EXEC_ARG_'
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
[Environment]::SetEnvironmentVariable($ModeName, $InternalMode, 'Process')
[Environment]::SetEnvironmentVariable($TargetName, $Target, 'Process')
[Environment]::SetEnvironmentVariable(
    $CountName,
    $InvocationArguments.Count.ToString(
        [Globalization.CultureInfo]::InvariantCulture
    ),
    'Process'
)
for ($Index = 0; $Index -lt $InvocationArguments.Count; $Index++) {
    [Environment]::SetEnvironmentVariable(
        ($ArgumentPrefix + $Index),
        $InvocationArguments[$Index],
        'Process'
    )
}

$PowerShellPath = Get-ProjWindowsPowerShellPath
$RunnerPath = Join-Path $KernelRoot '_shell\powershell-command.ps1'
& $PowerShellPath `
    -NoLogo `
    -NoProfile `
    -NonInteractive `
    -ExecutionPolicy Bypass `
    -File $RunnerPath
exit ([int]$LASTEXITCODE)
