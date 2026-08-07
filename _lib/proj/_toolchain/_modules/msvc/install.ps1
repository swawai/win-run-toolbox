Set-StrictMode -Version 2.0

function Get-ProjDevMsvcSdkPayload {
    param(
        [Parameter(Mandatory = $true)][object]$Recipe,
        [Parameter(Mandatory = $true)][string]$LeafName
    )

    $Matches = @($Recipe.SdkPayloads | Where-Object {
        [string]$_.LeafName -ieq $LeafName
    })
    if ($Matches.Count -ne 1) {
        throw (
            "Windows SDK manifest must contain one '$LeafName' payload; " +
            "found $($Matches.Count)."
        )
    }
    return $Matches[0]
}

function Get-ProjDevMsvcAssemblyVersions {
    param([Parameter(Mandatory = $true)][string]$InstallRoot)

    $ToolBase = Join-Path $InstallRoot 'VC\Tools\MSVC'
    $SdkBase = Join-Path $InstallRoot 'Windows Kits\10\bin'
    $ToolDirs = @(if ([IO.Directory]::Exists($ToolBase)) {
        Get-ChildItem -LiteralPath $ToolBase -Directory | Where-Object {
            $_.Name -match '^\d+(\.\d+)+$'
        } | Sort-Object { [version]$_.Name } -Descending
    })
    $SdkDirs = @(if ([IO.Directory]::Exists($SdkBase)) {
        Get-ChildItem -LiteralPath $SdkBase -Directory | Where-Object {
            $_.Name -match '^\d+(\.\d+)+$'
        } | Sort-Object { [version]$_.Name } -Descending
    })
    if ($ToolDirs.Count -ne 1) {
        throw "Expected one extracted x64 MSVC tool version; found $($ToolDirs.Count)."
    }
    if ($SdkDirs.Count -ne 1) {
        throw "Expected one extracted Windows SDK version; found $($SdkDirs.Count)."
    }
    return [pscustomobject][ordered]@{
        ToolVersion = [string]$ToolDirs[0].Name
        SdkVersion = [string]$SdkDirs[0].Name
    }
}

function Remove-ProjDevMsvcOptionalPath {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [Parameter(Mandatory = $true)][string]$RelativePath
    )

    $Path = Resolve-ProjDevChildPath `
        -Root $InstallRoot `
        -RelativePath $RelativePath `
        -Description 'MSVC cleanup path'
    if ([IO.Directory]::Exists($Path) -or [IO.File]::Exists($Path)) {
        Remove-ProjDevControlledPath `
            -Path $Path `
            -DataRoot $Context.DataRoot `
            -Activity 'cleaning an unused MSVC component'
    }
}

