Set-StrictMode -Version 2.0

function New-ProjConsoleCancelDriver {
    param([Parameter(Mandatory = $true)][string]$OutputAssembly)

    $OutputAssembly = [IO.Path]::GetFullPath($OutputAssembly)
    if ([IO.File]::Exists($OutputAssembly)) {
        throw "Console cancel driver already exists: $OutputAssembly"
    }
    Add-Type -OutputType ConsoleApplication -OutputAssembly $OutputAssembly `
        -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class ProjConsoleCancelDriver
{
    const uint CREATE_SUSPENDED = 0x00000004;
    const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;
    const uint STARTF_USESHOWWINDOW = 0x00000001;
    const short SW_HIDE = 0;
    const uint WAIT_OBJECT_0 = 0;
    const uint WAIT_TIMEOUT = 258;
    const uint STILL_ACTIVE = 259;
    const uint CTRL_C_EVENT = 0;
    const uint CTRL_BREAK_EVENT = 1;
    const uint ERROR_INVALID_HANDLE = 6;
    const int JobObjectBasicAccountingInformation = 1;
    const int JobObjectExtendedLimitInformation = 9;
    static readonly ConsoleHandler DriverConsoleHandler = IgnoreConsoleControl;

    public static int Main(string[] arguments)
    {
        if (arguments.Length != 3) return 64;
        string entry = Path.GetFullPath(arguments[0]);
        string workingDirectory = Path.GetFullPath(arguments[1]);
        string resultPath = Path.GetFullPath(arguments[2]);
        IntPtr job = IntPtr.Zero;
        PROCESS_INFORMATION child = new PROCESS_INFORMATION();
        try
        {
            CreateIsolatedConsole();
            job = CreateJobObjectW(IntPtr.Zero, null);
            if (job == IntPtr.Zero) throw LastError("create Job");
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits =
                new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if (!SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                ref limits,
                Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION))))
                throw LastError("configure Job");

            STARTUPINFO startup = new STARTUPINFO();
            startup.cb = Marshal.SizeOf(typeof(STARTUPINFO));
            startup.dwFlags = STARTF_USESHOWWINDOW;
            startup.wShowWindow = SW_HIDE;
            StringBuilder commandLine = new StringBuilder(
                "\"" + entry + "\" .dev.setup"
            );
            if (!CreateProcessW(
                entry,
                commandLine,
                IntPtr.Zero,
                IntPtr.Zero,
                false,
                CREATE_SUSPENDED,
                IntPtr.Zero,
                workingDirectory,
                ref startup,
                out child))
                throw LastError("start Entry");
            if (!AssignProcessToJobObject(job, child.hProcess))
                throw LastError("assign Entry to Job");
            if (ResumeThread(child.hThread) == UInt32.MaxValue)
                throw LastError("resume Entry");
            CloseHandle(child.hThread);
            child.hThread = IntPtr.Zero;

            uint observed = WaitForCommandTree(job, child.hProcess, 15000);
            uint consoleProcesses = SendConsoleCancellation();
            bool exited = WaitForSingleObject(child.hProcess, 15000) == WAIT_OBJECT_0;
            bool treeDrained = WaitForTreeDrain(job, 15000);
            uint exitCode = STILL_ACTIVE;
            if (!GetExitCodeProcess(child.hProcess, out exitCode))
                throw LastError("read Entry exit code");
            WriteResult(
                resultPath,
                observed,
                consoleProcesses,
                exited,
                treeDrained,
                exitCode,
                null
            );
            return exited && treeDrained ? 0 : 2;
        }
        catch (Exception error)
        {
            WriteResult(
                resultPath,
                0,
                0,
                false,
                false,
                STILL_ACTIVE,
                error.ToString()
            );
            return 1;
        }
        finally
        {
            if (child.hThread != IntPtr.Zero) CloseHandle(child.hThread);
            if (child.hProcess != IntPtr.Zero) CloseHandle(child.hProcess);
            if (job != IntPtr.Zero) CloseHandle(job);
        }
    }

    static uint WaitForCommandTree(IntPtr job, IntPtr process, int milliseconds)
    {
        DateTime deadline = DateTime.UtcNow.AddMilliseconds(milliseconds);
        uint observed = 0;
        while (DateTime.UtcNow < deadline)
        {
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION accounting;
            int returned;
            if (!QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                out accounting,
                Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)),
                out returned))
                throw LastError("inspect Entry Job");
            observed = Math.Max(observed, accounting.ActiveProcesses);
            if (observed >= 3) return observed;
            if (WaitForSingleObject(process, 0) == WAIT_OBJECT_0)
                throw new InvalidOperationException(
                    "Entry exited before Launcher, Core, and Toolchain were active"
                );
            Thread.Sleep(25);
        }
        throw new TimeoutException(
            "Launcher, Core, and Toolchain did not become active"
        );
    }

    static bool WaitForTreeDrain(IntPtr job, int milliseconds)
    {
        DateTime deadline = DateTime.UtcNow.AddMilliseconds(milliseconds);
        while (DateTime.UtcNow < deadline)
        {
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION accounting;
            int returned;
            if (!QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                out accounting,
                Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)),
                out returned))
                throw LastError("inspect canceled Entry Job");
            if (accounting.ActiveProcesses == 0) return true;
            Thread.Sleep(25);
        }
        return false;
    }

    static void CreateIsolatedConsole()
    {
        if (!FreeConsole() && Marshal.GetLastWin32Error() != ERROR_INVALID_HANDLE)
            throw LastError("detach driver console");
        if (!AllocConsole()) throw LastError("allocate isolated test console");
        IntPtr window = GetConsoleWindow();
        if (window != IntPtr.Zero) ShowWindow(window, SW_HIDE);
    }

    static uint SendConsoleCancellation()
    {
        uint[] processes = new uint[16];
        uint count = GetConsoleProcessList(processes, (uint)processes.Length);
        if (count == 0) throw LastError("inspect isolated console processes");
        if (!SetConsoleCtrlHandler(DriverConsoleHandler, true))
            throw LastError("ignore Ctrl+C in driver");
        // Headless Windows consoles do not reliably dispatch a synthetic
        // CTRL_C_EVENT. CTRL_BREAK_EVENT is the targetable automation signal;
        // Launcher and Core intentionally map both controls to one contract.
        if (!GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, 0))
            throw LastError("send console break to isolated Entry console");
        Thread.Sleep(100);
        return count;
    }

    static bool IgnoreConsoleControl(uint controlType)
    {
        return controlType == CTRL_C_EVENT || controlType == CTRL_BREAK_EVENT;
    }

    static void WriteResult(
        string path,
        uint observed,
        uint consoleProcesses,
        bool exited,
        bool treeDrained,
        uint exitCode,
        string error)
    {
        string escaped = error == null ? "null" : "\"" + error
            .Replace("\\", "\\\\")
            .Replace("\"", "\\\"")
            .Replace("\r", "\\r")
            .Replace("\n", "\\n") + "\"";
        File.WriteAllText(
            path,
            "{\"observedProcesses\":" + observed +
            ",\"consoleProcesses\":" + consoleProcesses +
            ",\"exited\":" + (exited ? "true" : "false") +
            ",\"treeDrained\":" + (treeDrained ? "true" : "false") +
            ",\"exitCode\":" + exitCode +
            ",\"error\":" + escaped + "}\n",
            new UTF8Encoding(false)
        );
    }

    static Win32Exception LastError(string operation)
    {
        return new Win32Exception(Marshal.GetLastWin32Error(), operation);
    }

    [StructLayout(LayoutKind.Sequential)]
    struct IO_COUNTERS
    {
        public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
        public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct JOBOBJECT_BASIC_LIMIT_INFORMATION
    {
        public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass, SchedulingClass;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION
    {
        public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
        public IO_COUNTERS IoInfo;
        public UIntPtr ProcessMemoryLimit, JobMemoryLimit;
        public UIntPtr PeakProcessMemoryUsed, PeakJobMemoryUsed;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION
    {
        public long TotalUserTime, TotalKernelTime;
        public long ThisPeriodTotalUserTime, ThisPeriodTotalKernelTime;
        public uint TotalPageFaultCount, TotalProcesses, ActiveProcesses;
        public uint TotalTerminatedProcesses;
    }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct STARTUPINFO
    {
        public int cb;
        public string lpReserved, lpDesktop, lpTitle;
        public uint dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars;
        public uint dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2;
        public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct PROCESS_INFORMATION
    {
        public IntPtr hProcess, hThread;
        public uint dwProcessId, dwThreadId;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool SetInformationJobObject(
        IntPtr job, int informationClass,
        ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION information, int length);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool QueryInformationJobObject(
        IntPtr job, int informationClass,
        out JOBOBJECT_BASIC_ACCOUNTING_INFORMATION information,
        int length, out int returned);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CreateProcessW(
        string applicationName, StringBuilder commandLine,
        IntPtr processAttributes, IntPtr threadAttributes, bool inheritHandles,
        uint creationFlags, IntPtr environment, string currentDirectory,
        ref STARTUPINFO startupInfo, out PROCESS_INFORMATION processInformation);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool FreeConsole();
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool AllocConsole();
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint GetConsoleProcessList(uint[] processList, uint processCount);
    [DllImport("kernel32.dll")]
    static extern IntPtr GetConsoleWindow();
    [DllImport("user32.dll")]
    static extern bool ShowWindow(IntPtr window, int command);
    [UnmanagedFunctionPointer(CallingConvention.Winapi)]
    delegate bool ConsoleHandler(uint controlType);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool SetConsoleCtrlHandler(ConsoleHandler handler, bool add);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool GenerateConsoleCtrlEvent(uint controlEvent, uint processGroupId);
}
'@
    return $OutputAssembly
}
