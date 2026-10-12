@echo off
title League Unlagged Launcher
cd /d "%~dp0"
if exist "target\release\league-unlagged.exe" (
    powershell -NoProfile -Command "Start-Process -FilePath '%~dp0target\release\league-unlagged.exe' -WorkingDirectory '%~dp0' -Verb RunAs"
) else (
    echo Compiling League Unlagged release binary...
    cargo build --release
    powershell -NoProfile -Command "Start-Process -FilePath '%~dp0target\release\league-unlagged.exe' -WorkingDirectory '%~dp0' -Verb RunAs"
)
