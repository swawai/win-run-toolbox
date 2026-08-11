[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$RequestBase64,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ResultPath,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ProcessIdentityPath
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Set-StrictMode -Version 2.0

$Utf8 = New-Object Text.UTF8Encoding($false)
[Console]::InputEncoding = $Utf8
[Console]::OutputEncoding = $Utf8
$OutputEncoding = $Utf8

$NativeSource = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

namespace SwawKit.RdpClient
{
    public static class DesktopNative
    {
        private const int UOI_NAME = 2;
        private const uint DESKTOP_READOBJECTS = 0x0001;
        private const uint DESKTOP_SWITCHDESKTOP = 0x0100;
        private const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
        private const uint MOUSEEVENTF_LEFTUP = 0x0004;
        private const int WTS_CURRENT_SERVER_HANDLE = 0;
        private const int WTS_USER_NAME = 5;
        private const int WTS_DOMAIN_NAME = 7;

        [DllImport("user32.dll", SetLastError = true)]
        private static extern IntPtr OpenInputDesktop(
            uint flags,
            bool inherit,
            uint desiredAccess
        );

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool CloseDesktop(IntPtr desktop);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool GetUserObjectInformation(
            IntPtr handle,
            int index,
            StringBuilder information,
            int length,
            out int needed
        );

        [DllImport("user32.dll")]
        private static extern int GetSystemMetrics(int index);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool SetCursorPos(int x, int y);

        [DllImport("user32.dll")]
        private static extern void mouse_event(
            uint flags,
            uint dx,
            uint dy,
            uint data,
            UIntPtr extraInfo
        );

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool SetProcessDPIAware();

        [DllImport("kernel32.dll")]
        private static extern uint GetCurrentProcessId();

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool ProcessIdToSessionId(
            uint processId,
            out uint sessionId
        );

        [DllImport(
            "wtsapi32.dll",
            EntryPoint = "WTSQuerySessionInformationW",
            CharSet = CharSet.Unicode,
            SetLastError = true
        )]
        private static extern bool WTSQuerySessionInformation(
            IntPtr server,
            int sessionId,
            int informationClass,
            out IntPtr buffer,
            out int bytesReturned
        );

        [DllImport("wtsapi32.dll")]
        private static extern void WTSFreeMemory(IntPtr buffer);

        [DllImport("user32.dll", EntryPoint = "SetProcessDpiAwarenessContext")]
        private static extern bool SetProcessDpiAwarenessContext(
            IntPtr value
        );

        public static void EnableDpiAwareness()
        {
            try
            {
                if (SetProcessDpiAwarenessContext(new IntPtr(-4)))
                {
                    return;
                }
            }
            catch (EntryPointNotFoundException)
            {
            }
            SetProcessDPIAware();
        }

        public static string GetInputDesktopName()
        {
            IntPtr desktop = OpenInputDesktop(
                0,
                false,
                DESKTOP_READOBJECTS | DESKTOP_SWITCHDESKTOP
            );
            if (desktop == IntPtr.Zero)
            {
                throw new Win32Exception(
                    Marshal.GetLastWin32Error(),
                    "OpenInputDesktop failed."
                );
            }
            try
            {
                int needed;
                GetUserObjectInformation(
                    desktop,
                    UOI_NAME,
                    null,
                    0,
                    out needed
                );
                StringBuilder name = new StringBuilder(
                    Math.Max(needed / 2, 32)
                );
                if (!GetUserObjectInformation(
                    desktop,
                    UOI_NAME,
                    name,
                    name.Capacity * 2,
                    out needed
                ))
                {
                    throw new Win32Exception(
                        Marshal.GetLastWin32Error(),
                        "GetUserObjectInformation failed."
                    );
                }
                return name.ToString();
            }
            finally
            {
                CloseDesktop(desktop);
            }
        }

        public static int Metric(int index)
        {
            return GetSystemMetrics(index);
        }

        public static int GetCurrentSessionId()
        {
            uint sessionId;
            if (!ProcessIdToSessionId(GetCurrentProcessId(), out sessionId))
            {
                throw new Win32Exception(
                    Marshal.GetLastWin32Error(),
                    "ProcessIdToSessionId failed."
                );
            }
            return checked((int)sessionId);
        }

        private static string QuerySessionString(
            int sessionId,
            int informationClass
        )
        {
            IntPtr buffer;
            int bytesReturned;
            if (!WTSQuerySessionInformation(
                new IntPtr(WTS_CURRENT_SERVER_HANDLE),
                sessionId,
                informationClass,
                out buffer,
                out bytesReturned
            ))
            {
                throw new Win32Exception(
                    Marshal.GetLastWin32Error(),
                    "WTSQuerySessionInformation failed."
                );
            }
            try
            {
                return buffer == IntPtr.Zero
                    ? String.Empty
                    : Marshal.PtrToStringUni(buffer) ?? String.Empty;
            }
            finally
            {
                if (buffer != IntPtr.Zero)
                {
                    WTSFreeMemory(buffer);
                }
            }
        }

        public static string GetSessionUserName(int sessionId)
        {
            return QuerySessionString(sessionId, WTS_USER_NAME);
        }

        public static string GetSessionDomainName(int sessionId)
        {
            return QuerySessionString(sessionId, WTS_DOMAIN_NAME);
        }

        public static void LeftClick(int x, int y)
        {
            if (!SetCursorPos(x, y))
            {
                throw new Win32Exception(
                    Marshal.GetLastWin32Error(),
                    "SetCursorPos failed."
                );
            }
            mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, UIntPtr.Zero);
            mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, UIntPtr.Zero);
        }
    }
}
'@

