@echo off
chcp 65001 >nul
cd /d "%~dp0"
echo ============================================
echo   账号管家 AccountHub - 本地启动
echo ============================================
echo.

REM 优先用 PATH 里的 python，找不到则用 3.13 全路径
where python >nul 2>nul
if %errorlevel%==0 (
    set PY=python
) else (
    set PY=D:\Program\Python\3.13.0\python.exe
)

echo 正在启动服务: http://127.0.0.1:8756/
echo 浏览器会自动打开；关闭本窗口即停止服务。
echo.
%PY% server.py %*
pause
