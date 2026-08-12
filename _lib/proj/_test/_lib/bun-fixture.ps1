Set-StrictMode -Version 2.0

$script:ProjBunFixtureRoot = $PSScriptRoot
. (Join-Path $PSScriptRoot 'argument-payload.ps1')

function Enter-ProjBunIsolatedEnvironment {
    param([Parameter(Mandatory = $true)][string[]]$ProjectVariableNames)

    $ProcessEnvironment = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    $OwnedProjectNames = [Collections.Generic.HashSet[string]]::new(
        [StringComparer]::OrdinalIgnoreCase
    )
    foreach ($Name in $ProjectVariableNames) {
        [void]$OwnedProjectNames.Add($Name)
    }
    foreach ($Name in [string[]]@($ProcessEnvironment.Keys)) {
        if ($Name.StartsWith(
            'SWAWKIT_PROJ_',
            [StringComparison]::OrdinalIgnoreCase
        ) -and -not $Name.StartsWith(
            'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            [void]$OwnedProjectNames.Add($Name)
        }
    }

    $ProjectValues = @{}
    foreach ($Name in $OwnedProjectNames) {
        $ProjectValues[$Name] = [Environment]::GetEnvironmentVariable(
            $Name,
            [EnvironmentVariableTarget]::Process
        )
        [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
    }

    $DevelopmentValues = @{}
    foreach ($Name in [string[]]@($ProcessEnvironment.Keys)) {
        if ($Name.StartsWith(
            'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            $DevelopmentValues[$Name] = [string]$ProcessEnvironment[$Name]
            [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
        }
    }
    return [pscustomobject][ordered]@{
        ProjectValues = $ProjectValues
        DevelopmentValues = $DevelopmentValues
    }
}

function Exit-ProjBunIsolatedEnvironment {
    param([Parameter(Mandatory = $true)][object]$Snapshot)

    $ProcessEnvironment = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    foreach ($Name in [string[]]@($ProcessEnvironment.Keys)) {
        if ($Name.StartsWith(
            'SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_',
            [StringComparison]::OrdinalIgnoreCase
        )) {
            [Environment]::SetEnvironmentVariable($Name, $null, 'Process')
        }
    }
    foreach ($Name in $Snapshot.DevelopmentValues.Keys) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            [string]$Snapshot.DevelopmentValues[$Name],
            [EnvironmentVariableTarget]::Process
        )
    }
    foreach ($Name in $Snapshot.ProjectValues.Keys) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            $Snapshot.ProjectValues[$Name],
            [EnvironmentVariableTarget]::Process
        )
    }
}

function Assert-ProjBunTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw "Bun test failed: $Message"
    }
}

function Assert-ProjBunThrows {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [Parameter(Mandatory = $true)][string]$Pattern
    )

    $Message = ''
    try {
        & $Action
    } catch {
        $Message = $_.Exception.Message
    }
    Assert-ProjBunTest `
        -Condition ($Message -like $Pattern) `
        -Message "expected error '$Pattern', received '$Message'"
}

function New-ProjBunFixtureExecutable {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Version
    )

    $ClassName = "ProjBunFixture$([Guid]::NewGuid().ToString('N'))"
    $Source = @"
using System;
using System.Collections;
using System.IO;

public static class $ClassName
{
    public static int Main(string[] args)
    {
        if (args.Length == 1 && args[0] == "--version")
        {
            Console.WriteLine("$Version");
            return 0;
        }

        if (args.Length == 1 && args[0] == "assert-no-export-metadata")
        {
            foreach (DictionaryEntry item in Environment.GetEnvironmentVariables())
            {
                if (((string)item.Key).StartsWith(
                    "SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_",
                    StringComparison.OrdinalIgnoreCase))
                {
                    return 91;
                }
            }
            return 0;
        }

        string capture = Environment.GetEnvironmentVariable("SWAWKIT_PROJ_TEST_BUN_CAPTURE");
        if (!String.IsNullOrEmpty(capture))
        {
            string[] lines = new string[args.Length + 4];
            lines[0] = Environment.CurrentDirectory;
            lines[1] = Environment.GetEnvironmentVariable("SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR");
            lines[2] = Environment.GetEnvironmentVariable("SWAWKIT_PROJ_CORE_COMMAND_ADDRESS");
            lines[3] = Environment.GetEnvironmentVariable("SWAWKIT_PROJ_CORE_COMMAND_DIR");
            Array.Copy(args, 0, lines, 4, args.Length);
            File.WriteAllLines(capture, lines);
        }

        foreach (string arg in args)
        {
            if (arg.StartsWith("exit:", StringComparison.Ordinal))
            {
                return Int32.Parse(arg.Substring(5));
            }
        }
        return 0;
    }
}
"@
    Add-Type `
        -TypeDefinition $Source `
        -Language CSharp `
        -OutputAssembly $Path `
        -OutputType ConsoleApplication
}