function Complete-ProjDevMsvcAssembly {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][string]$InstallRoot
    )

    $Versions = Get-ProjDevMsvcAssemblyVersions -InstallRoot $InstallRoot
    $ToolVersion = [string]$Versions.ToolVersion
    $SdkVersion = [string]$Versions.SdkVersion
    $DiaSource = Join-Path $InstallRoot 'DIA SDK\bin\amd64\msdia140.dll'
    $ToolBin = Join-Path $InstallRoot (
        "VC\Tools\MSVC\$ToolVersion\bin\Hostx64\x64"
    )
    if (-not [IO.File]::Exists($DiaSource)) {
        throw 'The extracted MSVC payload is missing the x64 DIA runtime.'
    }
    [void][IO.Directory]::CreateDirectory($ToolBin)
    [IO.File]::Copy(
        $DiaSource,
        (Join-Path $ToolBin 'msdia140.dll'),
        $true
    )

    foreach ($RelativePath in @(
        "VC\Tools\MSVC\$ToolVersion\bin\Hostx64\x64\vctip.exe"
        'Common7'
        'Catalogs'
        'DesignTime'
        'Windows Kits\10\Catalogs'
        'Windows Kits\10\DesignTime'
        "VC\Tools\MSVC\$ToolVersion\bin\Hostx86"
        "VC\Tools\MSVC\$ToolVersion\bin\Hostarm"
        "VC\Tools\MSVC\$ToolVersion\bin\Hostarm64"
    )) {
        Remove-ProjDevMsvcOptionalPath `
            -Context $Context `
            -InstallRoot $InstallRoot `
            -RelativePath $RelativePath
    }
    foreach ($Architecture in @('x86', 'arm', 'arm64')) {
        foreach ($RelativePath in @(
            "Windows Kits\10\bin\$SdkVersion\$Architecture"
            "Windows Kits\10\Lib\$SdkVersion\ucrt\$Architecture"
            "Windows Kits\10\Lib\$SdkVersion\um\$Architecture"
        )) {
            Remove-ProjDevMsvcOptionalPath `
                -Context $Context `
                -InstallRoot $InstallRoot `
                -RelativePath $RelativePath
        }
    }

    $BuildRoot = Join-Path $InstallRoot 'VC\Auxiliary\Build'
    [void][IO.Directory]::CreateDirectory($BuildRoot)
    [IO.File]::WriteAllText(
        (Join-Path $BuildRoot 'vcvarsall.bat'),
        "@echo off`r`nrem Compatibility marker for tools such as nvcc.`r`n",
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::WriteAllText(
        (Join-Path $BuildRoot 'vcvars64.bat'),
        "@echo off`r`ncall `"%~dp0..\..\..\setup_x64.bat`"`r`n",
        [Text.UTF8Encoding]::new($false)
    )
    $Setup = @(
        '@echo off'
        'set "VSCMD_ARG_HOST_ARCH=x64"'
        'set "VSCMD_ARG_TGT_ARCH=x64"'
        "set `"VCToolsVersion=$ToolVersion`""
        "set `"WindowsSDKVersion=$SdkVersion\`""
        "set `"VCToolsInstallDir=%~dp0VC\Tools\MSVC\$ToolVersion\`""
        'set "VCINSTALLDIR=%~dp0VC\"'
        'set "WindowsSdkDir=%~dp0Windows Kits\10\"'
        'set "WindowsSdkBinPath=%~dp0Windows Kits\10\bin\"'
        "set `"WindowsSdkVerBinPath=%~dp0Windows Kits\10\bin\$SdkVersion\x64\`""
        'set "UniversalCRTSdkDir=%~dp0Windows Kits\10\"'
        "set `"UCRTVersion=$SdkVersion`""
        (
            "set `"PATH=%~dp0VC\Tools\MSVC\$ToolVersion\bin\Hostx64\x64;" +
            "%~dp0Windows Kits\10\bin\$SdkVersion\x64;" +
            "%~dp0Windows Kits\10\bin\$SdkVersion\x64\ucrt;%PATH%`""
        )
        (
            "set `"INCLUDE=%~dp0VC\Tools\MSVC\$ToolVersion\include;" +
            "%~dp0Windows Kits\10\Include\$SdkVersion\ucrt;" +
            "%~dp0Windows Kits\10\Include\$SdkVersion\shared;" +
            "%~dp0Windows Kits\10\Include\$SdkVersion\um;" +
            "%~dp0Windows Kits\10\Include\$SdkVersion\winrt;" +
            "%~dp0Windows Kits\10\Include\$SdkVersion\cppwinrt`""
        )
        (
            "set `"LIB=%~dp0VC\Tools\MSVC\$ToolVersion\lib\x64;" +
            "%~dp0Windows Kits\10\Lib\$SdkVersion\ucrt\x64;" +
            "%~dp0Windows Kits\10\Lib\$SdkVersion\um\x64`""
        )
    )
    [IO.File]::WriteAllText(
        (Join-Path $InstallRoot 'setup_x64.bat'),
        ([string]::Join("`r`n", [string[]]$Setup) + "`r`n"),
        [Text.UTF8Encoding]::new($false)
    )
    return $Versions
}

