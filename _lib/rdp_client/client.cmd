@echo off
chcp 65001 >nul <nul
setlocal DisableDelayedExpansion

if not "%RDP_CLIENT_PROTOCOL%"=="1" goto :InvalidEntryProtocol
if not defined RDP_ENTRY_COMMAND set "RDP_ENTRY_COMMAND=rdp"
set "RDP_CLIENT_SESSION_PARAMETER="
set "RDP_DESKTOP_ACTION="
set "RDP_DESKTOP_DISPLAY="
set "RDP_DESKTOP_TIMEOUT=60s"
set "RDP_DESKTOP_TIMEOUT_SET="
set "RDP_DESKTOP_OUTPUT_PATH="
set "RDP_DESKTOP_SCRIPT_PATH="
set "RDP_DESKTOP_X="
set "RDP_DESKTOP_Y="

if "%~1"=="" goto :Connect
if /i "%~1"==".help" goto :ShowHelp
if /i "%~1"==".h" goto :ShowHelp
if "%~1"=="-h" goto :ShowHelp
if /i "%~1"=="--help" goto :ShowHelp
if /i "%~1"==".rdp" goto :GenerateRdp
if /i "%~1"==".list" goto :SessionList
if /i "%~1"==".shadow" goto :Shadow
if /i "%~1"==".peer" goto :Peer
if /i "%~1"==".hosts" goto :Hosts
if /i "%~1"==".sign" goto :Signing
set "RDP_CLIENT_SELECTOR=%~1"
if "%RDP_CLIENT_SELECTOR:~0,1%"=="." goto :SessionSelector
goto :UnknownCommand

:Connect
set "RDP_CLIENT_LAUNCH=-Launch"
set "RDP_CLIENT_FORCE="
goto :RunRdp

:GenerateRdp
set "RDP_CLIENT_LAUNCH="
set "RDP_CLIENT_FORCE="
if /i not "%~2"=="create" goto :InvalidRdpCommand
if "%~3"=="" goto :RunRdp
if /i not "%~3"=="--force" goto :InvalidRdpCommand
if not "%~4"=="" goto :InvalidRdpCommand
set "RDP_CLIENT_FORCE=-Force"
goto :RunRdp

:SessionSelector
set "RDP_CLIENT_SESSION_ID=%RDP_CLIENT_SELECTOR:~1%"
if "%RDP_CLIENT_SESSION_ID%"=="" goto :UnknownCommand
for /f "delims=0123456789" %%I in ("%RDP_CLIENT_SESSION_ID%") do goto :UnknownCommand
if "%~2"=="" goto :SessionConnect
if /i "%~2"=="connect" goto :SessionConnect
if /i "%~2"=="screenshot" goto :SessionScreenshot
if /i "%~2"=="pixel" goto :SessionPixel
if /i "%~2"=="click" goto :SessionClick
if /i "%~2"=="script" goto :SessionScript
goto :InvalidSessionCommand

:SessionConnect
if not "%~3"=="" goto :InvalidSessionCommand
set "RDP_CLIENT_LAUNCH=-Launch"
set "RDP_CLIENT_FORCE="
set "RDP_CLIENT_SESSION_PARAMETER=-SessionId %RDP_CLIENT_SESSION_ID%"
goto :RunRdp

:SessionScreenshot
set "RDP_DESKTOP_ACTION=screenshot"
goto :ParseNextDesktopOption

:SessionPixel
set "RDP_DESKTOP_ACTION=pixel"
goto :ParseDesktopCoordinates

:SessionClick
set "RDP_DESKTOP_ACTION=click"

:ParseDesktopCoordinates
if "%~3"=="" goto :InvalidSessionCommand
if "%~4"=="" goto :InvalidSessionCommand
set "RDP_DESKTOP_X=%~3"
set "RDP_DESKTOP_Y=%~4"
shift /3
shift /3
goto :ParseNextDesktopOption

