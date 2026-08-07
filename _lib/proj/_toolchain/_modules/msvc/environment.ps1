Set-StrictMode -Version 2.0

function Get-ProjDevMsvcRuntimeSignature {
    param(
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Metadata
    )

    return Get-ProjDevSha256Text -Value (
        [string]::Join("`n", [string[]]@(
            (Get-ProjDevMsvcDefinitionSignature -Definition $Definition)
            [string]$Metadata.manifestSha256
            [string]$Metadata.toolVersion
            [string]$Metadata.sdkVersion
        ))
    )
}

function Add-ProjDevMsvcEnvironment {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition,
        [Parameter(Mandatory = $true)][object]$Plan
    )

    $InstallRoot = Get-ProjDevMsvcInstallRoot `
        -Context $Context `
        -Definition $Definition
    $Metadata = Get-ProjDevMsvcValidMetadata `
        -Context $Context `
        -Definition $Definition
    if ($null -eq $Metadata) {
        throw 'Cannot generate an environment from an invalid MSVC installation.'
    }
    $ToolVersion = [string]$Metadata.toolVersion
    $SdkVersion = [string]$Metadata.sdkVersion
    $VcRoot = Join-Path $InstallRoot 'VC'
    $ToolRoot = Join-Path $VcRoot "Tools\MSVC\$ToolVersion"
    $SdkRoot = Join-Path $InstallRoot 'Windows Kits\10'
    $ToolBin = Join-Path $ToolRoot 'bin\Hostx64\x64'
    $SdkBin = Join-Path $SdkRoot "bin\$SdkVersion\x64"

    $Variables = [ordered]@{
        SWAWKIT_PROJ_DEV_MSVC_MODE = [string]$Definition.Mode
        SWAWKIT_PROJ_DEV_MSVC_CHANNEL = [string]$Definition.Channel
        SWAWKIT_PROJ_DEV_MSVC_TOOL_VERSION = $ToolVersion
        SWAWKIT_PROJ_DEV_MSVC_SDK_VERSION = $SdkVersion
        SWAWKIT_PROJ_DEV_MSVC_HOME = $InstallRoot
        SWAWKIT_PROJ_DEV_MSVC_SIGNATURE = Get-ProjDevMsvcRuntimeSignature `
            -Definition $Definition `
            -Metadata $Metadata
        VSCMD_ARG_HOST_ARCH = 'x64'
        VSCMD_ARG_TGT_ARCH = 'x64'
        VCToolsVersion = $ToolVersion
        WindowsSDKVersion = "$SdkVersion\"
        VCToolsInstallDir = "$ToolRoot\"
        VCINSTALLDIR = "$VcRoot\"
        WindowsSdkDir = "$SdkRoot\"
        WindowsSdkBinPath = "$SdkRoot\bin\"
        WindowsSdkVerBinPath = "$SdkBin\"
        UniversalCRTSdkDir = "$SdkRoot\"
        UCRTVersion = $SdkVersion
        INCLUDE = [string]::Join(';', [string[]]@(
            (Join-Path $ToolRoot 'include')
            (Join-Path $SdkRoot "Include\$SdkVersion\ucrt")
            (Join-Path $SdkRoot "Include\$SdkVersion\shared")
            (Join-Path $SdkRoot "Include\$SdkVersion\um")
            (Join-Path $SdkRoot "Include\$SdkVersion\winrt")
            (Join-Path $SdkRoot "Include\$SdkVersion\cppwinrt")
        ))
        LIB = [string]::Join(';', [string[]]@(
            (Join-Path $ToolRoot 'lib\x64')
            (Join-Path $SdkRoot "Lib\$SdkVersion\ucrt\x64")
            (Join-Path $SdkRoot "Lib\$SdkVersion\um\x64")
        ))
    }
    foreach ($Name in $Variables.Keys) {
        Set-ProjDevEnvironmentVariable `
            -Plan $Plan `
            -Name ([string]$Name) `
            -Value ([string]$Variables[$Name])
    }
    foreach ($Path in @(
        $ToolBin,
        $SdkBin,
        (Join-Path $SdkBin 'ucrt')
    )) {
        Add-ProjDevEnvironmentPath -Plan $Plan -Path $Path
    }
}

function Assert-ProjDevMsvcReady {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    if ($null -eq (Get-ProjDevMsvcValidMetadata `
        -Context $Context `
        -Definition $Definition)) {
        throw (
            'The managed MSVC installation is missing or inconsistent. Run ' +
            "'$($Context.EntryCommand) .dev.setup'."
        )
    }
}

function Assert-ProjDevMsvcEnvironmentCurrent {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $InstallRoot = Get-ProjDevMsvcInstallRoot `
        -Context $Context `
        -Definition $Definition
    $Metadata = Get-ProjDevMsvcValidMetadata `
        -Context $Context `
        -Definition $Definition
    if ($null -eq $Metadata) {
        throw 'The generated MSVC environment has no valid installation.'
    }
    $ToolVersion = [string]$Metadata.toolVersion
    $SdkVersion = [string]$Metadata.sdkVersion
    $ExpectedSignature = Get-ProjDevMsvcRuntimeSignature `
        -Definition $Definition `
        -Metadata $Metadata
    if ([string]$env:SWAWKIT_PROJ_DEV_MSVC_MODE -cne
            [string]$Definition.Mode -or
        [string]$env:SWAWKIT_PROJ_DEV_MSVC_CHANNEL -cne
            [string]$Definition.Channel -or
        [string]$env:SWAWKIT_PROJ_DEV_MSVC_TOOL_VERSION -cne $ToolVersion -or
        [string]$env:SWAWKIT_PROJ_DEV_MSVC_SDK_VERSION -cne $SdkVersion -or
        [string]$env:SWAWKIT_PROJ_DEV_MSVC_SIGNATURE -cne $ExpectedSignature -or
        [string]$env:VCToolsVersion -cne $ToolVersion -or
        [string]$env:WindowsSDKVersion -cne "$SdkVersion\" -or
        [string]::IsNullOrWhiteSpace([string]$env:INCLUDE) -or
        [string]::IsNullOrWhiteSpace([string]$env:LIB) -or
        [string]::IsNullOrWhiteSpace([string]$env:SWAWKIT_PROJ_DEV_MSVC_HOME) -or
        -not (Get-ProjDevCanonicalPath -Path (
            [string]$env:SWAWKIT_PROJ_DEV_MSVC_HOME
        )).Equals(
            (Get-ProjDevCanonicalPath -Path $InstallRoot),
            [StringComparison]::OrdinalIgnoreCase
        )) {
        throw (
            'The generated MSVC environment does not match the project ' +
            "declaration. Run '$($Context.EntryCommand) .dev.setup'."
        )
    }

    $ToolBin = Join-Path $InstallRoot (
        "VC\Tools\MSVC\$ToolVersion\bin\Hostx64\x64"
    )
    foreach ($ExecutableName in @('cl.exe', 'link.exe')) {
        $Command = Get-Command $ExecutableName `
            -CommandType Application `
            -ErrorAction SilentlyContinue |
            Select-Object -First 1
        $Expected = Join-Path $ToolBin $ExecutableName
        if ($null -eq $Command -or
            -not (Get-ProjDevCanonicalPath -Path $Command.Source).Equals(
                (Get-ProjDevCanonicalPath -Path $Expected),
                [StringComparison]::OrdinalIgnoreCase
            )) {
            throw (
                "The managed MSVC $ExecutableName is not active. Exit this " +
                'shell and start a new project shell.'
            )
        }
    }
}
