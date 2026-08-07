@echo off & chcp 65001 >nul <nul & setlocal DisableDelayedExpansion & set "RDP_CLIENT_PROTOCOL=1"
:::::::::::::::::::::::::::::::::::::::::::::::::::
:: Stable RDP-only alias. Each account on the same real host must use a different alias:
:: 稳定的 RDP 专用别名；同一真实主机的每个账号必须使用不同别名:
:::::::::::::::::::::::::::::::::::::::::::::::::::
set "RDP_HOST_ALIAS="

:::::::::::::::::::::::::::::::::::::::::::::::::::
:: Generated .rdp path. Empty uses the current Windows user's real Desktop directory:
:: 生成的 .rdp 文件路径；留空时使用当前 Windows 用户的实际“桌面”目录:
:: Example / 示例: set "RDP_OUTPUT_PATH=D:\RDP\muwen2024-administrator.rdp"
:::::::::::::::::::::::::::::::::::::::::::::::::::
set "RDP_OUTPUT_PATH="

:::::::::::::::::::::::::::::::::::::::::::::::::::
:: Optional template.vps1.cmd instance used by Shadow and .peer psexec:
:: 可选：供 Shadow 功能和 .peer psexec 使用的 template.vps1.cmd 实例:
:: Example / 示例: set "RDP_PEER_SSH_ENTRY=D:\swaw-kit\Favorites\server-admin.ssh.cmd"
:::::::::::::::::::::::::::::::::::::::::::::::::::
set "RDP_PEER_SSH_ENTRY=D:\2026.7\swaw-kit\rdp1.ssh.cmd"

:::::::::::::::::::::::::::::::::::::::::::::::::::
:: Optional: force help language zh-CN / en; empty selects the system language automatically:
:: 可选：指定 help 语言 zh-CN / en；留空时根据系统语言自动选择:
:::::::::::::::::::::::::::::::::::::::::::::::::::
set "RDP_HELP_LANG="

:::::::::::::::::::::::::::::::::::::::::::::::::::
:: Embedded RDP file properties:
:: 嵌入式 RDP 文件属性:
:: These lines are skipped by cmd.exe and describe the source .rdp document:
:: The generated copy replaces only "full address" with RDP_HOST_ALIAS:
:: cmd.exe 会跳过这些行；它们描述源 .rdp 文档:
:: 生成副本时，只把 "full address" 替换为 RDP_HOST_ALIAS:
:::::::::::::::::::::::::::::::::::::::::::::::::::
goto :RdpClientAfterEmbeddedRdpProperties

:: 远程对端的[主机地址:服务端口]，若 RDP_HOST_ALIAS 不为空，应用时主机地址会替换:
full address:s:192.168.1.115:3389
:: 此入口绑定的账号；建议写成 用户名 或 计算机名\用户名，域\用户名:
username:s:administrator

:: 屏幕模式：1=窗口，2=全屏:
screen mode id:i:1
:: 窗口模式时，窗口初始显示位置,3/4,5/6号数字分别表示左上角和右下角坐标像素点:
winposstr:s:0,1,44,89,3000,2000
:: 窗口模式时，窗口尺寸(像素):
desktopwidth:i:1200
desktopheight:i:800
:: 是否应用多个显示器：0=单显示器，1=多显示器:
use multimon:i:0

:: Win 按键是否发送到远程机：0=不发送（进本机），1=发送，2=仅全屏时发送:
keyboardhook:i:1

:: 剪贴板重定向：0=禁用，1=启用:
redirectclipboard:i:1
:: 驱动器重定向；多个盘符以分号分隔，空值表示禁用:
:: drivestoredirect:s:D:\;
:: 打印机重定向：0=禁用，1=启用:
redirectprinters:i:0
:: 串口重定向：0=禁用，1=启用:
redirectcomports:i:0
:: WebAuthn/FIDO 验证器重定向：0=禁用，1=启用:
redirectwebauthn:i:0
:: 智能卡重定向：0=禁用，1=启用:
redirectsmartcards:i:0

:: RemoteApp 模式：0=禁用，1=启用:
remoteapplicationmode:i:0
:: 直接指定未发布的程序需要服务端组策略允许，远程桌面会话主机 > 连接 > 允许远程启动未列出的程序:
remoteapplicationprogram:s:C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe
remoteapplicationcmdline:s:https://chat.openai.com/chat
remoteapplicationexpandworkingdir:i:1
remoteapplicationname:s:chatGPT
remoteapplicationicon:s:

:RdpClientAfterEmbeddedRdpProperties



:::::::::::::::::::::::::::::::::::::::::::::::::::
:: Do not edit below:
:: 下面的内容不要修改:
:::::::::::::::::::::::::::::::::::::::::::::::::::
for %%I in ("%~dp0_lib\rdp_client\client.cmd") do set "RDP_CLIENT_RUNTIME_INSTALLED=%%~fI"
for %%I in ("%~dp0..\_lib\rdp_client\client.cmd") do set "RDP_CLIENT_RUNTIME_TEMPLATE=%%~fI"

set "RDP_CLIENT_RUNTIME=%RDP_CLIENT_RUNTIME_INSTALLED%"
if exist "%RDP_CLIENT_RUNTIME%" goto :RdpClientFound
set "RDP_CLIENT_RUNTIME=%RDP_CLIENT_RUNTIME_TEMPLATE%"
if exist "%RDP_CLIENT_RUNTIME%" goto :RdpClientFound
echo [ERROR] RDP client runtime not found:
echo   "%RDP_CLIENT_RUNTIME_INSTALLED%"
echo   "%RDP_CLIENT_RUNTIME_TEMPLATE%"
echo.
echo Expected _lib\rdp_client\client.cmd next to the entry or its Favorites parent.
exit /b 1

:RdpClientFound
set "RDP_ENTRY_COMMAND=%~n0"
set "RDP_ENTRY_FILE=%~f0"

:: Tail-call the client runtime so cmd.exe does not parse forwarded arguments twice:
"%RDP_CLIENT_RUNTIME%" %*
