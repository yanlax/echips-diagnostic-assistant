<#
  Сборка образа WinPE (Windows 11 ADK) с нашим приложением и всем, что нужно ему для работы.
  Запускать на обычной Windows-машине ОТ АДМИНИСТРАТОРА, где установлены:
    - Windows ADK (Assessment and Deployment Kit) и
    - Windows PE add-on for the ADK  (оба — с сайта Microsoft, версии Windows 11).

  Пример:
    powershell -ExecutionPolicy Bypass -File build-winpe.ps1 `
      -AppExe "C:\Echips\Echips-Hardware-Check-v0.30.0.exe" `
      -WebView2Dir "C:\Echips\Microsoft.WebView2.FixedVersionRuntime.153.0.4234.48.x64" `
      -Drivers "C:\Drivers" -Iso "C:\Echips\Echips-WinPE.iso"

  Что делает: copype -> монтирует boot.wim -> добавляет компоненты WinPE (WMI, NetFX, PowerShell,
  StorageWMI, DismCmdlets, шрифты, Wi-Fi и др.) -> кладёт приложение и WebView2 в X:\Echips ->
  подменяет startnet.cmd -> (по желанию) добавляет драйверы -> сохраняет образ -> собирает ISO.
  ВНИМАНИЕ: не проверялось на реальной сборке (написано по документации ADK) — при ошибке скрипт
  остановится с сообщением, смонтированный образ отмонтируйте вручную: dism /Unmount-Image /MountDir:<..>\mount /Discard
#>
param(
  [Parameter(Mandatory)] [string]$AppExe,
  [Parameter(Mandatory)] [string]$WebView2Dir,
  [string]$WorkDir = "C:\WinPE_Echips",
  [string]$Drivers = "",
  [string]$Iso = "C:\Echips-WinPE.iso"
)
$ErrorActionPreference = "Stop"

$adk = "${env:ProgramFiles(x86)}\Windows Kits\10\Assessment and Deployment Kit"
$peRoot = "$adk\Windows Preinstallation Environment"
$oc = "$peRoot\amd64\WinPE_OCs"
$setenv = "$adk\Deployment Tools\DandISetEnv.bat"
foreach ($p in @($peRoot, $oc, $setenv, $AppExe, $WebView2Dir)) {
  if (-not (Test-Path $p)) { throw "Не найдено: $p (установлены ли ADK и WinPE add-on?)" }
}
if (-not (Test-Path (Join-Path $WebView2Dir "msedgewebview2.exe"))) { throw "В $WebView2Dir нет msedgewebview2.exe — укажите папку распакованного Fixed Version Runtime." }

if (Test-Path $WorkDir) { throw "$WorkDir уже существует — удалите или укажите другой -WorkDir." }

# 1. Каркас образа (copype нужно запускать из среды ADK)
cmd /c "`"$setenv`" && copype amd64 `"$WorkDir`""
if ($LASTEXITCODE -ne 0) { throw "copype завершился с ошибкой" }

$wim = "$WorkDir\media\sources\boot.wim"
$mount = "$WorkDir\mount"
dism /Mount-Image /ImageFile:$wim /Index:1 /MountDir:$mount
if ($LASTEXITCODE -ne 0) { throw "Не удалось смонтировать boot.wim" }

try {
  # 2. Компоненты WinPE. Порядок важен: PowerShell требует WMI, NetFX и Scripting.
  $packages = @(
    "WinPE-WMI", "WinPE-NetFX", "WinPE-Scripting", "WinPE-PowerShell",
    "WinPE-StorageWMI", "WinPE-DismCmdlets", "WinPE-EnhancedStorage",
    "WinPE-FontSupport-WinRE", "WinPE-Dot3Svc", "WinPE-WiFi-Package"
  )
  foreach ($name in $packages) {
    $cab = "$oc\$name.cab"
    if (-not (Test-Path $cab)) { Write-Warning "Пропуск (нет в этом ADK): $name"; continue }
    dism /Image:$mount /Add-Package /PackagePath:$cab | Out-Null
    $lp = "$oc\en-us\${name}_en-us.cab"
    if (Test-Path $lp) { dism /Image:$mount /Add-Package /PackagePath:$lp | Out-Null }
    Write-Host "Добавлено: $name"
  }

  # 3. Приложение и переносимый WebView2 внутрь образа (X:\Echips). Внимание: всё, что внутри
  #    boot.wim, при загрузке целиком уходит в ОЗУ — WebView2 ~0.5 ГБ, нужна память с запасом.
  #    Если ОЗУ мало — не копируйте WebView2 сюда, а положите папку \Echips на USB-раздел
  #    (startnet.cmd ищет приложение и там; winpe.rs находит WebView2 рядом с exe).
  $dst = "$mount\Echips"
  New-Item -ItemType Directory -Force $dst | Out-Null
  Copy-Item $AppExe "$dst\Echips-Hardware-Check.exe" -Force
  Copy-Item $WebView2Dir "$dst\$(Split-Path $WebView2Dir -Leaf)" -Recurse -Force

  # 4. Автозапуск приложения при загрузке
  Copy-Item "$PSScriptRoot\startnet.cmd" "$mount\Windows\System32\startnet.cmd" -Force

  # 5. Драйверы (сеть, Wi-Fi, NVMe/RAID) — по желанию
  if ($Drivers -and (Test-Path $Drivers)) {
    dism /Image:$mount /Add-Driver /Driver:$Drivers /Recurse | Out-Null
    Write-Host "Драйверы добавлены из $Drivers"
  }

  # 6. Место для временных файлов в ОЗУ (максимум 512 МБ)
  dism /Image:$mount /Set-ScratchSpace:512 | Out-Null
}
catch {
  dism /Unmount-Image /MountDir:$mount /Discard | Out-Null
  throw
}

dism /Unmount-Image /MountDir:$mount /Commit
if ($LASTEXITCODE -ne 0) { throw "Не удалось сохранить образ" }

# 7. ISO (для флешки можно вместо этого: MakeWinPEMedia /UFD $WorkDir E:)
cmd /c "`"$setenv`" && MakeWinPEMedia /ISO `"$WorkDir`" `"$Iso`""
if ($LASTEXITCODE -ne 0) { throw "MakeWinPEMedia завершился с ошибкой" }
Write-Host "Готово: $Iso"