:SessionScript
if "%~3"=="" goto :InvalidSessionCommand
set "RDP_DESKTOP_ACTION=script"
set "RDP_DESKTOP_SCRIPT_PATH=%~3"
shift /3

:ParseNextDesktopOption
if "%~3"=="" goto :RunDesktop
if /i "%~3"=="--display" goto :ParseDesktopDisplay
if /i "%~3"=="--timeout" goto :ParseDesktopTimeout
if /i "%~3"=="--output" goto :ParseDesktopOutput
goto :InvalidSessionCommand

:ParseDesktopDisplay
if defined RDP_DESKTOP_DISPLAY goto :InvalidSessionCommand
set "RDP_DESKTOP_DISPLAY=-Display"
shift /3
goto :ParseNextDesktopOption

:ParseDesktopTimeout
if defined RDP_DESKTOP_TIMEOUT_SET goto :InvalidSessionCommand
if "%~4"=="" goto :InvalidSessionCommand
set "RDP_DESKTOP_TIMEOUT=%~4"
set "RDP_DESKTOP_TIMEOUT_SET=1"
shift /3
shift /3
goto :ParseNextDesktopOption

:ParseDesktopOutput
if /i not "%RDP_DESKTOP_ACTION%"=="screenshot" goto :InvalidSessionCommand
if defined RDP_DESKTOP_OUTPUT_PATH goto :InvalidSessionCommand
if "%~4"=="" goto :InvalidSessionCommand
set "RDP_DESKTOP_OUTPUT_PATH=%~4"
shift /3
shift /3
goto :ParseNextDesktopOption

:RunDesktop
if not defined RDP_ENTRY_FILE goto :InvalidEntryFile
set "RDP_DESKTOP_SCRIPT=%~dp0desktop.ps1"
if not exist "%RDP_DESKTOP_SCRIPT%" (
    echo [ERROR] RDP desktop task script not found:
    echo   "%RDP_DESKTOP_SCRIPT%"
    exit /b 1
)

PowerShell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%RDP_DESKTOP_SCRIPT%" -Action "%RDP_DESKTOP_ACTION%" -EntryFile "%RDP_ENTRY_FILE%" -SshEntryFile "%RDP_PEER_SSH_ENTRY%" -SessionId "%RDP_CLIENT_SESSION_ID%" -CommandName "%RDP_ENTRY_COMMAND%" -X "%RDP_DESKTOP_X%" -Y "%RDP_DESKTOP_Y%" -Timeout "%RDP_DESKTOP_TIMEOUT%" -OutputPath "%RDP_DESKTOP_OUTPUT_PATH%" -ScriptPath "%RDP_DESKTOP_SCRIPT_PATH%" %RDP_DESKTOP_DISPLAY%
exit /b %ERRORLEVEL%

:SessionList
if not "%~2"=="" goto :InvalidSessionCommand
if not defined RDP_ENTRY_FILE goto :InvalidEntryFile
set "RDP_SESSION_LIST_SCRIPT=%~dp0session-list.ps1"
if not exist "%RDP_SESSION_LIST_SCRIPT%" (
    echo [ERROR] RDP session list script not found:
    echo   "%RDP_SESSION_LIST_SCRIPT%"
    exit /b 1
)

PowerShell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%RDP_SESSION_LIST_SCRIPT%" -SshEntryFile "%RDP_PEER_SSH_ENTRY%" -RdpEntryFile "%RDP_ENTRY_FILE%" -CommandName "%RDP_ENTRY_COMMAND%"
exit /b %ERRORLEVEL%

