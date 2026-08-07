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
    $InstallRoot = Get-ProjDevMsvcInstallRoot `
        -Context $Requirement.Context `
        -Definition $Requirement.Definition
    $Metadata = Get-ProjDevMsvcValidMetadata `
        -Context $Requirement.Context `
        -Definition $Requirement.Definition `
        -InstallRoot $InstallRoot
    if ($null -eq $Metadata) {
        throw 'The managed MSVC command metadata is unavailable.'
    }

    $Executable = Resolve-ProjDevChildPath `
        -Root $InstallRoot `
        -RelativePath (
            "VC\Tools\MSVC\$([string]$Metadata.toolVersion)\" +
            "bin\Hostx64\x64\$ExecutableName"
        ) `
        -Description 'MSVC command executable'
    if (-not [IO.File]::Exists($Executable)) {
        throw "The managed MSVC command executable is missing: $Executable"
    }

    return Invoke-ProjDevConsoleProcess `
        -Executable $Executable `
        -Arguments $Arguments `
        -WorkingDirectory $Requirement.Context.InvocationDirectory
}
