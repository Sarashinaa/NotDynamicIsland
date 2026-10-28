@echo off
title Membangun Project Nova
echo Membangun biner mandiri rilis Project Nova...

where cargo >nul 2>&1
if %errorlevel% neq 0 (
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)

cargo build --release
echo.
echo Selesai! Biner mandiri berada di: target\release\not_dynamic_island.exe
pause
