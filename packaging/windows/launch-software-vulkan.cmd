@echo off
setlocal
set "VK_DRIVER_FILES=%~dp0software-vulkan\lvp_icd.x86_64.json"
"%~dp0kit-editor.exe" %*
if errorlevel 1 (
    echo.
    echo Kit could not start. See the error above.
    pause
    exit /b 1
)
