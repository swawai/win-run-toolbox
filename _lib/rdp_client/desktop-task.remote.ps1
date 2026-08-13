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

function Resolve-RdpClientDesktopRequestInteger {
    param(
        [Parameter(Mandatory = $true)]$RequestObject,
        [Parameter(Mandatory = $true)][string]$Name,
        [int]$Minimum = 0,
        [int]$Maximum = [int]::MaxValue
    )

    $Property = $RequestObject.PSObject.Properties[$Name]
    $Result = [int]0
    if ($null -eq $Property -or
        -not [int]::TryParse([string]$Property.Value, [ref]$Result) -or
        $Result -lt $Minimum -or $Result -gt $Maximum) {
        throw (
            "[INVALID_REQUEST] $Name must be between $Minimum and $Maximum."
        )
    }
    return $Result
}

function Get-RdpClientDesktopScreenshotBase64 {
    param(
        [Parameter(Mandatory = $true)][int]$OriginX,
        [Parameter(Mandatory = $true)][int]$OriginY,
        [Parameter(Mandatory = $true)][int]$Width,
        [Parameter(Mandatory = $true)][int]$Height
    )

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
        return [Convert]::ToBase64String($Stream.ToArray())
    } finally {
        $Stream.Dispose()
        $Graphics.Dispose()
        $Bitmap.Dispose()
    }
}

