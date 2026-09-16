@echo off
rem MP14Tools - build entry point that always works.
rem
rem build.ps1 is a PowerShell script, so it inherits the machine's execution
rem policy. On a machine where that policy forbids scripts, ".\build.ps1" fails
rem with "running scripts is disabled on this system" before it does anything.
rem Calling it with -ExecutionPolicy Bypass from a batch file avoids that without
rem changing any machine-wide setting: the relaxation applies to this one
rem process only.
rem
rem Usage (identical switches to build.ps1):
rem   build.cmd              debug build
rem   build.cmd -Release     release build
rem   build.cmd -Check       type-check only
rem   build.cmd -Run         debug build + run
rem   build.cmd -KeepRunning do not stop a running instance first

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build.ps1" %*
exit /b %ERRORLEVEL%
