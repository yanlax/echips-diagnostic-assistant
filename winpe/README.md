# Свой образ WinPE для Echips Hardware Check

Скрипт `build-winpe.ps1` собирает загрузочный WinPE (Windows 11 ADK) с нужными компонентами,
нашим приложением и переносимым WebView2; `startnet.cmd` запускает приложение при загрузке.

## Что нужно
1. Обычная Windows-машина, PowerShell **от администратора**.
2. **Windows ADK** и **Windows PE add-on for the ADK** (Windows 11), с сайта Microsoft.
3. `Echips-Hardware-Check-vX.Y.Z.exe` (релиз) и распакованный **WebView2 Fixed Version Runtime**
   (папка с `msedgewebview2.exe`, см. README → «Работа в WinPE»).
4. По желанию — папка с драйверами (сеть, Wi-Fi, NVMe/RAID).

## Запуск
```
powershell -ExecutionPolicy Bypass -File winpe\build-winpe.ps1 `
  -AppExe "C:\Echips\Echips-Hardware-Check-v0.30.0.exe" `
  -WebView2Dir "C:\Echips\Microsoft.WebView2.FixedVersionRuntime.153.0.4234.48.x64" `
  -Drivers "C:\Drivers" -Iso "C:\Echips\Echips-WinPE.iso"
```
ISO пишется на флешку (Rufus/Ventoy) либо сразу флешкой: `MakeWinPEMedia /UFD C:\WinPE_Echips E:`.

## Состав образа
WinPE-WMI, NetFX, Scripting, PowerShell, StorageWMI, DismCmdlets, EnhancedStorage, шрифты,
Dot3Svc, Wi-Fi. Приложение — в `X:\Echips`. Всё, что внутри boot.wim, при загрузке уходит в ОЗУ
(WebView2 ≈ 0,5 ГБ) — нужна машина с запасом памяти; иначе положите папку `\Echips` на USB-раздел
(`startnet.cmd` ищет её на дисках C–K).

## Оговорки
- Скрипт написан по документации ADK и **не запускался** — при первой сборке возможны правки.
- Microsoft официально не заявляет поддержку WebView2 в WinPE. Диагностика запуска — `echips-startup.log`
  (рядом с exe и в `%TEMP%`), в нём же сторож пишет процессы и папку данных, если страница не загрузилась.
- После выхода из приложения открывается командная строка — можно посмотреть логи.
