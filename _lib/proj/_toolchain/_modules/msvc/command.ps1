Set-StrictMode -Version 2.0

function Invoke-ProjDevMsvcCommand {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('cl.exe')]
        [string]$ExecutableName,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [AllowEmptyString()]
        [string[]]$Arguments
    )

    $Requirement = Import-ProjDevMsvcCommandEnvironment
    $Repair = Get-ProjEnvironmentRepairInvocation `
        -Context $Requirement.Context
    $Command = Get-Command $ExecutableName `
        -CommandType Application `
        -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $Command) {
        throw (
            "The managed MSVC $ExecutableName is unavailable. Run " +
            "'$Repair'."
        )
    }

    return Invoke-ProjDevConsoleProcess `
        -Executable ([string]$Command.Source) `
        -Arguments $Arguments `
        -WorkingDirectory $Requirement.Context.InvocationDirectory
}
