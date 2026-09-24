@echo off
rem Echips WinPE autostart. Copied to Windows\System32\startnet.cmd of the image by build-winpe.ps1.
wpeinit
rem Сеть (DHCP) поднимает wpeinit; Wi-Fi — отдельно (см. winpe\README.md).
rem Приложение и переносимый WebView2 лежат в X:\Echips (внутри образа) либо на USB-разделе ECHIPS.
set APPDIR=X:\Echips
if not exist "%APPDIR%\Echips-Hardware-Check.exe" (
  for %%D in (C D E F G H I J K) do if exist "%%D:\Echips\Echips-Hardware-Check.exe" set APPDIR=%%D:\Echips
)
if not exist "%APPDIR%\Echips-Hardware-Check.exe" (
  echo Echips-Hardware-Check.exe not found in X:\Echips or in \Echips on drives C-K.
  cmd
  goto :eof
)
cd /d "%APPDIR%"
start "" /wait "%APPDIR%\Echips-Hardware-Check.exe"
rem После выхода из приложения — командная строка, чтобы можно было посмотреть echips-startup.log.
cmd