:Shadow
if /i "%~2"=="doctor" goto :ShadowDoctor
if "%~2"=="" goto :InvalidShadowCommand
set "RDP_SHADOW_SESSION_ID=%~2"
set "RDP_SHADOW_IS_CONSOLE="
if /i "%RDP_SHADOW_SESSION_ID%"=="console" set "RDP_SHADOW_IS_CONSOLE=1"
if defined RDP_SHADOW_IS_CONSOLE goto :ParseShadowOptions
for /f "delims=0123456789" %%I in ("%RDP_SHADOW_SESSION_ID%") do goto :InvalidShadowCommand
:ParseShadowOptions
set "RDP_SHADOW_CONTROL="
set "RDP_SHADOW_NO_CONSENT="
set "RDP_SHADOW_DISPLAY="
set "RDP_SHADOW_TSCON_PARAMETER="

:ParseNextShadowOption
if "%~3"=="" goto :RunShadowStart
if /i "%~3"=="--control" goto :ParseShadowControl
if /i "%~3"=="--no-consent" goto :ParseShadowNoConsent
if /i "%~3"=="--display" goto :ParseShadowDisplay
if /i "%~3"=="--tscon" goto :ParseShadowTscon
goto :InvalidShadowCommand

:ParseShadowControl
if defined RDP_SHADOW_CONTROL goto :InvalidShadowCommand
set "RDP_SHADOW_CONTROL=-Control"
shift /3
goto :ParseNextShadowOption

:ParseShadowNoConsent
if defined RDP_SHADOW_NO_CONSENT goto :InvalidShadowCommand
set "RDP_SHADOW_NO_CONSENT=-NoConsentPrompt"
shift /3
goto :ParseNextShadowOption

:ParseShadowDisplay
if not defined RDP_SHADOW_IS_CONSOLE goto :InvalidShadowCommand
if defined RDP_SHADOW_DISPLAY goto :InvalidShadowCommand
if defined RDP_SHADOW_TSCON_PARAMETER goto :InvalidShadowCommand
set "RDP_SHADOW_DISPLAY=-Display"
shift /3
goto :ParseNextShadowOption

:ParseShadowTscon
if not defined RDP_SHADOW_IS_CONSOLE goto :InvalidShadowCommand
if defined RDP_SHADOW_DISPLAY goto :InvalidShadowCommand
if defined RDP_SHADOW_TSCON_PARAMETER goto :InvalidShadowCommand
if "%~4"=="" goto :InvalidShadowCommand
for /f "delims=0123456789" %%I in ("%~4") do goto :InvalidShadowCommand
set "RDP_SHADOW_TSCON_PARAMETER=-TsconSessionId %~4"
shift /3
shift /3
goto :ParseNextShadowOption

:RunShadowStart
if not defined RDP_ENTRY_FILE goto :InvalidEntryFile
set "RDP_SHADOW_START_SCRIPT=%~dp0shadow-start.ps1"
if not exist "%RDP_SHADOW_START_SCRIPT%" (
    echo [ERROR] RDP Shadow start script not found:
    echo   "%RDP_SHADOW_START_SCRIPT%"
    exit /b 1
)

PowerShell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%RDP_SHADOW_START_SCRIPT%" -EntryFile "%RDP_ENTRY_FILE%" -SshEntryFile "%RDP_PEER_SSH_ENTRY%" -SessionId "%RDP_SHADOW_SESSION_ID%" -CommandName "%RDP_ENTRY_COMMAND%" %RDP_SHADOW_CONTROL% %RDP_SHADOW_NO_CONSENT% %RDP_SHADOW_DISPLAY% %RDP_SHADOW_TSCON_PARAMETER%
exit /b %ERRORLEVEL%

:ShadowDoctor
if not "%~3"=="" goto :InvalidShadowCommand
if not defined RDP_ENTRY_FILE goto :InvalidEntryFile
set "RDP_SHADOW_DOCTOR_SCRIPT=%~dp0shadow-doctor.ps1"
if not exist "%RDP_SHADOW_DOCTOR_SCRIPT%" (
    echo [ERROR] RDP Shadow doctor script not found:
    echo   "%RDP_SHADOW_DOCTOR_SCRIPT%"
    exit /b 1
)