function Get-RdpClientDesktopPixelColor {
    param(
        [Parameter(Mandatory = $true)][int]$X,
        [Parameter(Mandatory = $true)][int]$Y
    )

    $Bitmap = New-Object Drawing.Bitmap(1, 1)
    $Graphics = [Drawing.Graphics]::FromImage($Bitmap)
    try {
        $Graphics.CopyFromScreen(
            $X,
            $Y,
            0,
            0,
            $Bitmap.Size,
            [Drawing.CopyPixelOperation]::SourceCopy
        )
        $Color = $Bitmap.GetPixel(0, 0)
        return '#{0:X2}{1:X2}{2:X2}' -f $Color.R, $Color.G, $Color.B
    } finally {
        $Graphics.Dispose()
        $Bitmap.Dispose()
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
    if (@('screenshot', 'pixel', 'click', 'script') -notcontains $Action) {
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
        $Result.ImageBase64 = Get-RdpClientDesktopScreenshotBase64 `
            -OriginX $OriginX `
            -OriginY $OriginY `
            -Width $Width `
            -Height $Height
    } elseif ($Action -in @('pixel', 'click')) {
        $X = Resolve-RdpClientDesktopRequestInteger `
            -RequestObject $Request `
            -Name X
        $Y = Resolve-RdpClientDesktopRequestInteger `
            -RequestObject $Request `
            -Name Y
        Assert-RdpClientDesktopCoordinate `
            -X $X `
            -Y $Y `
            -Width $Width `
            -Height $Height
        $Result.X = $X
        $Result.Y = $Y

        if ($Action -eq 'pixel') {
            $Result.Color = Get-RdpClientDesktopPixelColor `
                -X ($OriginX + $X) `
                -Y ($OriginY + $Y)
        } else {
            [SwawKit.RdpClient.DesktopNative]::LeftClick(
                $OriginX + $X,
                $OriginY + $Y
            )
        }
    } else {
        $StepsProperty = $Request.PSObject.Properties['Steps']
        if ($null -eq $StepsProperty) {
            throw '[INVALID_REQUEST] Desktop script steps were not provided.'
        }
        $Steps = @($StepsProperty.Value)
        if ($Steps.Count -lt 1 -or $Steps.Count -gt 32) {
            throw '[INVALID_REQUEST] Desktop script must contain 1 to 32 steps.'
        }
        $ValidatedSteps = New-Object 'Collections.Generic.List[object]'
        $ScreenshotCount = 0
        for ($Index = 0; $Index -lt $Steps.Count; $Index++) {
            $Step = $Steps[$Index]
            $ActionProperty = $Step.PSObject.Properties['Action']
            if ($null -eq $ActionProperty -or
                [string]::IsNullOrWhiteSpace([string]$ActionProperty.Value)) {
                throw (
                    '[INVALID_REQUEST] Desktop script step {0} has no action.' -f `
                        ($Index + 1)
                )
            }
            $StepAction = [string]$ActionProperty.Value
            $ValidatedStep = [ordered]@{ Action = $StepAction }
            switch ($StepAction) {
                'screenshot' {
                    $ScreenshotCount++
                    if ($ScreenshotCount -gt 8) {
                        throw (
                            '[INVALID_REQUEST] Desktop script may contain ' +
                            'at most 8 screenshots.'
                        )
                    }
                }
                { $_ -in @('pixel', 'click') } {
                    $ValidatedStep.X = Resolve-RdpClientDesktopRequestInteger `
                        -RequestObject $Step `
                        -Name X
                    $ValidatedStep.Y = Resolve-RdpClientDesktopRequestInteger `
                        -RequestObject $Step `
                        -Name Y
                    Assert-RdpClientDesktopCoordinate `
                        -X $ValidatedStep.X `
                        -Y $ValidatedStep.Y `
                        -Width $Width `
                        -Height $Height
                }
                'wait' {
                    $ValidatedStep.Milliseconds = `
                        Resolve-RdpClientDesktopRequestInteger `
                            -RequestObject $Step `
                            -Name Milliseconds `
                            -Maximum 10000
                }
                default {
                    throw (
                        '[INVALID_REQUEST] Unsupported desktop script ' +
                        "action at step $($Index + 1): $StepAction"
                    )
                }
            }
            $ValidatedSteps.Add([pscustomobject]$ValidatedStep)
        }

        $StepResults = New-Object 'Collections.Generic.List[object]'
        $ImageBase64Characters = [int64]0
        for ($Index = 0; $Index -lt $ValidatedSteps.Count; $Index++) {
            $Step = $ValidatedSteps[$Index]
            $StepAction = [string]$Step.Action
            try {
                $StepResult = [ordered]@{
                    Index  = $Index + 1
                    Action = $StepAction
                }
                switch ($StepAction) {
                    'screenshot' {
                        $StepResult.ImageBase64 = `
                            Get-RdpClientDesktopScreenshotBase64 `
                                -OriginX $OriginX `
                                -OriginY $OriginY `
                                -Width $Width `
                                -Height $Height
                        $ImageBase64Characters += `
                            ([string]$StepResult.ImageBase64).Length
                        if ($ImageBase64Characters -gt 50000000) {
                            throw (
                                '[WORKFLOW_RESULT_TOO_LARGE] Encoded workflow ' +
                                'screenshots exceed the 50,000,000-character limit.'
                            )
                        }
                    }
                    'pixel' {
                        $StepX = [int]$Step.X
                        $StepY = [int]$Step.Y
                        $StepResult.X = $StepX
                        $StepResult.Y = $StepY
                        $StepResult.Color = Get-RdpClientDesktopPixelColor `
                            -X ($OriginX + $StepX) `
                            -Y ($OriginY + $StepY)
                    }
                    'click' {
                        $StepX = [int]$Step.X
                        $StepY = [int]$Step.Y
                        [SwawKit.RdpClient.DesktopNative]::LeftClick(
                            $OriginX + $StepX,
                            $OriginY + $StepY
                        )
                        $StepResult.X = $StepX
                        $StepResult.Y = $StepY
                    }
                    'wait' {
                        $Milliseconds = [int]$Step.Milliseconds
                        Start-Sleep -Milliseconds $Milliseconds
                        $StepResult.Milliseconds = $Milliseconds
                    }
                    default {
                        throw (
                            '[INVALID_REQUEST] Unsupported desktop script ' +
                            "action: $StepAction"
                        )
                    }
                }
                $StepResults.Add([pscustomobject]$StepResult)
            } catch {
                throw (
                    '[WORKFLOW_STEP_FAILED] Step {0} ({1}): {2}' -f `
                        ($Index + 1),
                        $StepAction,
                        $_.Exception.Message
                )
            }
        }
        $Result.Steps = $StepResults.ToArray()
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
