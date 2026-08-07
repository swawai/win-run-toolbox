@echo off & chcp 65001 >nul <nul & setlocal DisableDelayedExpansion
call "%~dp0_lib\editor_kit\entry-bootstrap.cmd" ".%~1" "REMOTE_TARGET" || call exit /b %%errorlevel%%
set "REMOTE_KIT_PROTOCOL=2" & goto :REMOTE_KIT_AFTER_SSH_CONFIG
:::::::::::::::::::::::::::::::::::::::::::::::::::
:: Use the full .ssh/config format:
:: 使用完整的 .ssh/config 格式配置:
:::::::::::::::::::::::::::::::::::::::::::::::::::

Host ___self___                     # 这一行不要改(Don't change this line)
  ___RemoteShell___ posix           # 可选(optional): posix, win.cmd, win.powershell, win.pwsh, win.git-bash
  HostName myvps1.example.com       # 必填(are required)
  User root                         # 必填(are required)
  Port 22                           # 必填(are required)
  IdentityFile ~/.ssh/id_rsa        # 必填(are required)
  StrictHostKeyChecking accept-new



:::::::::::::::::::::::::::::::::::::::::::::::::::
:: Do not edit anything below:
:: 以下任何内容请勿编辑：
:::::::::::::::::::::::::::::::::::::::::::::::::::
:REMOTE_KIT_AFTER_SSH_CONFIG
set "REMOTE_KIT=%~dp0_lib\ssh_remote_kit\kit.cmd"
if exist "%REMOTE_KIT%" goto :RemoteKitFound
echo [ERROR] SSH remote kit not found:
echo   "%REMOTE_KIT%"
exit /b 1
:RemoteKitFound
if /i "%~1"=="-h" goto :ShowRemoteKitHelp
if /i "%~1"=="--help" goto :ShowRemoteKitHelp
if "%~1"=="/?" goto :ShowRemoteKitHelp
set "REMOTE_KIT_ENTRY_FILE=%~f0"
"%REMOTE_KIT%" "0" "" "" "__REMOTE_KIT_SSH_CONFIG_IDENTITY__" %*
:ShowRemoteKitHelp
"%REMOTE_KIT%" -h "%~n0"
