Set-StrictMode -Version 2.0

$RdpClientLaunchUiNativeSource = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace SwawKit.RdpClient
{
    public sealed class ProcessSnapshotEntry
    {
        public int ProcessId { get; set; }
        public int ParentProcessId { get; set; }
        public string Name { get; set; }
    }

    public static class LaunchUiNative
    {
        private const uint TH32CS_SNAPPROCESS = 0x00000002;
        private static readonly IntPtr INVALID_HANDLE_VALUE = new IntPtr(-1);

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct PROCESSENTRY32
        {
            public uint dwSize;
            public uint cntUsage;
            public uint th32ProcessID;
            public IntPtr th32DefaultHeapID;
            public uint th32ModuleID;
            public uint cntThreads;
            public uint th32ParentProcessID;
            public int pcPriClassBase;
            public uint dwFlags;

            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
            public string szExeFile;
        }

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr CreateToolhelp32Snapshot(
            uint flags,
            uint processId
        );

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool Process32FirstW(
            IntPtr snapshot,
            ref PROCESSENTRY32 entry
        );

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool Process32NextW(
            IntPtr snapshot,
            ref PROCESSENTRY32 entry
        );

        [DllImport("kernel32.dll")]
        private static extern bool CloseHandle(IntPtr handle);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        public static extern int MessageBoxW(
            IntPtr owner,
            string text,
            string caption,
            uint type
        );

        public static ProcessSnapshotEntry[] GetProcessSnapshot()
        {
            IntPtr snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if (snapshot == INVALID_HANDLE_VALUE)
            {
                return new ProcessSnapshotEntry[0];
            }
            try
            {
                List<ProcessSnapshotEntry> result =
                    new List<ProcessSnapshotEntry>();
                PROCESSENTRY32 entry = new PROCESSENTRY32();
                entry.dwSize = (uint)Marshal.SizeOf(typeof(PROCESSENTRY32));
                if (!Process32FirstW(snapshot, ref entry))
                {
                    return result.ToArray();
                }
                do
                {
                    result.Add(new ProcessSnapshotEntry
                    {
                        ProcessId = checked((int)entry.th32ProcessID),
                        ParentProcessId = checked((int)entry.th32ParentProcessID),
                        Name = entry.szExeFile ?? String.Empty
                    });
                    entry.dwSize = (uint)Marshal.SizeOf(typeof(PROCESSENTRY32));
                }
                while (Process32NextW(snapshot, ref entry));
                return result.ToArray();
            }
            finally
            {
                CloseHandle(snapshot);
            }
        }
    }
}
'@

function Initialize-RdpClientLaunchUiNative {
    if (-not ('SwawKit.RdpClient.LaunchUiNative' -as [type])) {
        Add-Type -TypeDefinition $RdpClientLaunchUiNativeSource -Language CSharp
    }
}

function Get-RdpClientProcessSnapshot {
    Initialize-RdpClientLaunchUiNative
    return @([SwawKit.RdpClient.LaunchUiNative]::GetProcessSnapshot())
}

function Test-RdpClientExplorerLaunch {
    param(
        [int]$CurrentProcessId = $PID,
        [AllowNull()][object[]]$ProcessSnapshot
    )

    try {
        $Snapshot = if ($PSBoundParameters.ContainsKey('ProcessSnapshot')) {
            @($ProcessSnapshot)
        } else {
            @(Get-RdpClientProcessSnapshot)
        }
        $ById = @{}
        foreach ($Process in $Snapshot) {
            $ById[[int]$Process.ProcessId] = $Process
        }
        if (-not $ById.ContainsKey($CurrentProcessId)) {
            return $false
        }
        $ParentId = [int]$ById[$CurrentProcessId].ParentProcessId
        if (-not $ById.ContainsKey($ParentId) -or
            -not [string]::Equals(
                [string]$ById[$ParentId].Name,
                'cmd.exe',
                [StringComparison]::OrdinalIgnoreCase
            )) {
            return $false
        }
        $GrandparentId = [int]$ById[$ParentId].ParentProcessId
        return $ById.ContainsKey($GrandparentId) -and
            [string]::Equals(
                [string]$ById[$GrandparentId].Name,
                'explorer.exe',
                [StringComparison]::OrdinalIgnoreCase
            )
    } catch {
        return $false
    }
}

