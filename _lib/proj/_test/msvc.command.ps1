[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ProjRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $ProjRoot '_toolchain\setup.ps1')
. (Join-Path $ProjRoot '_toolchain\_modules\msvc\command.ps1')

function Assert-ProjMsvcCommandTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "MSVC command test failed: $Message"
    }
}

$TestBase = [IO.Path]::GetFullPath(
    (Join-Path $ProjRoot '..\..\data\_test')
)
[void][IO.Directory]::CreateDirectory($TestBase)
$TemporaryRoot = Join-Path $TestBase (
    "swawkit-proj-msvc-command-$([Guid]::NewGuid().ToString('N'))"
)
$InstallRoot = Join-Path $TemporaryRoot 'managed msvc'
$InvocationDirectory = Join-Path $TemporaryRoot 'invocation directory'
$ToolVersion = '14.44.35228'
$ExpectedExecutable = Join-Path $InstallRoot (
    "VC\Tools\MSVC\$ToolVersion\bin\Hostx64\x64\cl.exe"
)
[void][IO.Directory]::CreateDirectory(
    (Split-Path -Path $ExpectedExecutable -Parent)
)
[void][IO.Directory]::CreateDirectory($InvocationDirectory)
[IO.File]::WriteAllText($ExpectedExecutable, 'fixture')

$script:ProjMsvcCommandCapture = $null

function Import-ProjDevMsvcCommandEnvironment {
    return [pscustomobject]@{
        Context = [pscustomobject]@{
            InvocationDirectory = $InvocationDirectory
        }
        Definition = [pscustomobject]@{ Channel = '17' }
    }
}

function Get-ProjDevMsvcInstallRoot {
    return $InstallRoot
}

function Get-ProjDevMsvcValidMetadata {
    return [pscustomobject]@{ toolVersion = $ToolVersion }
}

function Invoke-ProjDevConsoleProcess {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [AllowEmptyString()]
        [string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory
    )

    $script:ProjMsvcCommandCapture = [pscustomobject]@{
        Executable = $Executable
        Arguments = [string[]]$Arguments
        WorkingDirectory = $WorkingDirectory
    }
    return 23
}

try {
    [string[]]$Arguments = @(
        '/nologo',
        '/TP',
        'source directory\hello.cpp',
        '/DNAME="hello world"',
        ''
    )
    $ExitCode = Invoke-ProjDevMsvcCommand `
        -ExecutableName 'cl.exe' `
        -Arguments $Arguments

    Assert-ProjMsvcCommandTest `
        -Condition ($ExitCode -eq 23) `
        -Message 'the compiler exit code was not preserved'
    Assert-ProjMsvcCommandTest `
        -Condition (
            $script:ProjMsvcCommandCapture.Executable -ceq
                $ExpectedExecutable
        ) `
        -Message 'the command did not select the managed cl.exe path'
    Assert-ProjMsvcCommandTest `
        -Condition (
            $script:ProjMsvcCommandCapture.WorkingDirectory -ceq
                $InvocationDirectory
        ) `
        -Message 'the invocation directory was not preserved'
    Assert-ProjMsvcCommandTest `
        -Condition (
            $script:ProjMsvcCommandCapture.Arguments.Count -eq
                $Arguments.Count -and
            [string]::Join("`n", $script:ProjMsvcCommandCapture.Arguments) -ceq
                [string]::Join("`n", $Arguments)
        ) `
        -Message 'compiler arguments were changed'

    [IO.File]::Delete($ExpectedExecutable)
    $MissingRejected = $false
    try {
        [void](Invoke-ProjDevMsvcCommand `
            -ExecutableName 'cl.exe' `
            -Arguments @())
    } catch {
        $MissingRejected = $_.Exception.Message -like (
            '*managed MSVC command executable is missing*'
        )
    }
    Assert-ProjMsvcCommandTest `
        -Condition $MissingRejected `
        -Message 'a missing managed compiler was accepted'

    Write-Host '[PASS] Proj MSVC command contract' -ForegroundColor Green
} finally {
    $ResolvedRoot = [IO.Path]::GetFullPath($TemporaryRoot)
    $AllowedPrefix = $TestBase.TrimEnd('\') + '\'
    if ($ResolvedRoot.StartsWith(
        $AllowedPrefix,
        [StringComparison]::OrdinalIgnoreCase
    ) -and
        [IO.Path]::GetFileName($ResolvedRoot).StartsWith(
            'swawkit-proj-msvc-command-',
            [StringComparison]::Ordinal
        ) -and
        [IO.Directory]::Exists($ResolvedRoot)) {
        Remove-Item -LiteralPath $ResolvedRoot -Recurse -Force
    }
}

$global:LASTEXITCODE = 0
