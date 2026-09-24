# Сторонние компоненты

Echips Hardware Check — внутренний инструмент инженеров сервисного центра Echips.
В сборку вшиты следующие сторонние компоненты (без изменений).

## LibreHardwareMonitorLib
- Назначение: чтение температур, оборотов вентиляторов, напряжений и мощности.
- Лицензия: Mozilla Public License 2.0 (MPL-2.0).
- Исходники: https://github.com/LibreHardwareMonitor/LibreHardwareMonitor
- Используется версия 0.9.6 из NuGet (`LibreHardwareMonitorLib`) вместе с её
  зависимостями (HidSharp, RAMSPDToolkit-NDD, DiskInfoToolkit и др. — их лицензии
  перечислены в `THIRD-PARTY-NOTICES.txt` пакета). Вспомогательный процесс
  `echips-sensors.exe` (исходники — `helper/EchipsSensors/`) собирается в CI.

## PawnIO (драйвер и установщик)
- Назначение: драйвер ядра для доступа к регистрам процессора/платы; без него
  LibreHardwareMonitor не отдаёт температуру и мощность CPU.
- Лицензия драйвера: GNU General Public License v2.0.
- Исходники драйвера: https://github.com/namazso/PawnIO
- Установщик: https://github.com/namazso/PawnIO.Setup, релиз 2.2.0,
  `PawnIO_setup.exe`, SHA-256
  `1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032`
  (скачивается в CI и сверяется по контрольной сумме, см. `.github/workflows/build.yml`).
- Установщик вшит в exe без изменений и запускается автоматически при
  первом запуске приложения (`-install -silent`); кнопка «Удалить драйвер
  PawnIO» на вкладке «Датчики» выполняет `-uninstall -silent` и отключает
  автоустановку.
- Программа предназначена для внутреннего использования инженерами. При передаче
  сборки третьим лицам соблюдайте условия GPL-2.0 (предоставление исходников
  драйвера — ссылка выше).

## Утилиты записи SMBIOS (заводской комплект)
- Назначение: запись серийного номера/UUID платы при замене (вкладка «Замена
  платы»): `Amidewin.exe` и драйверы `amifldrv64.sys`/`amifldrv32.sys`
  (American Megatrends, платы AMI), `H2OSDE-Wx64.exe` (Insyde Software, платы
  Insyde). Лежат в `src-tauri/assets/smbios`, вшиты в exe без изменений и
  распаковываются в `%LOCALAPPDATA%\Echips\HardwareCheck\smbios` при первой
  записи.
- Происхождение: тестовый комплект завода-разработчика, присланный сервисом;
  разрешение на вшивание и распространение в составе приложения дали
  разработчики завода. Правообладатели утилит — American Megatrends Inc. и
  Insyde Software Corp.
- Драйвер AMI 2014 года; современный Windows (Защитник/HVCI) может отказаться
  его загружать — тогда запись на плате с AMI не сработает.
