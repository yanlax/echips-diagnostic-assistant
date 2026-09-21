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
- Установщик вшит в exe без изменений и запускается только по кнопке
  «Установить драйвер PawnIO» на вкладке «Датчики» (`-install -silent`);
  кнопка «Удалить драйвер PawnIO» выполняет `-uninstall -silent`.
- Программа предназначена для внутреннего использования инженерами. При передаче
  сборки третьим лицам соблюдайте условия GPL-2.0 (предоставление исходников
  драйвера — ссылка выше).
