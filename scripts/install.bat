@echo off
title rindex Installer

echo.
echo   ========================================
echo      rindex Installer
echo   ========================================
echo.
echo   [1/3] Removing old version...
taskkill /f /im rindex.exe >nul 2>&1
del "%USERPROFILE%\.local\bin\rindex.exe" >nul 2>&1
echo        Old version cleaned.

echo.
echo   [2/3] Installing binary and model...
mkdir "%USERPROFILE%\.local\bin" >nul 2>&1
copy /Y "rindex.exe" "%USERPROFILE%\.local\bin\rindex.exe" >nul 2>&1
if errorlevel 1 (
    echo        [FAIL] Could not install rindex.exe
    pause
    exit /b 1
)
echo        [OK] %USERPROFILE%\.local\bin\rindex.exe

if exist "model\c2llm-static-256\token_embeddings.safetensors" (
    mkdir "%APPDATA%\rindex\models\c2llm-static-256" >nul 2>&1
    xcopy /E /Y "model\c2llm-static-256\*" "%APPDATA%\rindex\models\c2llm-static-256\" >nul 2>&1
    echo        [OK] model installed ^(~166MB^)
) else (
    echo        [--] model not bundled ^(run 'python scripts/distill.py' to build^)
)

:: Add to PATH
echo %PATH% | findstr /C:".local\bin" >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    setx PATH "%PATH%;%USERPROFILE%\.local\bin" >nul 2>&1
)

echo.
echo   [3/3] Verifying...
"%USERPROFILE%\.local\bin\rindex.exe" --version 2>nul
if errorlevel 1 (
    echo        [WARN] Restart terminal and try again
) else (
    echo        [OK] rindex is working
)

echo.
echo   ========================================
echo      Installation complete.
echo   ========================================
echo.
pause