PowerShell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%RDP_SHADOW_DOCTOR_SCRIPT%" -EntryFile "%RDP_ENTRY_FILE%" -SshEntryFile "%RDP_PEER_SSH_ENTRY%" -CommandName "%RDP_ENTRY_COMMAND%"
exit /b %ERRORLEVEL%

:Peer
if /i "%~2"=="shadow" goto :PeerShadow
if /i "%~2"=="psexec" goto :PeerPsExec
goto :InvalidPeerCommand

:PeerShadow
set "RDP_SHADOW_MANAGE_DRY_RUN="
set "RDP_SHADOW_MANAGE_MODE="
if /i "%~3"=="status" goto :PeerShadowStatus
if /i "%~3"=="enable" goto :PeerShadowSimple
if /i "%~3"=="restore" goto :PeerShadowSimple
if /i "%~3"=="mode" goto :PeerShadowMode
goto :InvalidPeerCommand

:PeerShadowStatus
set "RDP_SHADOW_MANAGE_ACTION=status"
if not "%~4"=="" goto :InvalidPeerCommand
goto :RunPeerShadowManage

:PeerShadowSimple
set "RDP_SHADOW_MANAGE_ACTION=%~3"
if not "%~5"=="" goto :InvalidPeerCommand
if "%~4"=="" goto :RunPeerShadowManage
if /i not "%~4"=="--dry-run" goto :InvalidPeerCommand
set "RDP_SHADOW_MANAGE_DRY_RUN=-DryRun"
goto :RunPeerShadowManage

:PeerShadowMode
set "RDP_SHADOW_MANAGE_ACTION=mode"
set "RDP_PEER_SHADOW_MODE=%~4"
if "%RDP_PEER_SHADOW_MODE%"=="" goto :InvalidPeerCommand
for /f "delims=01234" %%I in ("%RDP_PEER_SHADOW_MODE%") do goto :InvalidPeerCommand
if not "%RDP_PEER_SHADOW_MODE:~1%"=="" goto :InvalidPeerCommand
set "RDP_SHADOW_MANAGE_MODE=-Mode %RDP_PEER_SHADOW_MODE%"
if not "%~6"=="" goto :InvalidPeerCommand
if "%~5"=="" goto :RunPeerShadowManage
if /i not "%~5"=="--dry-run" goto :InvalidPeerCommand
set "RDP_SHADOW_MANAGE_DRY_RUN=-DryRun"

:RunPeerShadowManage
if not defined RDP_ENTRY_FILE goto :InvalidEntryFile
set "RDP_SHADOW_MANAGE_SCRIPT=%~dp0shadow-manage.ps1"
if not exist "%RDP_SHADOW_MANAGE_SCRIPT%" (
    echo [ERROR] RDP Shadow management script not found:
    echo   "%RDP_SHADOW_MANAGE_SCRIPT%"
    exit /b 1
)

PowerShell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%RDP_SHADOW_MANAGE_SCRIPT%" -Action "%RDP_SHADOW_MANAGE_ACTION%" %RDP_SHADOW_MANAGE_MODE% -SshEntryFile "%RDP_PEER_SSH_ENTRY%" -RdpEntryFile "%RDP_ENTRY_FILE%" -CommandName "%RDP_ENTRY_COMMAND%" %RDP_SHADOW_MANAGE_DRY_RUN%
exit /b %ERRORLEVEL%

:PeerPsExec
set "RDP_PSEXEC_ACTION="
set "RDP_PSEXEC_DRY_RUN="
set "RDP_PSEXEC_SESSION_PARAMETER="
set "RDP_PSEXEC_ARG_COUNT=0"
if /i "%~3"=="status" goto :PeerPsExecStatus
if /i "%~3"=="add" goto :PeerPsExecMutation
if /i "%~3"=="remove" goto :PeerPsExecMutation
if "%~3"=="--" goto :PeerPsExecNative
if "%~3"=="" goto :InvalidPeerCommand
for /f "delims=0123456789" %%I in ("%~3") do goto :InvalidPeerCommand
set "RDP_PSEXEC_ACTION=launch"
set "RDP_PSEXEC_SESSION_PARAMETER=-SessionId %~3"
goto :CollectPeerPsExecArguments

