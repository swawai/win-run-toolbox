@echo off
chcp 65001 >nul <nul
setlocal DisableDelayedExpansion
if "%~1"=="-h" goto :ShowHelp
if "%~1"=="--help" goto :ShowHelp
if "%~1"=="/?" goto :ShowHelp

PowerShell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0kit.ps1" %*
exit /b %ERRORLEVEL%

:ShowHelp
set "commandName=%~2"
if not defined commandName set "commandName=remote_kit"
PowerShell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0help.ps1" -CommandName "%commandName%"
exit /b %ERRORLEVEL%