function New-ProjBunTestDefinition {
    param(
        [Parameter(Mandatory = $true)][string]$ArchivePath,
        [Parameter(Mandatory = $true)][string]$Sha256
    )

    return [pscustomobject][ordered]@{
        Schema = 'swawkit.proj-dev.module.v0'
        Name = 'bun'
        Mode = 'managed'
        RequestedVersion = '1.2.15'
        Version = '1.2.15'
        Url = $ArchivePath
        SourceIdentity = "fixture:$ArchivePath"
        ProjectSha256 = $Sha256
        Sha256 = $Sha256
        Verification = 'project'
        Release = @{
            Provider = 'fixture'
        }
        ArchiveSubdir = 'bun-windows-x64'
        RecipeVersion = 'fixture-1'
        Executable = 'bun.exe'
        RequiredPaths = [string[]]@('bun.exe', 'bunx.cmd')
        ReleaseResolved = $true
        SelectionStatus = 'none'
    }
}

function Set-ProjBunProcessEnvironment {
    param([Parameter(Mandatory = $true)][hashtable]$Values)

    foreach ($Name in $Values.Keys) {
        [Environment]::SetEnvironmentVariable(
            $Name,
            [string]$Values[$Name],
            [EnvironmentVariableTarget]::Process
        )
    }
}

function Invoke-ProjBunEntryFixture {
    param(
        [Parameter(Mandatory = $true)][string]$PowerShell,
        [Parameter(Mandatory = $true)][string]$EntryPath,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [AllowEmptyString()]
        [string[]]$Arguments
    )

    $Payload = ConvertTo-ProjArgumentPayload -Arguments $Arguments
    $Runner = Join-Path $script:ProjBunFixtureRoot 'powershell-runner.ps1'
    $PreviousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Output = @(& $PowerShell `
            -NoLogo `
            -NoProfile `
            -ExecutionPolicy Bypass `
            -File $Runner `
            -EntryPath $EntryPath `
            -ArgumentPayload $Payload 2>&1)
        $ExitCode = [int]$LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousErrorActionPreference
    }
    return [pscustomobject][ordered]@{
        ExitCode = $ExitCode
        Output = [string]::Join("`n", [string[]]$Output)
    }
}

function Invoke-ProjToolchainCommandFixture {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$Handler,
        [AllowEmptyCollection()]
        [AllowEmptyString()]
        [string[]]$Arguments = @()
    )

    $Info = [Diagnostics.ProcessStartInfo]::new()
    $Info.FileName = $Executable
    $Info.Arguments = [string]::Join(
        ' ',
        [string[]](@('command-v1', $Handler) + $Arguments)
    )
    $Info.UseShellExecute = $false
    $Info.CreateNoWindow = $true
    $Info.RedirectStandardOutput = $true
    $Info.RedirectStandardError = $true
    $Process = [Diagnostics.Process]::Start($Info)
    try {
        $StandardOutputTask = $Process.StandardOutput.ReadToEndAsync()
        $StandardErrorTask = $Process.StandardError.ReadToEndAsync()
        if (-not $Process.WaitForExit(30000)) {
            $Process.Kill()
            $Process.WaitForExit()
            throw "Toolchain handler '$Handler' timed out"
        }
        $StandardOutput = $StandardOutputTask.GetAwaiter().GetResult()
        $StandardError = $StandardErrorTask.GetAwaiter().GetResult()
        return [pscustomobject][ordered]@{
            ExitCode = [int]$Process.ExitCode
            Output = ($StandardOutput + $StandardError).TrimEnd()
        }
    } finally {
        $Process.Dispose()
    }
}