:PeerPsExecStatus
if not "%~4"=="" goto :InvalidPeerCommand
set "RDP_PSEXEC_ACTION=status"
goto :RunPeerPsExec

:PeerPsExecMutation
set "RDP_PSEXEC_ACTION=%~3"
if "%~4"=="" goto :RunPeerPsExec
if /i not "%~4"=="--dry-run" goto :InvalidPeerCommand
if not "%~5"=="" goto :InvalidPeerCommand
set "RDP_PSEXEC_DRY_RUN=-DryRun"
goto :RunPeerPsExec

:PeerPsExecNative
set "RDP_PSEXEC_ACTION=run"

:CollectPeerPsExecArguments
if "%~4"=="" goto :RunPeerPsExecCommand
set /a RDP_PSEXEC_ARG_COUNT+=1 >nul
set "RDP_PSEXEC_ARG_%RDP_PSEXEC_ARG_COUNT%=%~4"
shift /4
goto :CollectPeerPsExecArguments

:RunPeerPsExecCommand
if "%RDP_PSEXEC_ARG_COUNT%"=="0" goto :InvalidPeerCommand

:RunPeerPsExec
if not defined RDP_ENTRY_FILE goto :InvalidEntryFile
set "RDP_PSEXEC_SCRIPT=%~dp0psexec.ps1"
if not exist "%RDP_PSEXEC_SCRIPT%" (
    echo [ERROR] RDP peer PsExec script not found:
    echo   "%RDP_PSEXEC_SCRIPT%"
    exit /b 1
)

PowerShell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%RDP_PSEXEC_SCRIPT%" -Action "%RDP_PSEXEC_ACTION%" -SshEntryFile "%RDP_PEER_SSH_ENTRY%" -RdpEntryFile "%RDP_ENTRY_FILE%" -ArgumentCount %RDP_PSEXEC_ARG_COUNT% %RDP_PSEXEC_SESSION_PARAMETER% -CommandName "%RDP_ENTRY_COMMAND%" %RDP_PSEXEC_DRY_RUN%
exit /b %ERRORLEVEL%

:RunRdp
if not defined RDP_ENTRY_FILE goto :InvalidEntryFile
set "RDP_CONNECT_SCRIPT=%~dp0connect.ps1"
if not exist "%RDP_CONNECT_SCRIPT%" (
    echo [ERROR] RDP connection script not found:
    echo   "%RDP_CONNECT_SCRIPT%"
    exit /b 1
)

PowerShell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%RDP_CONNECT_SCRIPT%" -EntryFile "%RDP_ENTRY_FILE%" -SshEntryFile "%RDP_PEER_SSH_ENTRY%" -CommandName "%RDP_ENTRY_COMMAND%" %RDP_CLIENT_LAUNCH% %RDP_CLIENT_FORCE% %RDP_CLIENT_SESSION_PARAMETER%
exit /b %ERRORLEVEL%

:Signing
set "RDP_SIGNING_DRY_RUN="
if /i "%~2"=="status" goto :SigningStatus
if /i "%~2"=="install" goto :SigningInstall
if /i "%~2"=="remove" goto :SigningRemove
if /i "%~2"=="open" goto :SigningOpen
goto :InvalidSigningCommand

:SigningStatus
if not "%~3"=="" goto :InvalidSigningCommand
set "RDP_SIGNING_ACTION=status"
goto :RunSigning

:SigningInstall
set "RDP_SIGNING_ACTION=install"
if "%~3"=="" goto :RunSigning
if /i not "%~3"=="--dry-run" goto :InvalidSigningCommand
if not "%~4"=="" goto :InvalidSigningCommand
set "RDP_SIGNING_DRY_RUN=-DryRun"
goto :RunSigning

