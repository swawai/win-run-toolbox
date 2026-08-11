Set-StrictMode -Version 2.0

if ($null -eq ('SwawKit.Proj.Tests.OwnedProcessTree' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

namespace SwawKit.Proj.Tests
{
    public sealed class OwnedProcessTree : IDisposable
    {
        private const uint CREATE_SUSPENDED = 0x00000004;
        private const uint CREATE_NO_WINDOW = 0x08000000;
        private const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;
        private const int JobObjectBasicAccountingInformation = 1;
        private const int JobObjectExtendedLimitInformation = 9;
        private const uint WAIT_OBJECT_0 = 0;
        private const uint WAIT_TIMEOUT = 258;
        private const uint STILL_ACTIVE = 259;

        private IntPtr job;
        private IntPtr process;

        private OwnedProcessTree(IntPtr job, IntPtr process, int processId)
        {
            this.job = job;
            this.process = process;
            ProcessId = processId;
        }

        public int ProcessId { get; private set; }

        public int ExitCode
        {
            get
            {
                uint code;
                if (process == IntPtr.Zero || !GetExitCodeProcess(process, out code))
                {
                    throw LastError("cannot read the owned process exit code");
                }
                if (code == STILL_ACTIVE)
                {
                    throw new InvalidOperationException("the owned process is still running");
                }
                return unchecked((int)code);
            }
        }

        public uint TotalProcesses
        {
            get
            {
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION information;
                int length;
                if (job == IntPtr.Zero || !QueryInformationJobObject(
                    job,
                    JobObjectBasicAccountingInformation,
                    out information,
                    Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)),
                    out length))
                {
                    throw LastError("cannot inspect the owned process tree");
                }
                return information.TotalProcesses;
            }
        }

        public static OwnedProcessTree Start(string executable, string workingDirectory)
        {
            if (String.IsNullOrWhiteSpace(executable) ||
                String.IsNullOrWhiteSpace(workingDirectory))
            {
                throw new ArgumentException("owned process paths cannot be empty");
            }

            IntPtr job = IntPtr.Zero;
            PROCESS_INFORMATION processInformation = new PROCESS_INFORMATION();
            try
            {
                job = CreateJobObjectW(IntPtr.Zero, null);
                if (job == IntPtr.Zero)
                {
                    throw LastError("cannot create the owned process Job");
                }

                JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits =
                    new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
                limits.BasicLimitInformation.LimitFlags =
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if (!SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    ref limits,
                    Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION))))
                {
                    throw LastError("cannot configure the owned process Job");
                }

                STARTUPINFO startup = new STARTUPINFO();
                startup.cb = Marshal.SizeOf(typeof(STARTUPINFO));
                StringBuilder commandLine = new StringBuilder(
                    "\"" + executable + "\""
                );
                if (!CreateProcessW(
                    executable,
                    commandLine,
                    IntPtr.Zero,
                    IntPtr.Zero,
                    false,
                    CREATE_SUSPENDED | CREATE_NO_WINDOW,
                    IntPtr.Zero,
                    workingDirectory,
                    ref startup,
                    out processInformation))
                {
                    throw LastError("cannot start the owned process");
                }
                if (!AssignProcessToJobObject(job, processInformation.hProcess))
                {
                    throw LastError("cannot assign the owned process to its Job");
                }
                if (ResumeThread(processInformation.hThread) == UInt32.MaxValue)
                {
                    throw LastError("cannot resume the owned process");
                }
                CloseHandle(processInformation.hThread);
                processInformation.hThread = IntPtr.Zero;
                return new OwnedProcessTree(
                    job,
                    processInformation.hProcess,
                    unchecked((int)processInformation.dwProcessId)
                );
            }
            catch
            {
                if (processInformation.hProcess != IntPtr.Zero)
                {
                    TerminateProcess(processInformation.hProcess, 1);
                }
                if (processInformation.hThread != IntPtr.Zero)
                {
                    CloseHandle(processInformation.hThread);
                }
                if (processInformation.hProcess != IntPtr.Zero)
                {
                    CloseHandle(processInformation.hProcess);
                }
                if (job != IntPtr.Zero)
                {
                    CloseHandle(job);
                }
                throw;
            }
        }

        public bool WaitForExit(int milliseconds)
        {
            if (milliseconds < 0)
            {
                throw new ArgumentOutOfRangeException("milliseconds");
            }
            uint result = WaitForSingleObject(process, unchecked((uint)milliseconds));
            if (result == WAIT_OBJECT_0)
            {
                return true;
            }
            if (result == WAIT_TIMEOUT)
            {
                return false;
            }
            throw LastError("cannot wait for the owned process");
        }

        public void Dispose()
        {
            Dispose(true);
            GC.SuppressFinalize(this);
        }

        ~OwnedProcessTree()
        {
            Dispose(false);
        }

        private void Dispose(bool disposing)
        {
            if (job != IntPtr.Zero)
            {
                CloseHandle(job);
                job = IntPtr.Zero;
            }
            if (process != IntPtr.Zero)
            {
                WaitForSingleObject(process, 5000);
                CloseHandle(process);
                process = IntPtr.Zero;
            }
        }

        private static Win32Exception LastError(string message)
        {
            return new Win32Exception(Marshal.GetLastWin32Error(), message);
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct IO_COUNTERS
        {
            public ulong ReadOperationCount;
            public ulong WriteOperationCount;
            public ulong OtherOperationCount;
            public ulong ReadTransferCount;
            public ulong WriteTransferCount;
            public ulong OtherTransferCount;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION
        {
            public long TotalUserTime;
            public long TotalKernelTime;
            public long ThisPeriodTotalUserTime;
            public long ThisPeriodTotalKernelTime;
            public uint TotalPageFaultCount;
            public uint TotalProcesses;
            public uint ActiveProcesses;
            public uint TotalTerminatedProcesses;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct JOBOBJECT_BASIC_LIMIT_INFORMATION
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
        private struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION
        {
            public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
            public IO_COUNTERS IoInfo;
            public UIntPtr ProcessMemoryLimit;
            public UIntPtr JobMemoryLimit;
            public UIntPtr PeakProcessMemoryUsed;
            public UIntPtr PeakJobMemoryUsed;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct STARTUPINFO
        {
            public int cb;
            public string lpReserved;
            public string lpDesktop;
            public string lpTitle;
            public uint dwX;
            public uint dwY;
            public uint dwXSize;
            public uint dwYSize;
            public uint dwXCountChars;
            public uint dwYCountChars;
            public uint dwFillAttribute;
            public uint dwFlags;
            public short wShowWindow;
            public short cbReserved2;
            public IntPtr lpReserved2;
            public IntPtr hStdInput;
            public IntPtr hStdOutput;
            public IntPtr hStdError;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct PROCESS_INFORMATION
        {
            public IntPtr hProcess;
            public IntPtr hThread;
            public uint dwProcessId;
            public uint dwThreadId;
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool SetInformationJobObject(
            IntPtr job,
            int informationClass,
            ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION information,
            int informationLength
        );

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool QueryInformationJobObject(
            IntPtr job,
            int informationClass,
            out JOBOBJECT_BASIC_ACCOUNTING_INFORMATION information,
            int informationLength,
            out int returnLength
        );

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool CreateProcessW(
            string applicationName,
            StringBuilder commandLine,
            IntPtr processAttributes,
            IntPtr threadAttributes,
            bool inheritHandles,
            uint creationFlags,
            IntPtr environment,
            string currentDirectory,
            ref STARTUPINFO startupInfo,
            out PROCESS_INFORMATION processInformation
        );

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint ResumeThread(IntPtr thread);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool TerminateProcess(IntPtr process, uint exitCode);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool CloseHandle(IntPtr handle);
    }
}
'@
}

function Start-ProjOwnedProcessTree {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory
    )

    $FilePath = [IO.Path]::GetFullPath($FilePath)
    $WorkingDirectory = [IO.Path]::GetFullPath($WorkingDirectory)
    if (-not [IO.File]::Exists($FilePath)) {
        throw "Owned process executable is missing: $FilePath"
    }
    if (-not [IO.Directory]::Exists($WorkingDirectory)) {
        throw "Owned process working directory is missing: $WorkingDirectory"
    }
    return [SwawKit.Proj.Tests.OwnedProcessTree]::Start(
        $FilePath,
        $WorkingDirectory
    )
}
