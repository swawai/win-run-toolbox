@echo off
chcp 65001 >nul <nul
setlocal DisableDelayedExpansion
if "%~1"=="-h" goto :ShowHelp
if "%~1"=="--help" goto :ShowHelp
if "%~1"=="/?" goto :ShowHelp
goto :Main
:ShowHelp
set "commandName=%~2"
if not defined commandName set "commandName=remote_kit"
PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0help.ps1" -CommandName "%commandName%"
exit /b %ERRORLEVEL%
:Main
rem -----------------------------------------------------------------------------
rem Remote entry config:
rem   %1 = SSH port
rem   %2 = remote host IP/domain
rem   %3 = fixed SSH user
rem   %4 = SSH private key path
rem   %5 = command: -- / tty / script / code / cursor / copy / key.add / key.remove / key.fix / key.add.fix
rem   %6/%7 = command arguments
rem -----------------------------------------------------------------------------
set "port=%~1"
set "host=%~2"
set "remoteUser=%~3"
set "sshKeyPath=%~4"
set "verb=%~5"
set "arg1=%~6"
set "arg2=%~7"
set "arg3=%~8"
set "remoteShell=posix"
if /i "%verb%"=="code" if not "%REMOTE_KIT_PROTOCOL%"=="2" goto :EditorEntryProtocolRequired
if /i "%verb%"=="cursor" if not "%REMOTE_KIT_PROTOCOL%"=="2" goto :EditorEntryProtocolRequired
set "useSshConfigHost="
if defined REMOTE_KIT_ENTRY_FILE goto :InitEmbeddedSshConfigHost
if defined REMOTE_SSH_HOST set "useSshConfigHost=1"
if defined useSshConfigHost goto :InitSshConfigHost
if not defined sshKeyPath set "sshKeyPath=%USERPROFILE%\.ssh\id_rsa"
if not defined port goto :InvalidArgs
if not defined host goto :InvalidArgs
if not defined remoteUser goto :InvalidArgs
for /f "delims=0123456789" %%a in ("%port%") do goto :InvalidArgs
goto :InitDirectHost
:InitEmbeddedSshConfigHost
set "REMOTE_SSH_HOST="
set "REMOTE_SSH_CONFIG=embedded"
set "useSshConfigHost=1"
:InitSshConfigHost
if not defined REMOTE_SSH_CONFIG (
    echo REMOTE_SSH_CONFIG must be set when REMOTE_SSH_HOST is set.
    exit /b 1
)
if /i not "%REMOTE_SSH_CONFIG%"=="embedded" goto :UseExternalSshConfig
if /i "%verb%"=="config.remove" goto :SkipEmbeddedSshConfigWrite
if not defined REMOTE_KIT_ENTRY_FILE (
    echo REMOTE_KIT_ENTRY_FILE must be set when REMOTE_SSH_CONFIG=embedded.
    exit /b 1
)
set "REMOTE_KIT_SSH_CONFIG_PATH="
set "remoteShell="
for /f "tokens=1,* delims=|" %%a in ('PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0ssh_config.ps1" -Action write -EntryFile "%REMOTE_KIT_ENTRY_FILE%" -RepoRoot "%~dp0..\.." -UserProfile "%USERPROFILE%"') do (
    if /i "%%a"=="config" set "REMOTE_KIT_SSH_CONFIG_PATH=%%b"
    if /i "%%a"=="shell" set "remoteShell=%%b"
)
if not defined REMOTE_KIT_SSH_CONFIG_PATH (
    echo Failed to generate embedded SSH config.
    exit /b 1
)
if not defined remoteShell (
    echo Failed to resolve embedded remote shell metadata.
    exit /b 1
)
for %%a in ("%REMOTE_KIT_SSH_CONFIG_PATH%") do set "REMOTE_SSH_HOST=%%~na"
goto :AfterSshConfigPath
:UseExternalSshConfig
set "REMOTE_KIT_SSH_CONFIG_PATH=%REMOTE_SSH_CONFIG%"
goto :AfterSshConfigPath
:SkipEmbeddedSshConfigWrite
set "REMOTE_KIT_SSH_CONFIG_PATH="
:AfterSshConfigPath
set "REMOTE_KIT_SSH_HOST=%REMOTE_SSH_HOST%"
if not defined port set "port=0"
if not defined host set "host=%REMOTE_SSH_HOST%"
if not defined remoteUser set "remoteUser=%REMOTE_SSH_HOST%"
set "REMOTE_TARGET=%REMOTE_SSH_HOST%"
set "VSCODE_REMOTE=ssh-remote+%REMOTE_SSH_HOST%"
set "SSH_CONNECT_OPTS=-F "%REMOTE_KIT_SSH_CONFIG_PATH%""
set "SCP_CONNECT_OPTS=-F "%REMOTE_KIT_SSH_CONFIG_PATH%""
goto :InitCommonOptions
:InitDirectHost
set "REMOTE_TARGET=%remoteUser%@%host%"
set "VSCODE_REMOTE=ssh-remote+%remoteUser%@%host%:%port%"
set "SSH_CONNECT_OPTS=-i "%sshKeyPath%" -p %port%"
set "SCP_CONNECT_OPTS=-i "%sshKeyPath%" -P %port%"
rem Use only the identity from the entry file, avoiding ssh-agent identity noise.
:InitCommonOptions
if not defined REMOTE_KIT_SSH_ID_OPTS if not defined useSshConfigHost set "REMOTE_KIT_SSH_ID_OPTS=-o IdentityAgent=none -o IdentitiesOnly=yes"
if not defined REMOTE_KIT_SSH_HOSTKEY_OPTS if not defined useSshConfigHost set "REMOTE_KIT_SSH_HOSTKEY_OPTS=-o StrictHostKeyChecking=accept-new"
set "SSH_COMMON_OPTS=%REMOTE_KIT_SSH_ID_OPTS% %REMOTE_KIT_SSH_HOSTKEY_OPTS%"
if defined REMOTE_KIT_SSH_LOG_OPTS set "SSH_COMMON_OPTS=%SSH_COMMON_OPTS% %REMOTE_KIT_SSH_LOG_OPTS%"
if not defined REMOTE_KIT_SSH_COMMAND_OPTS set "REMOTE_KIT_SSH_COMMAND_OPTS=-n -T -o BatchMode=yes -o ServerAliveInterval=60 -o ServerAliveCountMax=3"
if not defined REMOTE_KIT_SSH_TTY_OPTS set "REMOTE_KIT_SSH_TTY_OPTS=-tt -o BatchMode=yes -o ServerAliveInterval=60 -o ServerAliveCountMax=3"
set "REMOTE_KIT_VERBOSE_FLAG="
if /i "%REMOTE_KIT_VERBOSE%"=="1" set "REMOTE_KIT_VERBOSE_FLAG=1"
if /i "%REMOTE_KIT_VERBOSE%"=="true" set "REMOTE_KIT_VERBOSE_FLAG=1"
if /i "%REMOTE_KIT_VERBOSE%"=="yes" set "REMOTE_KIT_VERBOSE_FLAG=1"
if /i "%REMOTE_KIT_VERBOSE%"=="on" set "REMOTE_KIT_VERBOSE_FLAG=1"
if /i "%REMOTE_KIT_VERBOSE%"=="debug" set "REMOTE_KIT_VERBOSE_FLAG=1"
set "remoteHome="
if "%verb%"=="--" goto :RemoteCommand
if /i "%verb%"=="tty" goto :TtyRemoteCommand
if /i "%verb%"=="script" goto :ScriptCommand
if /i "%verb%"=="stdin" goto :StdinCommand
if defined arg3 goto :InvalidArgs
if not defined verb if not defined arg1 if not defined arg2 goto :OpenSsh
if /i "%verb%"=="code" goto :CodeCommand
if /i "%verb%"=="cursor" goto :CursorCommand
if /i "%verb%"=="copy" goto :CopyCommand
if /i "%verb%"=="config.install" goto :InstallConfig
if /i "%verb%"=="config.remove" goto :RemoveConfig
if /i "%verb%"=="key.add" goto :AddKey
if /i "%verb%"=="key.remove" goto :RemoveKey
if /i "%verb%"=="key.fix" goto :FixKey
if /i "%verb%"=="key.add.fix" goto :AddKeyFix
goto :InvalidArgs
:CodeCommand
if not defined arg1 goto :InvalidArgs
if defined arg2 goto :WriteSftpConfigCode
call :OpenRemotePath "code" "%arg1%"
exit /b %ERRORLEVEL%
:CursorCommand
if not defined arg1 goto :InvalidArgs
if defined arg2 goto :WriteSftpConfigCursor
call :OpenRemotePath "cursor" "%arg1%"
exit /b %ERRORLEVEL%
:WriteSftpConfigCode
call :WriteSftpConfig "code" "%arg1%" "%arg2%"
exit /b %ERRORLEVEL%
:WriteSftpConfigCursor
call :WriteSftpConfig "cursor" "%arg1%" "%arg2%"
exit /b %ERRORLEVEL%
:CopyCommand
if not defined arg1 goto :InvalidArgs
if not defined arg2 goto :InvalidArgs
set "copySrc=%arg1%"
set "copyDst=%arg2%"
set "copySrcFirst=%copySrc:~0,1%"
set "copyDstFirst=%copyDst:~0,1%"
if "%copySrcFirst%"==":" if "%copyDstFirst%"==":" goto :ScpRemoteToRemote
if "%copySrcFirst%"==":" if not "%copyDstFirst%"==":" goto :ScpRemoteToLocal
if not "%copySrcFirst%"==":" if "%copyDstFirst%"==":" goto :ScpLocalToRemote
echo copy requires at least one remote path. Remote paths must start with a colon.
exit /b 1
:AddKey
if defined arg1 goto :InvalidArgs
PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0key_manager.ps1" -Port "%port%" -RemoteHost "%host%" -RemoteUser "%remoteUser%" -SshKeyPath "%sshKeyPath%" -Action "add"
exit /b %ERRORLEVEL%
:AddKeyFix
if defined arg1 goto :InvalidArgs
PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0key_manager.ps1" -Port "%port%" -RemoteHost "%host%" -RemoteUser "%remoteUser%" -SshKeyPath "%sshKeyPath%" -Action "add" -FixSshdConfig
exit /b %ERRORLEVEL%
:RemoveKey
if defined arg1 goto :InvalidArgs
PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0key_manager.ps1" -Port "%port%" -RemoteHost "%host%" -RemoteUser "%remoteUser%" -SshKeyPath "%sshKeyPath%" -Action "remove"
exit /b %ERRORLEVEL%
:FixKey
if defined arg1 goto :InvalidArgs
PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0key_manager.ps1" -Port "%port%" -RemoteHost "%host%" -RemoteUser "%remoteUser%" -SshKeyPath "%sshKeyPath%" -Action "fix"
exit /b %ERRORLEVEL%
:InstallConfig
if defined arg1 goto :InvalidArgs
if /i not "%REMOTE_SSH_CONFIG%"=="embedded" (
    echo config.install is only supported when REMOTE_SSH_CONFIG=embedded.
    exit /b 1
)
call :InstallSshConfig
if errorlevel 1 exit /b 1
echo SSH config installed for "%REMOTE_SSH_HOST%": "%REMOTE_KIT_SSH_CONFIG_PATH%"
exit /b 0
:RemoveConfig
if defined arg1 goto :InvalidArgs
if /i not "%REMOTE_SSH_CONFIG%"=="embedded" (
    echo config.remove is only supported when REMOTE_SSH_CONFIG=embedded.
    exit /b 1
)
set "REMOTE_SSH_HOST="
for /f "delims=" %%a in ('PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0ssh_config.ps1" -Action remove -EntryFile "%REMOTE_KIT_ENTRY_FILE%" -RepoRoot "%~dp0..\.." -UserProfile "%USERPROFILE%"') do set "REMOTE_SSH_HOST=%%a"
if not defined REMOTE_SSH_HOST (
    echo Failed to remove embedded SSH config.
    exit /b 1
)
set "REMOTE_KIT_SSH_CONFIG_PATH="
echo SSH config removed for "%REMOTE_SSH_HOST%".
exit /b 0
:OpenSsh
if defined REMOTE_KIT_VERBOSE_FLAG echo ssh %SSH_COMMON_OPTS% %SSH_CONNECT_OPTS% "%REMOTE_TARGET%"
ssh %SSH_COMMON_OPTS% %SSH_CONNECT_OPTS% "%REMOTE_TARGET%"
exit /b %ERRORLEVEL%
:RemoteCommand
set "remoteCommandSshOpts=%REMOTE_KIT_SSH_COMMAND_OPTS%"
set "remoteCommand="
shift /5
goto :RemoteCommandArgLoop
:TtyRemoteCommand
if not "%arg1%"=="--" goto :InvalidArgs
set "remoteCommandSshOpts=%REMOTE_KIT_SSH_TTY_OPTS%"
set "remoteCommand="
shift /5
shift /5
:RemoteCommandArgLoop
if "%~5"=="" goto :RunRemoteCommand
if defined remoteCommand (
    set "remoteCommand=%remoteCommand% %~5"
) else (
    set "remoteCommand=%~5"
)
shift /5
goto :RemoteCommandArgLoop
:RunRemoteCommand
if not defined remoteCommand goto :InvalidArgs
if /i not "%remoteShell%"=="posix" if /i not "%remoteShell%"=="win.cmd" (
    echo [ERROR] Remote shell profile "%remoteShell%" is recognized but not implemented for remote commands.
    exit /b 1
)
if /i "%remoteShell%"=="win.cmd" set "remoteCommand=chcp 65001>nul & %remoteCommand%"
if defined REMOTE_KIT_VERBOSE_FLAG echo ssh %SSH_COMMON_OPTS% %remoteCommandSshOpts% %SSH_CONNECT_OPTS% "%REMOTE_TARGET%" "%remoteCommand%"
ssh %SSH_COMMON_OPTS% %remoteCommandSshOpts% %SSH_CONNECT_OPTS% "%REMOTE_TARGET%" "%remoteCommand%"
exit /b %ERRORLEVEL%
:ScriptCommand
if not defined arg1 goto :InvalidArgs
set "REMOTE_KIT_SCRIPT_ARG_COUNT=0"
shift /6
:ScriptCommandArgLoop
if "%~6"=="" goto :RunScriptCommand
set /a REMOTE_KIT_SCRIPT_ARG_COUNT+=1
set "REMOTE_KIT_SCRIPT_ARG_%REMOTE_KIT_SCRIPT_ARG_COUNT%=%~6"
shift /6
goto :ScriptCommandArgLoop
:RunScriptCommand
PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0script_runner.ps1" -Port "%port%" -RemoteHost "%host%" -RemoteUser "%remoteUser%" -SshKeyPath "%sshKeyPath%" -ScriptPath "%arg1%"
exit /b %ERRORLEVEL%
:StdinCommand
if not "%arg1%"=="--" goto :InvalidArgs
set "REMOTE_KIT_STDIN_ARG_COUNT=0"
shift /6
:StdinCommandArgLoop
if "%~6"=="" goto :RunStdinCommand
set /a REMOTE_KIT_STDIN_ARG_COUNT+=1
set "REMOTE_KIT_STDIN_ARG_%REMOTE_KIT_STDIN_ARG_COUNT%=%~6"
shift /6
goto :StdinCommandArgLoop
:RunStdinCommand
if "%REMOTE_KIT_STDIN_ARG_COUNT%"=="0" goto :InvalidArgs
PowerShell -NoLogo -NoProfile -NonInteractive -OutputFormat Text -ExecutionPolicy Bypass -File "%~dp0stdin_runner.ps1" -Port "%port%" -RemoteHost "%host%" -RemoteUser "%remoteUser%" -SshKeyPath "%sshKeyPath%" -RemoteArgumentCount %REMOTE_KIT_STDIN_ARG_COUNT% -RemoteShell "%remoteShell%"
exit /b %ERRORLEVEL%
:OpenRemotePath
set "editorExe=%~1"
set "remoteArg=%~2"
call :ResolveRemotePath "%remoteArg%"
if errorlevel 1 exit /b 1
call :InstallSshConfig
if errorlevel 1 exit /b 1
set "editorTarget=%remotePath%"
set "editorRemoteAuthority=%VSCODE_REMOTE%"
call :LaunchEditor
exit /b %ERRORLEVEL%
:InstallSshConfig
if /i not "%REMOTE_SSH_CONFIG%"=="embedded" exit /b 0
if not defined REMOTE_KIT_ENTRY_FILE (
    echo REMOTE_KIT_ENTRY_FILE must be set when REMOTE_SSH_CONFIG=embedded.
    exit /b 1
)
set "REMOTE_KIT_SSH_CONFIG_PATH="
set "remoteShell="
for /f "tokens=1,* delims=|" %%a in ('PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0ssh_config.ps1" -Action install -EntryFile "%REMOTE_KIT_ENTRY_FILE%" -RepoRoot "%~dp0..\.." -UserProfile "%USERPROFILE%"') do (
    if /i "%%a"=="config" set "REMOTE_KIT_SSH_CONFIG_PATH=%%b"
    if /i "%%a"=="shell" set "remoteShell=%%b"
)
if not defined REMOTE_KIT_SSH_CONFIG_PATH (
    echo Failed to install embedded SSH config.
    exit /b 1
)
if not defined remoteShell (
    echo Failed to resolve embedded remote shell metadata.
    exit /b 1
)
set "SSH_CONNECT_OPTS=-F "%REMOTE_KIT_SSH_CONFIG_PATH%""
set "SCP_CONNECT_OPTS=-F "%REMOTE_KIT_SSH_CONFIG_PATH%""
exit /b 0
:WriteSftpConfig
set "editorExe=%~1"
set "path1=%~2"
set "path2=%~3"
set "firstChar1=%path1:~0,1%"
set "firstChar2=%path2:~0,1%"
if "%firstChar1%"==":" if not "%firstChar2%"==":" goto :WriteSftpRemoteFirst
if not "%firstChar1%"==":" if "%firstChar2%"==":" goto :WriteSftpLocalFirst
echo SFTP setup requires exactly one remote path and one local path.
exit /b 1
:WriteSftpRemoteFirst
set "remoteArg=%path1%"
set "localPath=%path2%"
goto :WriteSftpResolved
:WriteSftpLocalFirst
set "localPath=%path1%"
set "remoteArg=%path2%"
goto :WriteSftpResolved
:WriteSftpResolved
call :ResolveRemotePath "%remoteArg%"
if errorlevel 1 exit /b 1
if not defined localPath (
    echo Local sync directory must not be empty.
    exit /b 1
)
if exist "%localPath%" if not exist "%localPath%\" (
    echo Local sync path exists but is not a directory: "%localPath%"
    exit /b 1
)
if not exist "%localPath%\.vscode\" mkdir "%localPath%\.vscode"
if errorlevel 1 (
    echo Failed to create local VS Code config directory: "%localPath%\.vscode"
    exit /b 1
)
set "sftpFile=%localPath%\.vscode\SFTP.json"
if not exist "%sftpFile%" goto :WriteSftpFile
for /f "delims=" %%a in ('PowerShell -NoProfile -ExecutionPolicy Bypass -Command "Get-Date -Format yyyyMMddHHmmss"') do set "sftpBackupStamp=%%a"
set "sftpBackupFile=%sftpFile%.swaw-kit-ssh-remote-backup-%sftpBackupStamp%"
copy /Y "%sftpFile%" "%sftpBackupFile%" >nul
if errorlevel 1 (
    echo Failed to back up existing SFTP config: "%sftpFile%"
    exit /b 1
)
echo Existing SFTP config backed up: "%sftpBackupFile%"
:WriteSftpFile
set "sftpName=%localPath:\=/%"
set "sftpKey=%sshKeyPath:\=/%"
if defined useSshConfigHost goto :WriteSftpFileSshConfig
> "%sftpFile%" echo {
>> "%sftpFile%" echo     "name": "%sftpName%.%remoteUser%",
>> "%sftpFile%" echo     "host": "%host%",
>> "%sftpFile%" echo     "protocol": "sftp",
>> "%sftpFile%" echo     "port": %port%,
>> "%sftpFile%" echo     "username": "%remoteUser%",
>> "%sftpFile%" echo     "privateKeyPath": "%sftpKey%",
>> "%sftpFile%" echo     "remotePath": "%remotePath%",
>> "%sftpFile%" echo     "uploadOnSave": true,
>> "%sftpFile%" echo     "useTempFile": false,
>> "%sftpFile%" echo     "openSsh": false
>> "%sftpFile%" echo }
if errorlevel 1 (
    echo Failed to write SFTP config: "%sftpFile%"
    exit /b 1
)
echo SFTP config written: "%sftpFile%"
goto :OpenSftpWorkspace
:WriteSftpFileSshConfig
set "sftpConfig=%REMOTE_KIT_SSH_CONFIG_PATH:\=/%"
> "%sftpFile%" echo {
>> "%sftpFile%" echo     "name": "%sftpName%.%REMOTE_SSH_HOST%",
>> "%sftpFile%" echo     "host": "%REMOTE_SSH_HOST%",
>> "%sftpFile%" echo     "protocol": "sftp",
>> "%sftpFile%" echo     "sshConfigPath": "%sftpConfig%",
>> "%sftpFile%" echo     "remotePath": "%remotePath%",
>> "%sftpFile%" echo     "uploadOnSave": true,
>> "%sftpFile%" echo     "useTempFile": false,
>> "%sftpFile%" echo     "openSsh": true
>> "%sftpFile%" echo }
if errorlevel 1 (
    echo Failed to write SFTP config: "%sftpFile%"
    exit /b 1
)
echo SFTP config written: "%sftpFile%"
:OpenSftpWorkspace
echo SFTP config is ready. Required extension: SFTP by Natizyskunk
set "editorTarget=%localPath%"
set "editorRemoteAuthority="
call :LaunchEditor
exit /b %ERRORLEVEL%
:ScpRemoteToRemote
set "src=%copySrc:~1%"
set "dst=%copyDst:~1%"
if not defined src goto :InvalidArgs
if not defined dst goto :InvalidArgs
echo scp -3: "%REMOTE_TARGET%:%src%"  to  "%REMOTE_TARGET%:%dst%"
scp -3 %SSH_COMMON_OPTS% %SCP_CONNECT_OPTS% -r %REMOTE_TARGET%:"%src%" %REMOTE_TARGET%:"%dst%"
exit /b %ERRORLEVEL%
:ScpRemoteToLocal
set "src=%copySrc:~1%"
set "dst=%copyDst%"
if not defined src goto :InvalidArgs
echo scp: "%REMOTE_TARGET%:%src%"  to  "%dst%"
scp %SSH_COMMON_OPTS% %SCP_CONNECT_OPTS% -r %REMOTE_TARGET%:"%src%" "%dst%"
exit /b %ERRORLEVEL%
:ScpLocalToRemote
set "src=%copySrc%"
set "dst=%copyDst:~1%"
if not defined dst goto :InvalidArgs
echo scp: "%src%"  to  "%REMOTE_TARGET%:%dst%"
scp %SSH_COMMON_OPTS% %SCP_CONNECT_OPTS% -r "%src%" %REMOTE_TARGET%:"%dst%"
exit /b %ERRORLEVEL%
:ResolveRemotePath
set "remoteInput=%~1"
if not defined remoteInput (
    echo Remote path must not be empty.
    exit /b 1
)
if "%remoteInput:~0,1%"==":" set "remoteInput=%remoteInput:~1%"
if not defined remoteInput (
    echo Remote path must not be empty.
    exit /b 1
)
if "%remoteInput:~0,1%"=="/" (
    set "remotePath=%remoteInput%"
    exit /b 0
)
call :GetRemoteHome
if errorlevel 1 exit /b 1
set "remotePath=%remoteHome%/%remoteInput%"
exit /b 0
:GetRemoteHome
if defined remoteHome exit /b 0
for /f "delims=" %%a in ('PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0home_reader.ps1" -Port "%port%" -RemoteHost "%host%" -RemoteUser "%remoteUser%" -SshKeyPath "%sshKeyPath%"') do set "remoteHome=%%a"
if not defined remoteHome (
    echo Failed to read remote $HOME. Check that the host is online and Unix-like.
    exit /b 1
)
if "%remoteHome%"=="$HOME" (
    echo Failed to read remote $HOME. Check that the host is online and Unix-like.
    exit /b 1
)
exit /b 0
:EditorEntryProtocolRequired
echo [ERROR] This remote entry predates the clean editor bootstrap.
echo [ERROR] Update its header from Favorites\template.vps1.cmd before using %verb%.
exit /b 1
:LaunchEditor
set "editorReuseFlag="
if /i "%WIN_RUN_EDITOR_BOOTSTRAP%"=="%editorExe%" set "editorReuseFlag=1"
set "WIN_RUN_EDITOR_BOOTSTRAP="
set "WIN_RUN_REMOTE_EDITOR_TARGET=%editorTarget%"
set "WIN_RUN_REMOTE_EDITOR_AUTHORITY=%editorRemoteAuthority%"
if defined editorReuseFlag (
    PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0editor-launch.ps1" -Tool "%editorExe%" -ReuseBootstrapWindow
) else (
    PowerShell -NoProfile -ExecutionPolicy Bypass -File "%~dp0editor-launch.ps1" -Tool "%editorExe%"
)
set "editorLaunchResult=%ERRORLEVEL%"
set "WIN_RUN_REMOTE_EDITOR_TARGET="
set "WIN_RUN_REMOTE_EDITOR_AUTHORITY="
exit /b %editorLaunchResult%
:InvalidArgs
echo Unrecognized argument combination. Run -h to view usage.
exit /b 1
