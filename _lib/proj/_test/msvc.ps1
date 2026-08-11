[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ToolchainPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
Add-Type -AssemblyName System.IO.Compression

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\setup.ps1')

function Assert-ProjMsvcTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "MSVC test failed: $Message"
    }
}

function New-ProjMsvcPayloadFixture {
    param([Parameter(Mandatory = $true)][string]$FileName)

    return [pscustomobject]@{
        fileName = $FileName
        sha256 = 'a' * 64
        size = 10
        url = (
            'https://download.visualstudio.microsoft.com/fixture/' +
            [Uri]::EscapeDataString($FileName)
        )
    }
}

function New-ProjMsvcManifestFixture {
    param([Parameter(Mandatory = $true)][object]$Definition)

    $Packages = [Collections.Generic.List[object]]::new()
    foreach ($Template in [string[]]$Definition.ToolPackageTemplates) {
        $Id = $Template.Replace('{tool}', '14.44.17.14')
        if ($Id.EndsWith(
            '.res.base',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            foreach ($Language in @('de-DE', 'en-US')) {
                $Packages.Add([pscustomobject]@{
                    id = $Id
                    language = $Language
                    payloads = @(
                        New-ProjMsvcPayloadFixture `
                            -FileName "tool-$Language.vsix"
                    )
                })
            }
        } else {
            $Packages.Add([pscustomobject]@{
                id = $Id
                payloads = @(
                    New-ProjMsvcPayloadFixture `
                        -FileName "$($Packages.Count)-tool.vsix"
                )
            })
        }
    }
    $Packages.Add([pscustomobject]@{
        id = 'Microsoft.VisualStudio.Component.Windows11SDK.26100'
        dependencies = [pscustomobject]@{
            Win11SDK_10 = '[10.0.0,11.0)'
        }
        payloads = @()
    })
    $SdkPayloads = foreach ($Name in [string[]]$Definition.SdkMsiNames) {
        New-ProjMsvcPayloadFixture -FileName "Installers\$Name"
    }
    $Packages.Add([pscustomobject]@{
        id = 'Win11SDK_10'
        payloads = [object[]]$SdkPayloads
    })
    return [pscustomobject]@{
        packages = [object[]]$Packages.ToArray()
    }
}

function Write-ProjMsvcFixtureInstall {
    param(
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [Parameter(Mandatory = $true)][string]$ToolVersion,
        [Parameter(Mandatory = $true)][string]$SdkVersion
    )

    foreach ($RelativePath in Get-ProjDevMsvcRequiredPaths `
        -ToolVersion $ToolVersion `
        -SdkVersion $SdkVersion) {
        $Path = Resolve-ProjDevChildPath `
            -Root $InstallRoot `
            -RelativePath $RelativePath `
            -Description 'MSVC test file'
        [void][IO.Directory]::CreateDirectory(
            (Split-Path -Path $Path -Parent)
        )
        [IO.File]::WriteAllText(
            $Path,
            "fixture:$RelativePath",
            [Text.UTF8Encoding]::new($false)
        )
    }
}

$PreviousMode = [Environment]::GetEnvironmentVariable(
    'SWAWKIT_PROJ_MSVC_MODE',
    'Process'
)
$PreviousChannel = [Environment]::GetEnvironmentVariable(
    'SWAWKIT_PROJ_MSVC_CHANNEL',
    'Process'
)
$RuntimeEnvironmentNames = @(
    'SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL',
    'SWAWKIT_HOME',
    'SWAWKIT_PROJ_TARGET_PROJECT_ROOT',
    'SWAWKIT_PROJ_DATA_ROOT',
    'SWAWKIT_PROJ_ENTRY_COMMAND',
    'SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR',
    'SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION',
    'SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION',
    'SWAWKIT_PROJ_CORE_TOOLCHAIN_EXECUTABLE',
    'SWAWKIT_PROJ_BUN_MODE',
    'SWAWKIT_PROJ_BUN_VERSION',
    'SWAWKIT_PROJ_BUN_SHA256'
)
$PreviousRuntimeEnvironment = @{}
foreach ($Name in $RuntimeEnvironmentNames) {
    $PreviousRuntimeEnvironment[$Name] =
        [Environment]::GetEnvironmentVariable($Name, 'Process')
}
$PreviousPath = [string]$env:PATH
$PreviousDevelopmentEnvironment = @{}
$ProcessEnvironment = [Environment]::GetEnvironmentVariables('Process')
foreach ($Name in [string[]]@($ProcessEnvironment.Keys)) {
    if ($Name.StartsWith(
        'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_',
        [StringComparison]::OrdinalIgnoreCase
    )) {
        $PreviousDevelopmentEnvironment[$Name] =
            [string]$ProcessEnvironment[$Name]
    }
}
$TestTemporaryBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TestTemporaryBase)
$TemporaryRoot = Join-Path $TestTemporaryBase (
    "swawkit-proj-msvc-$([Guid]::NewGuid().ToString('N'))"
)
$ResolvedToolchainPath = [IO.Path]::GetFullPath($ToolchainPath)
if (-not [IO.File]::Exists($ResolvedToolchainPath)) {
    throw "Toolchain test candidate is missing: $ResolvedToolchainPath"
}

try {
    $env:SWAWKIT_PROJ_MSVC_MODE = 'managed'
    $env:SWAWKIT_PROJ_MSVC_CHANNEL = '17'
    $Definition = Get-ProjDevMsvcDefinition
    Assert-ProjMsvcTest `
        -Condition (
            [string]$Definition.Channel -ceq '17' -and
            [string]$Definition.ChannelUrl -ceq
                'https://aka.ms/vs/17/release/channel'
        ) `
        -Message 'channel declaration did not produce a stable definition'

    $ManifestPayload = ConvertTo-ProjDevMsvcPayload `
        -Payload (New-ProjMsvcPayloadFixture `
            -FileName 'VisualStudio.vsman') `
        -Description 'Visual Studio manifest fixture'
    $Manifest = New-ProjMsvcManifestFixture -Definition $Definition
    $Recipe = Resolve-ProjDevMsvcManifest `
        -Definition $Definition `
        -ManifestPayload $ManifestPayload `
        -VisualStudioManifest $Manifest
    Assert-ProjMsvcTest `
        -Condition (
            [string]$Recipe.ToolPackageVersion -ceq '14.44.17.14' -and
            [string]$Recipe.SdkPackageId -ceq 'Win11SDK_10' -and
            @($Recipe.ToolPayloads).Count -eq 7 -and
            @($Recipe.MsiPayloads).Count -eq 8 -and
            @($Recipe.ToolPayloads | Where-Object {
                [string]$_.LeafName -ceq 'tool-en-US.vsix'
            }).Count -eq 1 -and
            @($Recipe.ToolPayloads | Where-Object {
                [string]$_.LeafName -ceq 'tool-de-DE.vsix'
            }).Count -eq 0
        ) `
        -Message 'manifest resolution did not select exact x64/en-US packages'

    $InvalidPayload = New-ProjMsvcPayloadFixture -FileName 'bad.vsix'
    $InvalidPayload.sha256 = ''
    $Rejected = $false
    try {
        [void](ConvertTo-ProjDevMsvcPayload `
            -Payload $InvalidPayload `
            -Description 'test payload')
    } catch {
        $Rejected = $_.Exception.Message -like '*Invalid Microsoft payload*'
    }
    Assert-ProjMsvcTest `
        -Condition $Rejected `
        -Message 'a Microsoft payload without SHA-256 was accepted'

    $CabFixture = Join-Path $TemporaryRoot 'cab-scan.msi'
    [void][IO.Directory]::CreateDirectory($TemporaryRoot)
    [IO.File]::WriteAllBytes(
        $CabFixture,
        [Text.Encoding]::ASCII.GetBytes(
            "binary`0sdk-one.cab`0other`0sdk_two.cab`0"
        )
    )
    $CabNames = Get-ProjDevMsvcCabNames `
        -MsiPath $CabFixture `
        -CandidateNames @(
            'sdk-one.cab',
            'sdk_two.cab',
            'not-present.cab'
        )
    Assert-ProjMsvcTest `
        -Condition (
            $CabNames.Count -eq 2 -and
            $CabNames -contains 'sdk-one.cab' -and
            $CabNames -contains 'sdk_two.cab'
        ) `
        -Message 'MSI CAB discovery lost dependencies'

    $VsixPath = Join-Path $TemporaryRoot 'fixture.vsix'
    $VsixStream = [IO.File]::Open(
        $VsixPath,
        [IO.FileMode]::Create,
        [IO.FileAccess]::ReadWrite,
        [IO.FileShare]::None
    )
    try {
        $Zip = [IO.Compression.ZipArchive]::new(
            $VsixStream,
            [IO.Compression.ZipArchiveMode]::Create,
            $true
        )
        try {
            foreach ($Name in @(
                'Contents/VC/Tools/file.txt',
                'ignored/file.txt'
            )) {
                $Entry = $Zip.CreateEntry($Name)
                $Writer = [IO.StreamWriter]::new($Entry.Open())
                try { $Writer.Write('fixture') } finally { $Writer.Dispose() }
            }
        } finally {
            $Zip.Dispose()
        }
    } finally {
        $VsixStream.Dispose()
    }
    $VsixRoot = Join-Path $TemporaryRoot 'vsix'
    Expand-ProjDevMsvcVsix `
        -ArchivePath $VsixPath `
        -Destination $VsixRoot `
        -ControlledRoot $TemporaryRoot
    Assert-ProjMsvcTest `
        -Condition (
            [IO.File]::Exists(
                (Join-Path $VsixRoot 'VC\Tools\file.txt')
            ) -and
            -not [IO.File]::Exists(
                (Join-Path $VsixRoot 'ignored\file.txt')
            )
        ) `
        -Message 'VSIX Contents projection is incorrect'

    $SlipPath = Join-Path $TemporaryRoot 'slip.vsix'
    $SlipStream = [IO.File]::Open(
        $SlipPath,
        [IO.FileMode]::Create,
        [IO.FileAccess]::ReadWrite,
        [IO.FileShare]::None
    )
    try {
        $SlipZip = [IO.Compression.ZipArchive]::new(
            $SlipStream,
            [IO.Compression.ZipArchiveMode]::Create,
            $true
        )
        try {
            $Entry = $SlipZip.CreateEntry('Contents/../escaped.txt')
            $Writer = [IO.StreamWriter]::new($Entry.Open())
            try { $Writer.Write('escape') } finally { $Writer.Dispose() }
        } finally {
            $SlipZip.Dispose()
        }
    } finally {
        $SlipStream.Dispose()
    }
    $SlipRejected = $false
    try {
        Expand-ProjDevMsvcVsix `
            -ArchivePath $SlipPath `
            -Destination (Join-Path $TemporaryRoot 'slip-root') `
            -ControlledRoot $TemporaryRoot
    } catch {
        $SlipRejected = $_.Exception.Message -like '*escapes its root*'
    }
    Assert-ProjMsvcTest `
        -Condition (
            $SlipRejected -and
            -not [IO.File]::Exists(
                (Join-Path $TemporaryRoot 'escaped.txt')
            )
        ) `
        -Message 'VSIX path traversal was not rejected'

    $ProjectRoot = Join-Path $TemporaryRoot 'project'
    $DataRoot = Join-Path $TemporaryRoot 'data'
    [void][IO.Directory]::CreateDirectory($ProjectRoot)
    [void][IO.Directory]::CreateDirectory($DataRoot)
    $InputRevision = 'sha256-' + ('a' * 64)
    $ProfilePath = Join-Path $DataRoot '_profile.json'
    [IO.File]::WriteAllText($ProfilePath, '{}')
    $ProfileRevision = 'sha256-' + (
        Get-ProjDevFileSha256 -Path $ProfilePath
    )
    $Context = New-ProjDevContext `
        -ProjectRoot $ProjectRoot `
        -DataRoot $DataRoot `
        -CacheDataRoot (Join-Path $TemporaryRoot 'shared cache') `
        -EnvironmentInputRevision $InputRevision `
        -CommandProfileRevision $ProfileRevision
    $TargetRoot = Get-ProjDevMsvcInstallRoot `
        -Context $Context `
        -Definition $Definition
    $StageRoot = Join-Path (
        Split-Path -Path $TargetRoot -Parent
    ) '.partial-fixture'
    Write-ProjMsvcFixtureInstall `
        -InstallRoot $StageRoot `
        -ToolVersion '14.44.35228' `
        -SdkVersion '10.0.26100.0'
    $MetadataRecipe = [pscustomobject]@{
        ManifestUrl = 'https://download.visualstudio.microsoft.com/vsman'
        ManifestSha256 = 'b' * 64
        ToolPackageVersion = '14.44.17.14'
        SdkPackageId = 'Win11SDK_10'
    }
    Write-ProjDevMsvcMetadata `
        -Definition $Definition `
        -Recipe $MetadataRecipe `
        -InstallRoot $StageRoot `
        -ToolVersion '14.44.35228' `
        -SdkVersion '10.0.26100.0'
    Assert-ProjMsvcTest `
        -Condition (Test-ProjDevMsvcInstalled `
            -Context $Context `
            -Definition $Definition `
            -InstallRoot $StageRoot) `
        -Message 'staged MSVC fixture was not trusted'
    $Validator = {
        param($ValidationContext, $ValidationDefinition, $InstallRoot)

        return Test-ProjDevMsvcInstalled `
            -Context $ValidationContext `
            -Definition $ValidationDefinition `
            -InstallRoot $InstallRoot
    }
    Publish-ProjDevInstallDirectory `
        -Context $Context `
        -Definition $Definition `
        -StagedPath $StageRoot `
        -TargetPath $TargetRoot `
        -ValidatePublished $Validator
    Assert-ProjMsvcTest `
        -Condition (Test-ProjDevMsvcInstalled `
            -Context $Context `
            -Definition $Definition) `
        -Message 'published MSVC fixture was not trusted'

    $Plan = New-ProjDevEnvironmentPlan
    Add-ProjDevMsvcEnvironment `
        -Context $Context `
        -Definition $Definition `
        -Plan $Plan
    $Attempt = Start-ProjDevSetupProviderPublication -Context $Context
    Set-ProjDevEnvironmentVariable `
        -Plan $Plan `
        -Name (Get-ProjDevSetupPublicationTokenVariable) `
        -Value ([string]$Attempt.Token)
    $Scripts = ConvertTo-ProjDevEnvironmentScripts -Plan $Plan
    $DuplicateMsvcVariables = @($Plan.Variables.Keys | Where-Object {
        ([string]$_).StartsWith(
            'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_MSVC_',
            [StringComparison]::OrdinalIgnoreCase
        )
    })
    Assert-ProjMsvcTest `
        -Condition (
            $Scripts.Cmd -like '*VCToolsVersion=14.44.35228*' -and
            $Scripts.Cmd -like '*WindowsSDKVersion=10.0.26100.0\*' -and
            -not $Scripts.Ps1.Contains(
                'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_MSVC_'
            ) -and
            $DuplicateMsvcVariables.Count -eq 0 -and
            $Plan.PathPrefixes.Count -eq 3
        ) `
        -Message 'generated MSVC environment lost the baseline contract'

    $env:SWAWKIT_PROJ_BUN_MODE = 'managed'
    $env:SWAWKIT_PROJ_BUN_VERSION = '1.0.0'
    $env:SWAWKIT_PROJ_BUN_SHA256 = ''
    [void](Publish-ProjDevEnvironmentScripts `
        -Context $Context `
        -Scripts $Scripts)
    Complete-ProjDevSetupProviderPublication `
        -Context $Context `
        -Attempt $Attempt
    $env:SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL = '1'
    $env:SWAWKIT_HOME = [IO.Path]::GetFullPath(
        (Join-Path $ProjRoot '..\..')
    )
    $env:SWAWKIT_PROJ_TARGET_PROJECT_ROOT = $ProjectRoot
    $env:SWAWKIT_PROJ_DATA_ROOT = $DataRoot
    $env:SWAWKIT_PROJ_ENTRY_COMMAND = 'fixture'
    $env:SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR = $ProjectRoot
    $env:SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION = $InputRevision
    $env:SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION = $ProfileRevision
    $env:SWAWKIT_PROJ_CORE_TOOLCHAIN_EXECUTABLE = $ResolvedToolchainPath
    foreach ($Name in [string[]]@(
        [Environment]::GetEnvironmentVariables('Process').Keys
    )) {
        if ($Name.StartsWith(
            'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
        }
    }
    $env:SWAWKIT_PROJ_BUN_VERSION = '2.0.0'
    . (Join-Path $ProjRoot '_toolchain\_modules\msvc\runtime.ps1')
    $OriginalMsvcMetadataValidator = (
        Get-Command Get-ProjDevMsvcValidMetadata -CommandType Function
    ).ScriptBlock
    $script:ProjMsvcCurrentMetadataValidationCount = 0
    Set-Item -LiteralPath Function:\Get-ProjDevMsvcValidMetadata -Value {
        param($Context, $Definition, $InstallRoot)

        $script:ProjMsvcCurrentMetadataValidationCount++
        throw 'Current must not inspect MSVC installation metadata.'
    }
    try {
        $RuntimeRequirement = Import-ProjDevMsvcCommandEnvironment
    } finally {
        Set-Item `
            -LiteralPath Function:\Get-ProjDevMsvcValidMetadata `
            -Value $OriginalMsvcMetadataValidator
    }
    $LeakedMetadata = @(
        [Environment]::GetEnvironmentVariables('Process').Keys |
            Where-Object {
                ([string]$_).StartsWith(
                    'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_',
                    [StringComparison]::OrdinalIgnoreCase
                )
            }
    )
    Assert-ProjMsvcTest `
        -Condition (
            [string]$RuntimeRequirement.Definition.Channel -ceq '17' -and
            $script:ProjMsvcCurrentMetadataValidationCount -eq 0 -and
            $LeakedMetadata.Count -eq 0 -and
            [string]$env:VCToolsVersion -ceq '14.44.35228' -and
            [string]$env:WindowsSDKVersion -ceq '10.0.26100.0\'
        ) `
        -Message (
            'an unrelated declaration blocked MSVC or export metadata leaked'
        )
    $ExpectedWindowsSdkVersion = [string]$env:WindowsSDKVersion
    $env:WindowsSDKVersion = '10.0.26100.0'
    $InvalidVersionRejected = $false
    try {
        Assert-ProjDevMsvcEnvironmentCurrent `
            -Context $RuntimeRequirement.Context `
            -Definition $RuntimeRequirement.Definition
    } catch {
        $InvalidVersionRejected = $_.Exception.Message -like (
            "*invalid version variables*Run 'fixture .dev.setup'*"
        )
    } finally {
        $env:WindowsSDKVersion = $ExpectedWindowsSdkVersion
    }
    Assert-ProjMsvcTest `
        -Condition $InvalidVersionRejected `
        -Message 'Current accepted a malformed WindowsSDKVersion'

    [IO.File]::AppendAllText(
        (Join-Path $TargetRoot (
            'VC\Tools\MSVC\14.44.35228\bin\Hostx64\x64\cl.exe'
        )),
        'damage'
    )
    Assert-ProjMsvcTest `
        -Condition (-not (Test-ProjDevMsvcInstalled `
            -Context $Context `
            -Definition $Definition)) `
        -Message 'installed compiler corruption was not detected'

    Write-Host '[PASS] Proj MSVC module test' -ForegroundColor Green
} finally {
    $env:PATH = $PreviousPath
    $CurrentEnvironment = [Environment]::GetEnvironmentVariables('Process')
    foreach ($Name in [string[]]@($CurrentEnvironment.Keys)) {
        if ($Name.StartsWith(
            'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
        }
    }
    foreach ($Name in $PreviousDevelopmentEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            [string]$PreviousDevelopmentEnvironment[$Name],
            'Process'
        )
    }
    foreach ($Name in $RuntimeEnvironmentNames) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            $PreviousRuntimeEnvironment[$Name],
            'Process'
        )
    }
    [Environment]::SetEnvironmentVariable(
        'SWAWKIT_PROJ_MSVC_MODE',
        $PreviousMode,
        'Process'
    )
    [Environment]::SetEnvironmentVariable(
        'SWAWKIT_PROJ_MSVC_CHANNEL',
        $PreviousChannel,
        'Process'
    )
    $ResolvedRoot = [IO.Path]::GetFullPath($TemporaryRoot)
    $AllowedPrefix = $TestTemporaryBase.TrimEnd('\') + '\'
    if ($ResolvedRoot.StartsWith(
        $AllowedPrefix,
        [StringComparison]::OrdinalIgnoreCase
    ) -and
        [IO.Path]::GetFileName($ResolvedRoot).StartsWith(
            'swawkit-proj-msvc-',
            [StringComparison]::Ordinal
        ) -and
        [IO.Directory]::Exists($ResolvedRoot)) {
        Remove-Item -LiteralPath $ResolvedRoot -Recurse -Force
    }
}
