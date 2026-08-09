Set-StrictMode -Version 2.0

$script:ProjDevelopmentEnvironmentRevisionPlaceholder =
    '__SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_EXPORT_REVISION__'
$script:ProjDevSetupExportRevisionVariable =
    'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_EXPORT_REVISION'

function Get-ProjDevelopmentEnvironmentRevisionPlaceholder {
    return $script:ProjDevelopmentEnvironmentRevisionPlaceholder
}

function Assert-ProjDevelopmentEnvironmentControlledRoot {
    param(
        [Parameter(Mandatory = $true)][string]$EnvironmentRoot
    )

    $FullPath = [IO.Path]::GetFullPath($EnvironmentRoot)
    $Item = Get-Item `
        -LiteralPath $FullPath `
        -Force `
        -ErrorAction SilentlyContinue
    if ($null -eq $Item) {
        return $FullPath
    }
    if (($Item.Attributes -band
        [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw (
            'The managed development environment root cannot be a reparse ' +
            "point: $FullPath"
        )
    }
    if (-not $Item.PSIsContainer) {
        throw (
            'The managed development environment root must be a directory: ' +
            $FullPath
        )
    }
    return $FullPath
}

function Get-ProjDevelopmentEnvironmentContentRevision {
    param(
        [Parameter(Mandatory = $true)][string]$CmdContent,
        [Parameter(Mandatory = $true)][string]$Ps1Content
    )

    $Sha = [Security.Cryptography.SHA256]::Create()
    try {
        $Bytes = [Text.Encoding]::UTF8.GetBytes(
            "$CmdContent`n---`n$Ps1Content"
        )
        return ([BitConverter]::ToString(
            $Sha.ComputeHash($Bytes)
        ).Replace('-', '').ToLowerInvariant()).Substring(0, 16)
    } finally {
        $Sha.Dispose()
    }
}
