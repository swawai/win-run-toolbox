Set-StrictMode -Version 2.0

if ($null -eq ('ProjLauncherJobTest' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class ProjLauncherJobTest
{
    [StructLayout(LayoutKind.Sequential)]
    private struct BasicLimitInformation
    {
        public long PerProcessUserTimeLimit;
        public long PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize;
        public UIntPtr MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass;
        public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct IoCounters
    {
        public ulong ReadOperationCount;
        public ulong WriteOperationCount;
        public ulong OtherOperationCount;
        public ulong ReadTransferCount;
        public ulong WriteTransferCount;
        public ulong OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ExtendedLimitInformation
    {
        public BasicLimitInformation BasicLimitInformation;
        public IoCounters IoInfo;
        public UIntPtr ProcessMemoryLimit;
        public UIntPtr JobMemoryLimit;
        public UIntPtr PeakProcessMemoryUsed;
        public UIntPtr PeakJobMemoryUsed;
    }

    private const int JobObjectExtendedLimitInformation = 9;
    private const uint JobObjectLimitKillOnJobClose = 0x00002000;

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateJobObject(IntPtr attributes, string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetInformationJobObject(
        IntPtr job,
        int informationClass,
        ref ExtendedLimitInformation information,
        uint informationLength
    );

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateEvent(
        IntPtr attributes,
        bool manualReset,
        bool initialState,
        string name
    );

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool IsProcessInJob(
        IntPtr process,
        IntPtr job,
        out bool result
    );

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr handle);

    public static IntPtr CreateKillOnCloseJob(string name)
    {
        IntPtr job = CreateJobObject(IntPtr.Zero, name);
        if (job == IntPtr.Zero) {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        var information = new ExtendedLimitInformation();
        information.BasicLimitInformation.LimitFlags =
            JobObjectLimitKillOnJobClose;
        if (!SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            ref information,
            (uint)Marshal.SizeOf(typeof(ExtendedLimitInformation)))) {
            int error = Marshal.GetLastWin32Error();
            CloseHandle(job);
            throw new Win32Exception(error);
        }
        return job;
    }

    public static IntPtr CreateReadyEvent(string name)
    {
        IntPtr ready = CreateEvent(IntPtr.Zero, true, false, name);
        if (ready == IntPtr.Zero) {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        return ready;
    }

    public static bool WaitReady(IntPtr ready, uint milliseconds)
    {
        return WaitForSingleObject(ready, milliseconds) == 0;
    }

    public static bool ContainsProcess(IntPtr job, IntPtr process)
    {
        bool result;
        if (!IsProcessInJob(process, job, out result)) {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        return result;
    }
}
'@
}

function Start-ProjLauncherRuntimeProcess {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Collections.IDictionary]$EnvironmentVariables = @{}
    )

    $StartInfo = [Diagnostics.ProcessStartInfo]::new()
    $StartInfo.FileName = $Executable
    $StartInfo.Arguments = $Arguments
    $StartInfo.WorkingDirectory = $WorkingDirectory
    $StartInfo.UseShellExecute = $false
    $StartInfo.CreateNoWindow = $true
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true
    $InheritedEnvironment = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    [void]$StartInfo.EnvironmentVariables
    $StartInfo.EnvironmentVariables.Clear()
    foreach ($Name in [string[]]@($InheritedEnvironment.Keys)) {
        $StartInfo.EnvironmentVariables[$Name] = [string]$InheritedEnvironment[$Name]
    }
    foreach ($Pair in $EnvironmentVariables.GetEnumerator()) {
        $StartInfo.EnvironmentVariables[[string]$Pair.Key] = [string]$Pair.Value
    }
    $Process = [Diagnostics.Process]::new()
    $Process.StartInfo = $StartInfo
    if (-not $Process.Start()) {
        $Process.Dispose()
        throw "Launcher process did not start: $Executable"
    }
    return $Process
}

function Invoke-ProjLauncherJobTestProcess {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [int]$TimeoutMilliseconds = 30000
    )

    $Identity = [Guid]::NewGuid().ToString('N')
    $JobName = "Local\SwawKit.Proj.Test.Job.$Identity"
    $ReadyName = "Local\SwawKit.Proj.Test.Ready.$Identity"
    $Job = [IntPtr]::Zero
    $Ready = [IntPtr]::Zero
    $Process = $null
    try {
        $Job = [ProjLauncherJobTest]::CreateKillOnCloseJob($JobName)
        $Ready = [ProjLauncherJobTest]::CreateReadyEvent($ReadyName)
        $Process = Start-ProjLauncherRuntimeProcess `
            -Executable $Executable `
            -Arguments $Arguments `
            -WorkingDirectory $WorkingDirectory `
            -EnvironmentVariables @{
                SWAWKIT_PROJ_CORE_LAUNCH_WORKER_PROTOCOL = '1'
                SWAWKIT_PROJ_CORE_LAUNCH_WORKER_JOB_NAME = $JobName
                SWAWKIT_PROJ_CORE_LAUNCH_WORKER_READY_EVENT_NAME = $ReadyName
            }
        $StandardOutput = $Process.StandardOutput.ReadToEndAsync()
        $StandardError = $Process.StandardError.ReadToEndAsync()
        if (-not [ProjLauncherJobTest]::WaitReady(
            $Ready,
            [uint32]$TimeoutMilliseconds
        )) {
            throw 'Launcher did not signal the Web worker ready event.'
        }
        $InJob = [ProjLauncherJobTest]::ContainsProcess(
            $Job,
            $Process.Handle
        )
        if (-not $Process.WaitForExit($TimeoutMilliseconds)) {
            throw 'Launcher Web worker process did not exit before timeout.'
        }
        return [pscustomobject][ordered]@{
            ExitCode = [int]$Process.ExitCode
            StandardOutput = [string]$StandardOutput.Result
            StandardError = [string]$StandardError.Result
            InJob = [bool]$InJob
        }
    } finally {
        if ($null -ne $Process) {
            if (-not $Process.HasExited) {
                if ([ProjLauncherJobTest]::CloseHandle($Job)) {
                    $Job = [IntPtr]::Zero
                }
                [void]$Process.WaitForExit(5000)
                if (-not $Process.HasExited) {
                    $Process.Kill()
                    [void]$Process.WaitForExit(5000)
                }
            }
            $Process.Dispose()
        }
        if ($Ready -ne [IntPtr]::Zero) {
            [void][ProjLauncherJobTest]::CloseHandle($Ready)
        }
        if ($Job -ne [IntPtr]::Zero) {
            [void][ProjLauncherJobTest]::CloseHandle($Job)
        }
    }
}