function Assert-ProjBunEnvironmentScriptsUsable {
    param(
        [Parameter(Mandatory = $true)][object]$Context,
        [Parameter(Mandatory = $true)][string]$ExpectedExecutable,
        [Parameter(Mandatory = $true)][string]$PowerShell
    )

    $EscapedPs1Path = $Context.EnvPs1Path.Replace("'", "''")
    $PsCommand = (
        ". '$EscapedPs1Path'; " +
        '(Get-Command bun.exe -CommandType Application | ' +
        'Select-Object -First 1).Source; ' +
        'bun.exe --version; exit $LASTEXITCODE'
    )
    $PreviousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $PsOutput = @(& $PowerShell `
            -NoLogo `
            -NoProfile `
            -ExecutionPolicy Bypass `
            -Command $PsCommand 2>&1)
        $PsExitCode = [int]$LASTEXITCODE

        $WrapperName = ".$([Guid]::NewGuid().ToString('N')).activate.cmd"
        $WrapperPath = Join-Path $Context.EnvironmentRoot $WrapperName
        $Wrapper = (
            "@echo off`r`n" +
            "call env.cmd`r`n" +
            "where bun.exe`r`n" +
            "bun.exe --version`r`n"
        )
        [IO.File]::WriteAllText(
            $WrapperPath,
            $Wrapper,
            [Text.UTF8Encoding]::new($false)
        )
        $CmdStartInfo = [Diagnostics.ProcessStartInfo]::new()
        $CmdStartInfo.FileName = $env:ComSpec
        $CmdStartInfo.Arguments = "/d /s /c `"$WrapperName`""
        $CmdStartInfo.WorkingDirectory = $Context.EnvironmentRoot
        $CmdStartInfo.UseShellExecute = $false
        $CmdStartInfo.CreateNoWindow = $true
        $CmdStartInfo.RedirectStandardOutput = $true
        $CmdStartInfo.RedirectStandardError = $true
        $CmdProcess = [Diagnostics.Process]::Start($CmdStartInfo)
        try {
            $CmdStandardOutput = $CmdProcess.StandardOutput.ReadToEnd()
            $CmdStandardError = $CmdProcess.StandardError.ReadToEnd()
            $CmdProcess.WaitForExit()
            $CmdExitCode = [int]$CmdProcess.ExitCode
        } finally {
            $CmdProcess.Dispose()
        }
        $CmdOutput = @(
            ($CmdStandardOutput + $CmdStandardError) -split "`r?`n" |
                Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
        )
    } finally {
        $ErrorActionPreference = $PreviousErrorActionPreference
    }

    Assert-ProjBunTest `
        -Condition ($PsExitCode -eq 0 -and
            $PsOutput.Count -ge 2 -and
            (Get-ProjDevCanonicalPath -Path ([string]$PsOutput[0])) -ceq
            (Get-ProjDevCanonicalPath -Path $ExpectedExecutable) -and
            [string]$PsOutput[-1] -ceq '1.2.15') `
        -Message "env.ps1 did not activate managed Bun: $PsOutput"
    Assert-ProjBunTest `
        -Condition ($CmdExitCode -eq 0 -and
            $CmdOutput.Count -ge 2 -and
            (Get-ProjDevCanonicalPath -Path ([string]$CmdOutput[0])) -ceq
            (Get-ProjDevCanonicalPath -Path $ExpectedExecutable) -and
            [string]$CmdOutput[-1] -ceq '1.2.15') `
        -Message "env.cmd did not activate managed Bun: $CmdOutput"
    $ExpectedPathLine = (
        'set "PATH=' +
        (Split-Path -Path $ExpectedExecutable -Parent) +
        ';%PATH%"'
    )
    Assert-ProjBunTest `
        -Condition (
            [IO.File]::ReadAllLines($Context.EnvCmdPath) -ccontains
            $ExpectedPathLine
        ) `
        -Message 'env.cmd did not prepend the managed Bun directory to PATH'
}

function Assert-ProjBunZipTraversalRejected {
    param(
        [Parameter(Mandatory = $true)][string]$TemporaryRoot,
        [Parameter(Mandatory = $true)][string]$FixtureRoot
    )

    $SlipArchive = Join-Path $FixtureRoot 'slip.zip'
    $SlipStream = [IO.File]::Open(
        $SlipArchive,
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
            $SlipEntry = $SlipZip.CreateEntry('../escaped.txt')
            $Writer = [IO.StreamWriter]::new($SlipEntry.Open())
            try {
                $Writer.Write('escape')
            } finally {
                $Writer.Dispose()
            }
        } finally {
            $SlipZip.Dispose()
        }
    } finally {
        $SlipStream.Dispose()
    }

    $SlipDestination = Join-Path $TemporaryRoot 'slip-destination'
    Assert-ProjBunThrows `
        -Action {
            Expand-ProjDevZipSafely `
                -ArchivePath $SlipArchive `
                -Destination $SlipDestination `
                -ControlledRoot $TemporaryRoot
        } `
        -Pattern '*escapes extraction*'
    Assert-ProjBunTest `
        -Condition (-not [IO.File]::Exists(
            (Join-Path $TemporaryRoot 'escaped.txt')
        )) `
        -Message 'ZIP traversal wrote outside the extraction directory'
}
