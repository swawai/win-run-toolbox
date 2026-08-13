[CmdletBinding()]
param(
    [string]$LauncherPath = '',
    [string]$CorePath = '',
    [string]$HostPath = '',
    [string]$ToolchainPath = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-ProjJournalAbandonment {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "Run Journal abandonment test failed: $Message"
    }
}

function Invoke-ProjJournalEntry {
    param(
        [Parameter(Mandatory = $true)][string]$EntryPath,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    $PreviousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Output = @(& $EntryPath @Arguments 2>&1)
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousPreference
    }
    return [pscustomobject][ordered]@{
        ExitCode = [int]$ExitCode
        Text = [string]::Join(
            [Environment]::NewLine,
            [string[]]@($Output | ForEach-Object { [string]$_ })
        )
    }
}

function New-ProjJournalAbandonmentAction {
    param([Parameter(Mandatory = $true)][string]$OutputAssembly)

    $OutputAssembly = [IO.Path]::GetFullPath($OutputAssembly)
    [void][IO.Directory]::CreateDirectory(
        [IO.Path]::GetDirectoryName($OutputAssembly)
    )
    Add-Type -OutputType ConsoleApplication -OutputAssembly $OutputAssembly `
        -TypeDefinition @'
using System;
using System.Diagnostics;
using System.IO;

public static class ProjJournalAbandonmentAction
{
    public static int Main()
    {
        string root = Environment.GetEnvironmentVariable(
            "SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT"
        );
        if (String.IsNullOrWhiteSpace(root))
        {
            return 64;
        }
        Directory.CreateDirectory(root);
        ProcessStartInfo start = new ProcessStartInfo();
        start.FileName = Path.Combine(Environment.SystemDirectory, "PING.EXE");
        start.Arguments = "-t 127.0.0.1";
        start.UseShellExecute = false;
        start.CreateNoWindow = true;
        start.RedirectStandardOutput = true;
        start.RedirectStandardError = true;
        using (Process child = Process.Start(start))
        {
            File.WriteAllText(
                Path.Combine(root, "descendant.identity"),
                child.Id.ToString() + Environment.NewLine +
                    child.StartTime.ToFileTimeUtc().ToString()
            );
            Console.WriteLine("journal-before-abandonment");
            Console.Out.Flush();
            child.WaitForExit();
            return child.ExitCode;
        }
    }
}
'@
}

function Test-ProjJournalDescendantAlive {
    param([Parameter(Mandatory = $true)][string]$IdentityPath)

    if (-not [IO.File]::Exists($IdentityPath)) {
        return $false
    }
    $Identity = [IO.File]::ReadAllLines($IdentityPath)
    if ($Identity.Length -ne 2) {
        throw "Invalid descendant identity: $IdentityPath"
    }
    try {
        $Process = [Diagnostics.Process]::GetProcessById([int]$Identity[0])
    } catch [ArgumentException] {
        return $false
    }
    try {
        return $Process.StartTime.ToFileTimeUtc() -eq [int64]$Identity[1]
    } catch [InvalidOperationException] {
        return $false
    } finally {
        $Process.Dispose()
    }
}

function Stop-ProjJournalDescendant {
    param([Parameter(Mandatory = $true)][string]$IdentityPath)

    if (-not [IO.File]::Exists($IdentityPath)) {
        return
    }
    $Identity = [IO.File]::ReadAllLines($IdentityPath)
    if ($Identity.Length -ne 2) {
        throw "Invalid descendant identity: $IdentityPath"
    }
    try {
        $Process = [Diagnostics.Process]::GetProcessById([int]$Identity[0])
    } catch [ArgumentException] {
        return
    }
    try {
        if ($Process.StartTime.ToFileTimeUtc() -eq [int64]$Identity[1]) {
            $Process.Kill()
            [void]$Process.WaitForExit(5000)
        }
    } catch [InvalidOperationException] {
        return
    } finally {
        $Process.Dispose()
    }
}

function Read-ProjSharedJournalText {
    param([Parameter(Mandatory = $true)][string]$Path)

    $Stream = [IO.File]::Open(
        $Path,
        [IO.FileMode]::Open,
        [IO.FileAccess]::Read,
        ([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
    )
    try {
        $Reader = [IO.StreamReader]::new(
            $Stream,
            [Text.UTF8Encoding]::new($false),
            $true,
            4096,
            $true
        )
        try {
            return $Reader.ReadToEnd()
        } finally {
            $Reader.Dispose()
        }
    } finally {
        $Stream.Dispose()
    }
}

function Read-ProjJournalOutputText {
    param([Parameter(Mandatory = $true)][string]$Path)

    $Lines = (Read-ProjSharedJournalText -Path $Path) -split "`n"
    return [string]::Join(
        '',
        [string[]]@($Lines | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            ForEach-Object {
                $Event = $_ | ConvertFrom-Json
                if ($Event.kind -ceq 'output') {
                    [string]$Event.text
                }
            })
    )
}

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
. (Join-Path $PSScriptRoot '_lib\runtime-fixture.ps1')
. (Join-Path $PSScriptRoot '_lib\owned-process-tree.ps1')
$Artifacts = Resolve-ProjCandidateRuntimeArtifacts `
    -LauncherPath $LauncherPath `
    -CorePath $CorePath `
    -HostPath $HostPath `
    -ToolchainPath $ToolchainPath