function Install-ProjDevMsvc {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][object]$Definition
    )

    Assert-ProjDevWindowsX64 -ToolName 'MSVC'
    $Target = Get-ProjDevMsvcInstallRoot `
        -Context $Context `
        -Definition $Definition
    $ValidateInstalled = {
        param($ValidationContext, $ValidationDefinition, $InstallRoot)

        return Test-ProjDevMsvcInstalled `
            -Context $ValidationContext `
            -Definition $ValidationDefinition `
            -InstallRoot $InstallRoot
    }
    $Recovery = Repair-ProjDevInstallState `
        -Context $Context `
        -Definition $Definition `
        -TargetPath $Target `
        -ValidateCandidate $ValidateInstalled
    if ($Recovery.Ready) {
        return $false
    }

    Write-Host (
        "[STEP] Resolving MSVC channel $($Definition.Channel)..."
    ) -ForegroundColor Cyan
    $Recipe = Resolve-ProjDevMsvcRelease `
        -Context $Context `
        -Definition $Definition
    Write-Host (
        "[INFO] MSVC package $($Recipe.ToolPackageVersion), " +
        "SDK $($Recipe.SdkPackageId)"
    ) -ForegroundColor DarkGray

    $Parent = Split-Path -Path $Target -Parent
    [void][IO.Directory]::CreateDirectory($Parent)
    $StagedRoot = New-ProjDevInstallWorkPath `
        -TargetPath $Target `
        -Kind 'partial'
    $MsiSourceRoot = New-ProjDevInstallWorkPath `
        -TargetPath $Target `
        -Kind 'work'
    [void][IO.Directory]::CreateDirectory($StagedRoot)
    try {
        foreach ($Payload in [object[]]$Recipe.ToolPayloads) {
            $Path = Get-ProjDevMsvcVerifiedPayload `
                -Context $Context `
                -Definition $Definition `
                -Payload $Payload
            Write-Host "[EXT] $($Payload.LeafName)" -ForegroundColor DarkGray
            Expand-ProjDevMsvcVsix `
                -ArchivePath $Path `
                -Destination $StagedRoot `
                -ControlledRoot $Context.DataRoot
        }

        $MsiPaths = [Collections.Generic.List[string]]::new()
        $CabNames = [Collections.Generic.HashSet[string]]::new(
            [StringComparer]::OrdinalIgnoreCase
        )
        $CabCandidates = [string[]]@($Recipe.SdkPayloads |
            Where-Object {
                [IO.Path]::GetExtension([string]$_.LeafName) -ieq '.cab'
            } |
            ForEach-Object { [string]$_.LeafName })
        foreach ($Payload in [object[]]$Recipe.MsiPayloads) {
            $Path = Get-ProjDevMsvcVerifiedPayload `
                -Context $Context `
                -Definition $Definition `
                -Payload $Payload
            $MsiPath = Copy-ProjDevMsvcPayloadToSourceRoot `
                -Context $Context `
                -Payload $Payload `
                -VerifiedPath $Path `
                -SourceRoot $MsiSourceRoot
            $MsiPaths.Add($MsiPath)
            foreach ($CabName in Get-ProjDevMsvcCabNames `
                -MsiPath $MsiPath `
                -CandidateNames $CabCandidates) {
                [void]$CabNames.Add($CabName)
            }
        }
        foreach ($CabName in $CabNames) {
            $Payload = Get-ProjDevMsvcSdkPayload `
                -Recipe $Recipe `
                -LeafName $CabName
            $Path = Get-ProjDevMsvcVerifiedPayload `
                -Context $Context `
                -Definition $Definition `
                -Payload $Payload
            [void](Copy-ProjDevMsvcPayloadToSourceRoot `
                -Context $Context `
                -Payload $Payload `
                -VerifiedPath $Path `
                -SourceRoot $MsiSourceRoot)
        }
        $MsiLogRoot = Join-Path (
            Join-Path $Context.EnvironmentRoot 'msvc'
        ) '_logs'
        foreach ($MsiPath in $MsiPaths) {
            Write-Host (
                "[MSI] $([IO.Path]::GetFileName($MsiPath))"
            ) -ForegroundColor DarkGray
            Invoke-ProjDevMsvcAdministrativeInstall `
                -MsiPath $MsiPath `
                -Destination $StagedRoot `
                -LogPath (Join-Path $MsiLogRoot (
                    "$([IO.Path]::GetFileName($MsiPath)).install.log"
                )) `
                -ControlledRoot $Context.DataRoot
        }

        $Versions = Complete-ProjDevMsvcAssembly `
            -Context $Context `
            -InstallRoot $StagedRoot
        Write-ProjDevMsvcMetadata `
            -Definition $Definition `
            -Recipe $Recipe `
            -InstallRoot $StagedRoot `
            -ToolVersion ([string]$Versions.ToolVersion) `
            -SdkVersion ([string]$Versions.SdkVersion)
        if (-not (Test-ProjDevMsvcInstalled `
            -Context $Context `
            -Definition $Definition `
            -InstallRoot $StagedRoot)) {
            throw 'Staged MSVC installation failed validation.'
        }
        Publish-ProjDevInstallDirectory `
            -Context $Context `
            -Definition $Definition `
            -StagedPath $StagedRoot `
            -TargetPath $Target `
            -ValidatePublished $ValidateInstalled
        return $true
    } finally {
        Remove-ProjDevInstallResidues `
            -Context $Context `
            -Paths @($StagedRoot, $MsiSourceRoot) `
            -Activity 'cleaning MSVC installation work data'
    }
}