function Get-RdpClientLaunchUiLanguage {
    $Configured = [string]$env:RDP_HELP_LANG
    if ([string]::Equals($Configured, 'zh-CN', [StringComparison]::OrdinalIgnoreCase)) {
        return 'zh-CN'
    }
    if ([string]::Equals($Configured, 'en', [StringComparison]::OrdinalIgnoreCase)) {
        return 'en'
    }
    if ([Globalization.CultureInfo]::CurrentUICulture.Name.StartsWith(
        'zh',
        [StringComparison]::OrdinalIgnoreCase
    )) {
        return 'zh-CN'
    }
    return 'en'
}

function Show-RdpClientNativeMessage {
    param(
        [Parameter(Mandatory = $true)][string]$Message,
        [Parameter(Mandatory = $true)][string]$Title,
        [Parameter(Mandatory = $true)][uint32]$Style
    )

    Initialize-RdpClientLaunchUiNative
    return [SwawKit.RdpClient.LaunchUiNative]::MessageBoxW(
        [IntPtr]::Zero,
        $Message,
        $Title,
        $Style -bor 0x00012000
    )
}

function Request-RdpClientHostsInstall {
    param(
        [Parameter(Mandatory = $true)][string]$HostAlias,
        [Parameter(Mandatory = $true)][string]$CommandName
    )

    $Message = if ((Get-RdpClientLaunchUiLanguage) -eq 'zh-CN') {
        (
            "远程桌面别名尚未安装：`n`n$HostAlias`n`n" +
            "连接前需要把该别名写入 Windows hosts 文件。`n" +
            "是否现在申请管理员权限并安装？`n`n" +
            "命令：$CommandName .hosts install --uac`n`n" +
            '选择“是”安装；选择“否”取消连接。'
        )
    } else {
        (
            "The Remote Desktop alias is not installed:`n`n$HostAlias`n`n" +
            "It must be added to the Windows hosts file before connecting.`n" +
            "Request administrator approval and install it now?`n`n" +
            "Command: $CommandName .hosts install --uac`n`n" +
            'Choose Yes to install, or No to cancel the connection.'
        )
    }
    $Result = Show-RdpClientNativeMessage `
        -Message $Message `
        -Title "$CommandName - Remote Desktop setup" `
        -Style 0x34
    return $(if ($Result -eq 6) { 'Install' } else { 'Cancel' })
}

function Request-RdpClientSigningSetup {
    param([Parameter(Mandatory = $true)][string]$CommandName)

    $Message = if ((Get-RdpClientLaunchUiLanguage) -eq 'zh-CN') {
        (
            "当前 Windows 用户尚未安装 swaw-kit RDP 签名身份。`n`n" +
            "安装会为当前用户创建签名私钥、Root 信任副本和 mstsc " +
            "发布者信任项；不需要管理员权限。`n`n" +
            "命令：$CommandName .sign install`n`n" +
            '选择“是”安装并连接；选择“否”继续使用未签名文件；' +
            '选择“取消”停止。'
        )
    } else {
        (
            "The current Windows user has not installed the swaw-kit RDP " +
            "signing identity.`n`nInstallation creates a current-user signing " +
            "key, Root trust copy, and mstsc publisher trust entry. It does " +
            "not require administrator privileges.`n`n" +
            "Command: $CommandName .sign install`n`n" +
            'Choose Yes to install and connect, No to continue unsigned, ' +
            'or Cancel to stop.'
        )
    }
    $Result = Show-RdpClientNativeMessage `
        -Message $Message `
        -Title "$CommandName - RDP file signing" `
        -Style 0x23
    switch ($Result) {
        6 { return 'Install' }
        7 { return 'ContinueUnsigned' }
        default { return 'Cancel' }
    }
}

function Show-RdpClientLaunchError {
    param(
        [Parameter(Mandatory = $true)][string]$Message,
        [Parameter(Mandatory = $true)][string]$CommandName
    )

    try {
        $Text = if ((Get-RdpClientLaunchUiLanguage) -eq 'zh-CN') {
            "远程桌面连接失败：`n`n$Message"
        } else {
            "Remote Desktop connection failed:`n`n$Message"
        }
        $null = Show-RdpClientNativeMessage `
            -Message $Text `
            -Title "$CommandName - Remote Desktop" `
            -Style 0x10
    } catch {
    }
}