:SigningRemove
if not "%~3"=="" goto :InvalidSigningCommand
set "RDP_SIGNING_ACTION=remove"
goto :RunSigning

:SigningOpen
if not "%~3"=="" goto :InvalidSigningCommand
set "RDP_SIGNING_ACTION=open"

:RunSigning
set "RDP_SIGNING_SCRIPT=%~dp0signing.ps1"
if not exist "%RDP_SIGNING_SCRIPT%" (
    echo [ERROR] RDP signing script not found:
    echo   "%RDP_SIGNING_SCRIPT%"
    exit /b 1
)

PowerShell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%RDP_SIGNING_SCRIPT%" -Action "%RDP_SIGNING_ACTION%" -CommandName "%RDP_ENTRY_COMMAND%" %RDP_SIGNING_DRY_RUN%
exit /b %ERRORLEVEL%

:Hosts
set "RDP_HOSTS_UAC="
set "RDP_HOSTS_DRY_RUN="
if /i "%~2"=="status" goto :HostsStatus
if /i "%~2"=="install" goto :HostsInstall
if /i "%~2"=="remove" goto :HostsRemove
if /i "%~2"=="cleanup" goto :HostsCleanup
goto :InvalidHostsCommand

:HostsStatus
set "RDP_HOSTS_ACTION=status"
if not "%~3"=="" goto :InvalidHostsCommand
goto :RunHosts

:HostsInstall
set "RDP_HOSTS_ACTION=install"
goto :ParseHostsMutation

:HostsRemove
set "RDP_HOSTS_ACTION=remove"
goto :ParseHostsMutation

:HostsCleanup
set "RDP_HOSTS_ACTION=cleanup"
if "%~3"=="" goto :RunHosts
if not "%~4"=="" goto :InvalidHostsCommand
if /i "%~3"=="--uac" set "RDP_HOSTS_UAC=-Uac"
if /i "%~3"=="--uac" goto :RunHosts
if /i "%~3"=="--dry-run" set "RDP_HOSTS_DRY_RUN=-DryRun"
if /i "%~3"=="--dry-run" goto :RunHosts
goto :InvalidHostsCommand

:ParseHostsMutation
if "%~3"=="" goto :RunHosts
if /i not "%~3"=="--uac" goto :InvalidHostsCommand
if not "%~4"=="" goto :InvalidHostsCommand
set "RDP_HOSTS_UAC=-Uac"

:RunHosts
if not defined RDP_ENTRY_FILE goto :InvalidEntryFile
set "RDP_HOSTS_SCRIPT=%~dp0hosts.ps1"
if not exist "%RDP_HOSTS_SCRIPT%" (
    echo [ERROR] RDP hosts script not found:
    echo   "%RDP_HOSTS_SCRIPT%"
    exit /b 1
)

PowerShell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%RDP_HOSTS_SCRIPT%" -EntryFile "%RDP_ENTRY_FILE%" -Action "%RDP_HOSTS_ACTION%" -HostAlias "%RDP_HOST_ALIAS%" -CommandName "%RDP_ENTRY_COMMAND%" %RDP_HOSTS_UAC% %RDP_HOSTS_DRY_RUN%
exit /b %ERRORLEVEL%

:ShowHelp
if not "%~3"=="" goto :InvalidHelpCommand
set "RDP_HELP_SCRIPT=%~dp0help.ps1"
if not exist "%RDP_HELP_SCRIPT%" (
    echo [ERROR] RDP help script not found:
    echo   "%RDP_HELP_SCRIPT%"
    exit /b 1
)

PowerShell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%RDP_HELP_SCRIPT%" -CommandName "%RDP_ENTRY_COMMAND%" -Language "%~2"
exit /b %ERRORLEVEL%

:InvalidHelpCommand
echo [ERROR] Help usage:
echo   "%RDP_ENTRY_COMMAND% .help [zh^|en]"
exit /b 1

