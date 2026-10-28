@echo off
title Project Nova - Dynamic Island
where cargo >nul 2>&1
if %errorlevel% neq 0 (
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)

if exist "target\release\not_dynamic_island.exe" (
    echo Menjalankan biner rilis NotDynamicIsland...
    start "" "target\release\not_dynamic_island.exe"
) else (
    echo Mengompilasi dan menjalankan Project Nova...
    cargo run --release
)