function Write-RdpClientDesktopResult {
    param(
        [Parameter(Mandatory = $true)][Collections.IDictionary]$Result,
        [Parameter(Mandatory = $true)][Text.Encoding]$Encoding,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $Json = ConvertTo-Json -InputObject $Result -Compress -Depth 4
    $Payload = [Convert]::ToBase64String($Encoding.GetBytes($Json))
    $Bytes = $Encoding.GetBytes('RDP_CLIENT_DESKTOP_RESULT_V1:' + $Payload)
    $Stream = [IO.File]::Open(
        [IO.Path]::GetFullPath($Path),
        [IO.FileMode]::CreateNew,
        [IO.FileAccess]::Write,
        [IO.FileShare]::None
    )
    try {
        $Stream.Write($Bytes, 0, $Bytes.Length)
    } finally {
        $Stream.Dispose()
    }
}

function Assert-RdpClientDesktopCoordinate {
    param(
        [Parameter(Mandatory = $true)][int]$X,
        [Parameter(Mandatory = $true)][int]$Y,
        [Parameter(Mandatory = $true)][int]$Width,
        [Parameter(Mandatory = $true)][int]$Height
    )

    if ($X -lt 0 -or $Y -lt 0 -or $X -ge $Width -or $Y -ge $Height) {
        throw (
            '[COORDINATE_OUT_OF_RANGE] Coordinate ' +
            "($X,$Y) is outside the $Width x $Height desktop."
        )
    }
}

$Action = ''
try {
    $CurrentProcess = [Diagnostics.Process]::GetCurrentProcess()
    $ProcessIdentityJson = ConvertTo-Json -InputObject ([ordered]@{
        Version           = 1
        ProcessId         = [int]$CurrentProcess.Id
        StartTimeUtcTicks = [int64]$CurrentProcess.StartTime.ToUniversalTime().Ticks
    }) -Compress
    $ProcessIdentityBytes = $Utf8.GetBytes($ProcessIdentityJson)
    $ProcessIdentityStream = [IO.File]::Open(
        [IO.Path]::GetFullPath($ProcessIdentityPath),
        [IO.FileMode]::CreateNew,
        [IO.FileAccess]::Write,
        [IO.FileShare]::None
    )
    try {
        $ProcessIdentityStream.Write(
            $ProcessIdentityBytes,
            0,
            $ProcessIdentityBytes.Length
        )
    } finally {
        $ProcessIdentityStream.Dispose()
        $CurrentProcess.Dispose()
    }
    if ([string]::IsNullOrWhiteSpace($RequestBase64)) {
        throw '[INVALID_REQUEST] The desktop task request was not provided.'
    }
    $RequestJson = $Utf8.GetString(
        [Convert]::FromBase64String($RequestBase64)
    )
    $Request = $RequestJson | ConvertFrom-Json
    $Action = [string]$Request.Action
    if (@('screenshot', 'pixel', 'click') -notcontains $Action) {
        throw "[INVALID_REQUEST] Unsupported desktop action: $Action"
    }

    Add-Type -AssemblyName System.Drawing
    Add-Type -TypeDefinition $NativeSource -Language CSharp
    [SwawKit.RdpClient.DesktopNative]::EnableDpiAwareness()

    $ExpectedSessionId = [int]0
    if (-not [int]::TryParse(
        [string]$Request.SessionId,
        [ref]$ExpectedSessionId
    ) -or $ExpectedSessionId -le 0) {
        throw '[INVALID_REQUEST] The expected desktop session ID is invalid.'
    }
    $CurrentSessionId = [SwawKit.RdpClient.DesktopNative]::GetCurrentSessionId()
    if ($CurrentSessionId -ne $ExpectedSessionId) {
        throw (
            '[SESSION_CHANGED] The desktop task started in session ' +
            "$CurrentSessionId, not expected session $ExpectedSessionId."
        )
    }
    $ExpectedUserName = [string]$Request.ExpectedUserName
    $ExpectedDomainName = [string]$Request.ExpectedDomainName
    $ActualUserName = [SwawKit.RdpClient.DesktopNative]::GetSessionUserName(
        $CurrentSessionId
    )
    $ActualDomainName = [SwawKit.RdpClient.DesktopNative]::GetSessionDomainName(
        $CurrentSessionId
    )
    if ([string]::IsNullOrWhiteSpace($ExpectedUserName) -or
        -not [string]::Equals(
            $ActualUserName,
            $ExpectedUserName,
            [StringComparison]::OrdinalIgnoreCase
        ) -or
        -not [string]::Equals(
            $ActualDomainName,
            $ExpectedDomainName,
            [StringComparison]::OrdinalIgnoreCase
        )) {
        throw (
            '[SESSION_CHANGED] Session identity changed before the desktop ' +
            "task ran. Expected $ExpectedDomainName\$ExpectedUserName; " +
            "found $ActualDomainName\$ActualUserName."
        )
    }

    $DesktopName = [SwawKit.RdpClient.DesktopNative]::GetInputDesktopName()
    if (-not [string]::Equals(
        $DesktopName,
        'Default',
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw (
            '[DESKTOP_NOT_INTERACTIVE] The input desktop is ' +
            "$DesktopName, not Default. The session may be locked or showing " +
            'a secure desktop.'
        )
    }

    $OriginX = [SwawKit.RdpClient.DesktopNative]::Metric(76)
    $OriginY = [SwawKit.RdpClient.DesktopNative]::Metric(77)
    $Width = [SwawKit.RdpClient.DesktopNative]::Metric(78)
    $Height = [SwawKit.RdpClient.DesktopNative]::Metric(79)
    if ($Width -le 0 -or $Height -le 0) {
        throw '[DISPLAY_NOT_READY] Windows reported no capturable virtual screen.'
    }

    $Result = [ordered]@{
        Version     = 1
        Success     = $true
        Action      = $Action
        SessionId   = $CurrentSessionId
        UserName    = $ActualUserName
        DomainName  = $ActualDomainName
        DesktopName = $DesktopName
        OriginX     = $OriginX
        OriginY     = $OriginY
        Width       = $Width
        Height      = $Height
    }

    if ($Action -eq 'screenshot') {
        $Bitmap = New-Object Drawing.Bitmap(
            $Width,
            $Height,
            [Drawing.Imaging.PixelFormat]::Format32bppArgb
        )
        $Graphics = [Drawing.Graphics]::FromImage($Bitmap)
        $Stream = New-Object IO.MemoryStream
        try {
            $Graphics.CopyFromScreen(
                $OriginX,
                $OriginY,
                0,
                0,
                $Bitmap.Size,
                [Drawing.CopyPixelOperation]::SourceCopy
            )
            $Bitmap.Save($Stream, [Drawing.Imaging.ImageFormat]::Png)
            $Result.ImageBase64 = [Convert]::ToBase64String($Stream.ToArray())
        } finally {
            $Stream.Dispose()
            $Graphics.Dispose()
            $Bitmap.Dispose()
        }
    } else {
        $X = [int]$Request.X
        $Y = [int]$Request.Y
        Assert-RdpClientDesktopCoordinate `
            -X $X `
            -Y $Y `
            -Width $Width `
            -Height $Height
        $Result.X = $X
        $Result.Y = $Y

        if ($Action -eq 'pixel') {
            $Bitmap = New-Object Drawing.Bitmap(1, 1)
            $Graphics = [Drawing.Graphics]::FromImage($Bitmap)
            try {
                $Graphics.CopyFromScreen(
                    $OriginX + $X,
                    $OriginY + $Y,
                    0,
                    0,
                    $Bitmap.Size,
                    [Drawing.CopyPixelOperation]::SourceCopy
                )
                $Color = $Bitmap.GetPixel(0, 0)
                $Result.Color = '#{0:X2}{1:X2}{2:X2}' -f `
                    $Color.R,
                    $Color.G,
                    $Color.B
            } finally {
                $Graphics.Dispose()
                $Bitmap.Dispose()
            }
        } else {
            [SwawKit.RdpClient.DesktopNative]::LeftClick(
                $OriginX + $X,
                $OriginY + $Y
            )
        }
    }

    Write-RdpClientDesktopResult `
        -Result $Result `
        -Encoding $Utf8 `
        -Path $ResultPath
    exit 0
} catch {
    $Failure = $_.Exception
    while ($null -ne $Failure.InnerException) {
        $Failure = $Failure.InnerException
    }
    $Message = [string]$Failure.Message
    $ErrorCode = 'DESKTOP_TASK_FAILED'
    if ($Message -match '^\[(?<Code>[A-Z0-9_]+)\]\s*(?<Detail>.*)$') {
        $ErrorCode = $Matches.Code
        $Message = $Matches.Detail
    }
    Write-RdpClientDesktopResult -Result ([ordered]@{
        Version   = 1
        Success   = $false
        Action    = $Action
        ErrorCode = $ErrorCode
        Error     = $Message
    }) -Encoding $Utf8 -Path $ResultPath
    exit 1
}
