$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$NativeSource = @'
using System;
using System.Runtime.InteropServices;

namespace SwawKit.RdpClient
{
    internal static class SessionNative
    {
        internal enum WtsConnectState
        {
            Active = 0,
            Connected = 1,
            ConnectQuery = 2,
            Shadow = 3,
            Disconnected = 4,
            Idle = 5,
            Listen = 6,
            Reset = 7,
            Down = 8,
            Init = 9
        }

        internal enum WtsInfoClass
        {
            InitialProgram = 0,
            ApplicationName = 1,
            WorkingDirectory = 2,
            OemId = 3,
            SessionId = 4,
            UserName = 5,
            WinStationName = 6,
            DomainName = 7,
            ConnectState = 8,
            ClientBuildNumber = 9,
            ClientName = 10,
            ClientDirectory = 11,
            ClientProductId = 12,
            ClientHardwareId = 13,
            ClientAddress = 14,
            ClientDisplay = 15,
            ClientProtocolType = 16,
            IdleTime = 17,
            LogonTime = 18,
            IncomingBytes = 19,
            OutgoingBytes = 20,
            IncomingFrames = 21,
            OutgoingFrames = 22,
            ClientInfo = 23,
            SessionInfo = 24,
            SessionInfoEx = 25
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct WtsSessionInfo
        {
            internal int SessionId;
            internal IntPtr WinStationName;
            internal WtsConnectState State;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        internal struct WtsInfoExLevel1
        {
            internal uint SessionId;
            internal WtsConnectState SessionState;
            internal int SessionFlags;

            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 33)]
            internal string WinStationName;

            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 21)]
            internal string UserName;

            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 18)]
            internal string DomainName;

            internal long LogonTime;
            internal long ConnectTime;
            internal long DisconnectTime;
            internal long LastInputTime;
            internal long CurrentTime;
            internal uint IncomingBytes;
            internal uint OutgoingBytes;
            internal uint IncomingFrames;
            internal uint OutgoingFrames;
            internal uint IncomingCompressedBytes;
            internal uint OutgoingCompressedBytes;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct WtsInfoEx
        {
            internal uint Level;
            internal WtsInfoExLevel1 Data;
        }

        [DllImport("wtsapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool WTSEnumerateSessions(
            IntPtr server,
            int reserved,
            int version,
            out IntPtr sessionInfo,
            out int count);

        [DllImport("wtsapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool WTSQuerySessionInformation(
            IntPtr server,
            int sessionId,
            WtsInfoClass infoClass,
            out IntPtr buffer,
            out int bytesReturned);

        [DllImport("wtsapi32.dll")]
        internal static extern void WTSFreeMemory(IntPtr memory);

        [DllImport("kernel32.dll")]
        internal static extern uint WTSGetActiveConsoleSessionId();

        internal static string QueryString(int sessionId, WtsInfoClass infoClass)
        {
            IntPtr buffer;
            int bytesReturned;
            if (!WTSQuerySessionInformation(
                IntPtr.Zero,
                sessionId,
                infoClass,
                out buffer,
                out bytesReturned))
            {
                return String.Empty;
            }

            try
            {
                if (buffer == IntPtr.Zero || bytesReturned <= 2)
                {
                    return String.Empty;
                }
                return Marshal.PtrToStringUni(buffer) ?? String.Empty;
            }
            finally
            {
                WTSFreeMemory(buffer);
            }
        }

        internal static int QueryProtocolType(int sessionId)
        {
            IntPtr buffer;
            int bytesReturned;
            if (!WTSQuerySessionInformation(
                IntPtr.Zero,
                sessionId,
                WtsInfoClass.ClientProtocolType,
                out buffer,
                out bytesReturned))
            {
                return -1;
            }

            try
            {
                if (buffer == IntPtr.Zero || bytesReturned < 2)
                {
                    return -1;
                }
                return Marshal.ReadInt16(buffer);
            }
            finally
            {
                WTSFreeMemory(buffer);
            }
        }

        internal static int QuerySessionFlags(int sessionId)
        {
            IntPtr buffer;
            int bytesReturned;
            if (!WTSQuerySessionInformation(
                IntPtr.Zero,
                sessionId,
                WtsInfoClass.SessionInfoEx,
                out buffer,
                out bytesReturned))
            {
                return -1;
            }

            try
            {
                if (buffer == IntPtr.Zero ||
                    bytesReturned < Marshal.SizeOf(typeof(WtsInfoEx)))
                {
                    return -1;
                }
                WtsInfoEx info = (WtsInfoEx)Marshal.PtrToStructure(
                    buffer,
                    typeof(WtsInfoEx));
                if (info.Level != 1)
                {
                    return -1;
                }
                return info.Data.SessionFlags;
            }
            finally
            {
                WTSFreeMemory(buffer);
            }
        }
    }

    public sealed class SessionSnapshot
    {
        public int Id { get; set; }
        public string UserName { get; set; }
        public string DomainName { get; set; }
        public string SessionName { get; set; }
        public string State { get; set; }
        public string ClientName { get; set; }
        public int ProtocolType { get; set; }
        public bool IsConsole { get; set; }
        public int SessionFlags { get; set; }
    }

    public static class SessionQuery
    {
        public static uint GetActiveConsoleSessionId()
        {
            return SessionNative.WTSGetActiveConsoleSessionId();
        }

        public static SessionSnapshot[] GetSessions()
        {
            IntPtr buffer;
            int count;
            if (!SessionNative.WTSEnumerateSessions(
                IntPtr.Zero,
                0,
                1,
                out buffer,
                out count))
            {
                throw new System.ComponentModel.Win32Exception(
                    Marshal.GetLastWin32Error(),
                    "WTSEnumerateSessions failed.");
            }

            try
            {
                uint consoleId = SessionNative.WTSGetActiveConsoleSessionId();
                int size = Marshal.SizeOf(typeof(SessionNative.WtsSessionInfo));
                SessionSnapshot[] sessions = new SessionSnapshot[count];
                long address = buffer.ToInt64();
                for (int index = 0; index < count; index++)
                {
                    SessionNative.WtsSessionInfo native =
                        (SessionNative.WtsSessionInfo)Marshal.PtrToStructure(
                            new IntPtr(address + (index * size)),
                            typeof(SessionNative.WtsSessionInfo));
                    sessions[index] = new SessionSnapshot
                    {
                        Id = native.SessionId,
                        UserName = SessionNative.QueryString(
                            native.SessionId,
                            SessionNative.WtsInfoClass.UserName),
                        DomainName = SessionNative.QueryString(
                            native.SessionId,
                            SessionNative.WtsInfoClass.DomainName),
                        SessionName = Marshal.PtrToStringUni(
                            native.WinStationName) ?? String.Empty,
                        State = native.State.ToString(),
                        ClientName = SessionNative.QueryString(
                            native.SessionId,
                            SessionNative.WtsInfoClass.ClientName),
                        ProtocolType = SessionNative.QueryProtocolType(
                            native.SessionId),
                        IsConsole = consoleId != UInt32.MaxValue &&
                            native.SessionId == consoleId,
                        SessionFlags = SessionNative.QuerySessionFlags(
                            native.SessionId)
                    };
                }
                return sessions;
            }
            finally
            {
                SessionNative.WTSFreeMemory(buffer);
            }
        }
    }
}
'@