$TemporaryRoot = Join-Path $RepoRoot (
    "data\_test\swawkit-proj-journal-abandon-$([Guid]::NewGuid().ToString('N'))"
)
$EntryName = "journal-abandon-$([Guid]::NewGuid().ToString('N'))"
$Tree = $null
$DescendantIdentity = $null

try {
    $Runtime = New-ProjCandidateRuntimeFixture `
        -RuntimeHome (Join-Path $TemporaryRoot 'runtime-home') `
        -LauncherPath $Artifacts.LauncherPath `
        -CorePath $Artifacts.CorePath `
        -HostPath $Artifacts.HostPath `
        -ToolchainPath $Artifacts.ToolchainPath
    $EntryPath = Add-ProjCandidateRuntimeEntry `
        -Runtime $Runtime `
        -RelativePath "Favorites\$EntryName.exe"
    $ActionAddress = 'abandon-journal'
    $ActionPath = Join-Path $Runtime.Home ".swaw\$ActionAddress\run.exe"
    New-ProjJournalAbandonmentAction -OutputAssembly $ActionPath

    $Bound = Invoke-ProjJournalEntry `
        -EntryPath $EntryPath `
        -Arguments @(
            '..entry.env.project.SWAWKIT_PROJ_TARGET_PROJECT_ROOT',
            '${SWAWKIT_HOME}'
        )
    Assert-ProjJournalAbandonment `
        -Condition ($Bound.ExitCode -eq 0) `
        -Message "cannot bind the isolated Entry: $($Bound.Text)"
    foreach ($Tool in @('bun', 'pwsh', 'msvc', 'rust', 'go', 'python', 'uv')) {
        $Variable = 'SWAWKIT_PROJ_{0}_MODE' -f $Tool.ToUpperInvariant()
        $Disabled = Invoke-ProjJournalEntry `
            -EntryPath $EntryPath `
            -Arguments @("..entry.env.$Tool.$Variable", 'disabled')
        Assert-ProjJournalAbandonment `
            -Condition ($Disabled.ExitCode -eq 0) `
            -Message "cannot disable ${Tool}: $($Disabled.Text)"
    }

    $DataRoot = Join-Path $Runtime.Home "data\proj.$EntryName"
    $ActionDataRoot = Join-Path $DataRoot "modules\action\$ActionAddress"
    $RunsRoot = Join-Path $ActionDataRoot '_runs'
    $DescendantIdentity = Join-Path $ActionDataRoot 'descendant.identity'
    $Tree = Start-ProjOwnedProcessTree `
        -FilePath $EntryPath `
        -Arguments $ActionAddress `
        -WorkingDirectory $Runtime.Home

    $Deadline = [DateTime]::UtcNow.AddSeconds(15)
    $StatePath = $null
    $State = $null
    $EventsPath = $null
    do {
        $StatePath = Get-ChildItem `
            -LiteralPath $RunsRoot `
            -Filter '_state.json' `
            -File `
            -Recurse `
            -ErrorAction SilentlyContinue |
            Select-Object -First 1 -ExpandProperty FullName
        if ($null -ne $StatePath) {
            try {
                $State = [IO.File]::ReadAllText($StatePath) | ConvertFrom-Json
                $EventsPath = Join-Path (Split-Path $StatePath -Parent) 'events.jsonl'
                $Ready = $State.status -ceq 'running' -and
                    [IO.File]::Exists($EventsPath) -and
                    (Read-ProjJournalOutputText -Path $EventsPath).Contains(
                        'journal-before-abandonment'
                    ) -and
                    (Test-ProjJournalDescendantAlive `
                        -IdentityPath $DescendantIdentity)
            } catch {
                $Ready = $false
            }
        } else {
            $Ready = $false
        }
        if (-not $Ready) {
            Start-Sleep -Milliseconds 50
        }
    } while (-not $Ready -and [DateTime]::UtcNow -lt $Deadline)
    $RootStatus = if ($Tree.WaitForExit(0)) {
        "exited:$($Tree.ExitCode)"
    } else {
        'running'
    }
    $ObservedFiles = if ([IO.Directory]::Exists($DataRoot)) {
        [string]::Join(
            ', ',
            [string[]]@(Get-ChildItem -LiteralPath $DataRoot -File -Recurse |
                ForEach-Object {
                    $_.FullName.Substring($DataRoot.TrimEnd('\').Length + 1)
                })
        )
    } else {
        '<no DataRoot>'
    }
    $ObservedEvents = if ($null -ne $EventsPath -and
        [IO.File]::Exists($EventsPath)) {
        $Text = Read-ProjSharedJournalText -Path $EventsPath
        $Text.Replace("`r", '<CR>').Replace("`n", '<LF>')
    } else {
        '<no events>'
    }
    $DescendantAlive = Test-ProjJournalDescendantAlive `
        -IdentityPath $DescendantIdentity
    $ObservedState = if ($null -eq $State) { '<no state>' } else { $State.status }
    Assert-ProjJournalAbandonment `
        -Condition $Ready `
        -Message (
            'the real Action did not publish a running journal and output event; ' +
            "launcher=$RootStatus; processes=$($Tree.TotalProcesses); " +
            "state=$ObservedState; descendant=$DescendantAlive; " +
            "events=$ObservedEvents; files=$ObservedFiles"
        )

    $RunRoot = Split-Path $StatePath -Parent
    $RunId = Split-Path $RunRoot -Leaf
    $OwnerPath = Join-Path $RunsRoot ".$RunId.owner.lock"
    Assert-ProjJournalAbandonment `
        -Condition (
            [IO.File]::Exists($OwnerPath) -and
            $Tree.TotalProcesses -ge 4
        ) `
        -Message 'the active Action was not fully owned by its journal and test Job'

    $Tree.Dispose()
    $Tree = $null
    $ExitDeadline = [DateTime]::UtcNow.AddSeconds(5)
    while ((Test-ProjJournalDescendantAlive -IdentityPath $DescendantIdentity) -and
        [DateTime]::UtcNow -lt $ExitDeadline) {
        Start-Sleep -Milliseconds 50
    }
    Assert-ProjJournalAbandonment `
        -Condition (-not (Test-ProjJournalDescendantAlive `
            -IdentityPath $DescendantIdentity)) `
        -Message 'closing the owned Job left the Action descendant alive'
    $InterruptedState = [IO.File]::ReadAllText($StatePath) | ConvertFrom-Json
    Assert-ProjJournalAbandonment `
        -Condition (
            $InterruptedState.status -ceq 'running' -and
            [IO.File]::Exists($OwnerPath)
        ) `
        -Message 'process termination bypassed the intended read-time reconciliation boundary'

    $Logs = Invoke-ProjJournalEntry `
        -EntryPath $EntryPath `
        -Arguments @('.logs', $ActionAddress, '--run', $RunId, '--after', '0')
    Assert-ProjJournalAbandonment `
        -Condition ($Logs.ExitCode -eq 0) `
        -Message "the public .logs command could not reconcile the run: $($Logs.Text)"
    $Document = $Logs.Text | ConvertFrom-Json
    $RetainedOutput = [string]::Join(
        '',
        [string[]]@($Document.events | Where-Object { $_.kind -ceq 'output' } |
            ForEach-Object { [string]$_.text })
    )
    Assert-ProjJournalAbandonment `
        -Condition (
            $Document.protocol -ceq 'swawkit.command-run-journal/v1' -and
            $Document.id -ceq $RunId -and
            $Document.state -ceq 'failed' -and
            $Document.error -ceq (
                'command execution owner ended before publishing a terminal state'
            ) -and
            $Document.nextCursor -ge 1 -and
            $RetainedOutput.Contains('journal-before-abandonment') -and
            -not [IO.File]::Exists($OwnerPath)
        ) `
        -Message 'the public journal did not retain output and converge to failed'
} finally {
    if ($null -ne $Tree) {
        $Tree.Dispose()
    }
    if ($null -ne $DescendantIdentity -and
        (Test-ProjJournalDescendantAlive -IdentityPath $DescendantIdentity)) {
        Stop-ProjJournalDescendant -IdentityPath $DescendantIdentity
    }
    if ([IO.Directory]::Exists($TemporaryRoot)) {
        Remove-ProjCandidateRuntimeFixture -Path $TemporaryRoot
    }
}

Write-Host '[PASS] Proj Run Journal abnormal-exit reconciliation' -ForegroundColor Green
$global:LASTEXITCODE = 0
