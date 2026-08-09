Set-StrictMode -Version 2.0

function Get-ProjDevMsvcEnvironmentLayout {
    param(
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [Parameter(Mandatory = $true)][string]$ToolVersion,
        [Parameter(Mandatory = $true)][string]$SdkVersion
    )

    $VcRoot = Join-Path $InstallRoot 'VC'
    $ToolRoot = Join-Path $VcRoot "Tools\MSVC\$ToolVersion"
    $SdkRoot = Join-Path $InstallRoot 'Windows Kits\10'
    $ToolBin = Join-Path $ToolRoot 'bin\Hostx64\x64'
    $SdkBin = Join-Path $SdkRoot "bin\$SdkVersion\x64"

    $Variables = [ordered]@{
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
    return [pscustomobject][ordered]@{
        Variables = $Variables
        PathPrefixes = [string[]]@(
            $ToolBin
            $SdkBin
            (Join-Path $SdkBin 'ucrt')
        )
    }
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
        -Definition $Definition `
        -InstallRoot $InstallRoot
    if ($null -eq $Metadata) {
        $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
        throw (
            'The managed MSVC installation is missing or inconsistent. Run ' +
            "'$Repair'."
        )
    }
    $Layout = Get-ProjDevMsvcEnvironmentLayout `
        -InstallRoot $InstallRoot `
        -ToolVersion ([string]$Metadata.toolVersion) `
        -SdkVersion ([string]$Metadata.sdkVersion)
    foreach ($Name in $Layout.Variables.Keys) {
        Set-ProjDevEnvironmentVariable `
            -Plan $Plan `
            -Name ([string]$Name) `
            -Value ([string]$Layout.Variables[$Name])
    }
    foreach ($Path in $Layout.PathPrefixes) {
        Add-ProjDevEnvironmentPath -Plan $Plan -Path $Path
    }
}

function Assert-ProjDevMsvcEnvironmentCurrent {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    $Repair = Get-ProjEnvironmentRepairInvocation -Context $Context
    $InstallRoot = Get-ProjDevMsvcInstallRoot `
        -Context $Context `
        -Definition $Definition
    $ToolVersion = [Environment]::GetEnvironmentVariable(
        'VCToolsVersion',
        [EnvironmentVariableTarget]::Process
    )
    $WindowsSdkVersion = [Environment]::GetEnvironmentVariable(
        'WindowsSDKVersion',
        [EnvironmentVariableTarget]::Process
    )
    $ToolVersionMatch = [regex]::Match(
        [string]$ToolVersion,
        '^\d+(?:\.\d+)+\z',
        [Text.RegularExpressions.RegexOptions]::CultureInvariant
    )
    $SdkMatch = [regex]::Match(
        [string]$WindowsSdkVersion,
        '^(?<Version>\d+(?:\.\d+)+)\\\z',
        [Text.RegularExpressions.RegexOptions]::CultureInvariant
    )
    if (-not $ToolVersionMatch.Success -or -not $SdkMatch.Success) {
        throw (
            'The generated MSVC environment has invalid version variables. ' +
            "Run '$Repair'."
        )
    }
    $ExpectedEnvironment = Get-ProjDevMsvcEnvironmentLayout `
        -InstallRoot $InstallRoot `
        -ToolVersion ([string]$ToolVersion) `
        -SdkVersion ([string]$SdkMatch.Groups['Version'].Value)
    foreach ($Name in $ExpectedEnvironment.Variables.Keys) {
        $Expected = [string]$ExpectedEnvironment.Variables[$Name]
        $Actual = [Environment]::GetEnvironmentVariable(
            [string]$Name,
            [EnvironmentVariableTarget]::Process
        )
        if ([string]$Actual -cne $Expected) {
            throw (
                "The generated MSVC environment has a stale $Name. Run " +
                "'$Repair'."
            )
        }
    }

    $PathEntries = @(([string]$env:PATH).Split(
        [IO.Path]::PathSeparator
    ) | Where-Object {
        -not [string]::IsNullOrWhiteSpace([string]$_)
    })
    $SearchIndex = 0
    foreach ($Prefix in $ExpectedEnvironment.PathPrefixes) {
        $ExpectedPath = Get-ProjDevCanonicalPath -Path ([string]$Prefix)
        $FoundIndex = -1
        for ($Index = $SearchIndex; $Index -lt $PathEntries.Count; $Index++) {
            try {
                $ActualPath = Get-ProjDevCanonicalPath `
                    -Path ([string]$PathEntries[$Index])
            } catch {
                continue
            }
            if ($ActualPath.Equals(
                $ExpectedPath,
                [StringComparison]::OrdinalIgnoreCase
            )) {
                $FoundIndex = $Index
                break
            }
        }
        if ($FoundIndex -lt 0) {
            throw (
                'The generated MSVC environment has an incomplete PATH. ' +
                "Run '$Repair'."
            )
        }
        $SearchIndex = $FoundIndex + 1
    }

    $ExecutableRoots = [ordered]@{
        'cl.exe' = [string]$ExpectedEnvironment.PathPrefixes[0]
        'link.exe' = [string]$ExpectedEnvironment.PathPrefixes[0]
        'rc.exe' = [string]$ExpectedEnvironment.PathPrefixes[1]
    }
    foreach ($ExecutableName in $ExecutableRoots.Keys) {
        $Command = Get-Command $ExecutableName `
            -CommandType Application `
            -ErrorAction SilentlyContinue |
            Select-Object -First 1
        $Expected = Join-Path $ExecutableRoots[$ExecutableName] $ExecutableName
        if ($null -eq $Command -or
            -not (Get-ProjDevCanonicalPath -Path $Command.Source).Equals(
                (Get-ProjDevCanonicalPath -Path $Expected),
                [StringComparison]::OrdinalIgnoreCase
            )) {
            throw (
                "The managed MSVC $ExecutableName is not selected. Run " +
                "'$Repair'."
            )
        }
    }
}