if (-not ('SwawKit.RdpClient.SessionQuery' -as [type])) {
    Add-Type -TypeDefinition $NativeSource -Language CSharp
}

$ConsoleSessionId = [SwawKit.RdpClient.SessionQuery]::GetActiveConsoleSessionId()
$Sessions = @(
    foreach ($Session in [SwawKit.RdpClient.SessionQuery]::GetSessions()) {
        if ([string]::IsNullOrWhiteSpace($Session.UserName) -and
            -not $Session.IsConsole) {
            continue
        }

        $Locked = $null
        if ($Session.SessionFlags -eq 0) {
            $Locked = $true
        } elseif ($Session.SessionFlags -eq 1) {
            $Locked = $false
        }

        $Terminal = if ($Session.IsConsole) {
            'console'
        } elseif ($Session.ProtocolType -eq 2) {
            'rdp'
        } elseif ($Session.State -eq 'Disconnected') {
            'detached'
        } else {
            'other'
        }

        [ordered]@{
            Id          = $Session.Id
            UserName    = $Session.UserName
            DomainName  = $Session.DomainName
            SessionName = $Session.SessionName
            State       = $Session.State
            Locked      = $Locked
            Terminal    = $Terminal
            IsConsole   = $Session.IsConsole
            ClientName  = $Session.ClientName
        }
    }
)

$PolicyPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
$SystemPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server'
$SingleSessionPerUser = $null
foreach ($Path in @($PolicyPath, $SystemPath)) {
    try {
        $SingleSessionPerUser = [int](Get-ItemPropertyValue `
            -Path $Path `
            -Name 'fSingleSessionPerUser' `
            -ErrorAction Stop)
        break
    } catch {
    }
}

$State = [ordered]@{
    Version              = 1
    ComputerName         = $env:COMPUTERNAME
    ConsoleSessionId     = [uint64]$ConsoleSessionId
    SingleSessionPerUser = $SingleSessionPerUser
    Sessions             = $Sessions
}
$Json = ConvertTo-Json -InputObject $State -Depth 5 -Compress
$Payload = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Json))
Write-Output ('RDP_CLIENT_SESSION_STATE_V1:' + $Payload)