:InvalidRdpCommand
echo [ERROR] RDP file usage:
echo   "%RDP_ENTRY_COMMAND% .rdp create [--force]"
exit /b 1

:InvalidShadowCommand
echo [ERROR] Shadow usage:
echo   "%RDP_ENTRY_COMMAND% .shadow doctor"
echo   "%RDP_ENTRY_COMMAND% .shadow <session-id> [--control] [--no-consent]"
echo   "%RDP_ENTRY_COMMAND% .shadow console [--control] [--no-consent]"
echo   "%RDP_ENTRY_COMMAND% .shadow console --display [--control] [--no-consent]"
echo   "%RDP_ENTRY_COMMAND% .shadow console --tscon <session-id> [--control] [--no-consent]"
exit /b 1

:InvalidSessionCommand
echo [ERROR] Session usage:
echo   "%RDP_ENTRY_COMMAND% .list"
echo   "%RDP_ENTRY_COMMAND% .<session-id> [connect]"
echo   "%RDP_ENTRY_COMMAND% .<session-id> screenshot [--display] [--timeout <seconds>] [--output <absolute.png>]"
echo   "%RDP_ENTRY_COMMAND% .<session-id> pixel <x> <y> [--display] [--timeout <seconds>]"
echo   "%RDP_ENTRY_COMMAND% .<session-id> click <x> <y> [--display] [--timeout <seconds>]"
echo   "%RDP_ENTRY_COMMAND% .<session-id> script <workflow.ps1> [--display] [--timeout <seconds>]"
exit /b 1

:InvalidPeerCommand
echo [ERROR] Peer usage:
echo   "%RDP_ENTRY_COMMAND% .peer shadow status"
echo   "%RDP_ENTRY_COMMAND% .peer shadow enable [--dry-run]"
echo   "%RDP_ENTRY_COMMAND% .peer shadow mode <0-4> [--dry-run]"
echo   "%RDP_ENTRY_COMMAND% .peer shadow restore [--dry-run]"
echo   "%RDP_ENTRY_COMMAND% .peer psexec status"
echo   "%RDP_ENTRY_COMMAND% .peer psexec add [--dry-run]"
echo   "%RDP_ENTRY_COMMAND% .peer psexec remove [--dry-run]"
echo   "%RDP_ENTRY_COMMAND% .peer psexec ^<session-id^> ^<program-and-arguments^>"
echo   "%RDP_ENTRY_COMMAND% .peer psexec -- <native-arguments>"
exit /b 1

:InvalidHostsCommand
echo [ERROR] Hosts usage:
echo   "%RDP_ENTRY_COMMAND% .hosts status"
echo   "%RDP_ENTRY_COMMAND% .hosts install [--uac]"
echo   "%RDP_ENTRY_COMMAND% .hosts remove [--uac]"
echo   "%RDP_ENTRY_COMMAND% .hosts cleanup [--dry-run^|--uac]"
exit /b 1

:InvalidSigningCommand
echo [ERROR] Sign usage:
echo   "%RDP_ENTRY_COMMAND% .sign status"
echo   "%RDP_ENTRY_COMMAND% .sign install [--dry-run]"
echo   "%RDP_ENTRY_COMMAND% .sign remove"
echo   "%RDP_ENTRY_COMMAND% .sign open"
exit /b 1

:UnknownCommand
echo [ERROR] Unknown RDP command: %~1
echo Run "%RDP_ENTRY_COMMAND% .help" to view the commands currently available.
exit /b 1

:InvalidEntryProtocol
echo [ERROR] This RDP entry is missing RDP_CLIENT_PROTOCOL=1.
echo Copy Favorites\template.rdp1.cmd again or update the entry header.
exit /b 1

:InvalidEntryFile
echo [ERROR] This RDP entry did not provide RDP_ENTRY_FILE.
echo Copy Favorites\template.rdp1.cmd again or update the entry footer.
exit /b 1
