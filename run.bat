@echo off
setlocal
title Hardware & Task Manager Monitor

set "EXE_PATH=%~dp0hw-monitor.exe"
if not exist "%EXE_PATH%" set "EXE_PATH=%~dp0target\x86_64-pc-windows-msvc\release\hw-monitor.exe"
if not exist "%EXE_PATH%" set "EXE_PATH=%~dp0target\release\hw-monitor.exe"

if not exist "%EXE_PATH%" (
    echo [ERROR] hw-monitor.exe tidak ditemukan.
    echo Jalankan perintah berikut untuk build:
    echo   cargo build --release --target x86_64-pc-windows-msvc
    echo atau:
    echo   cargo build --release
    pause
    exit /b 1
)

echo Menjalankan %EXE_PATH% ...
echo Tekan Ctrl+C di jendela ini untuk menghentikan server.
"%EXE_PATH%" %*
