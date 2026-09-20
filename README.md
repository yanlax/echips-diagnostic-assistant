# Echips Diagnostic Assistant

Каркас Tauri 2 приложения для инженеров сервисного центра — чек-лист
аппаратной диагностики ноутбука с сохранением отчёта.

Построено по конвенциям `echips-driver-assistant`: кастомный титлбар,
переключение темы, `screen`-based рендеринг, `.devlist`/`.devrow`,
`.btn-primary`/`.btn-ghost`, `escapeHtml()`/`showLoading()`/`showError()`.

## Структура

```
src/                     фронтенд (без сборки — чистый HTML/CSS/JS)
  index.html
  style.css
  theme.js                переключение тёмной/светлой темы
  titlebar.js              свернуть/развернуть/закрыть окна
  tests.js                 реестр тестов (список пунктов диагностики)
  app.js                    рендер экранов, состояние, IPC-вызовы

src-tauri/
  Cargo.toml
  tauri.conf.json
  capabilities/default.json
  .cargo/config.toml        crt-static для Windows (см. learnings)
  src/
    main.rs
    lib.rs                  регистрация команд
    powershell.rs            run_ps/run_ps_json, декодирование cp1251
    commands/
      system.rs               get_system_info — WMI Win32_ComputerSystem/BIOS
      battery.rs               get_battery_info — WMI Win32_Battery
      network.rs               get_network_adapters — Wi-Fi/Bluetooth
      stress.rs                 run_cpu_stress/stop_cpu_stress
      usb.rs                   list_usb_devices — Win32_PnPEntity USB\*
      report.rs                 save_report — сохранение .txt отчёта
```

## Что уже работает

- Определение устройства (модель, серийник, BIOS, ОС, CPU, ОЗУ)
- Клавиатура — счётчик уникальных нажатых клавиш
- Тачпад — визуальный тест рисованием
- Дисплей — заливка сплошными цветами на весь экран для поиска дефектов
- Камера — live-превью через `getUserMedia`, обработка отказа в доступе
- Звук — запись 5 с с микрофона и воспроизведение (`MediaRecorder`)
- USB-порты — список устройств (`list_usb_devices`, Win32_PnPEntity) + ручная отметка по портам
- Wi-Fi/Bluetooth — статус адаптеров (`get_network_adapters`, Win32_NetworkAdapter) + ручная отметка
- Стресс-тест CPU — нагрузка на все ядра (`run_cpu_stress`), прогресс-бар, скорость по секундам, отметка pass/fail
- Батарея — заряд/статус, design и full charge capacity, health%, циклы (XML из `powercfg /batteryreport`)
- Итоговый отчёт — сводка по всем пунктам + сохранение в
  `%APPDATA%/ru.echips.diagnostic-assistant/reports/*.txt`

## Что оформлено как заглушка (ручная отметка Исправно/Неисправно/Пропустить)

отпечаток пальца, температуры/кулер — сами экраны в навигации уже есть,
но без автоматической проверки. Реализуются по одному: добавить рендер-функцию в `app.js` и
зарегистрировать в объекте `RENDERERS`, при необходимости — новую
Tauri-команду в `src-tauri/src/commands/`.

## Известные TODO

- `logo.png` — нужно скопировать из `echips-driver-assistant`
- Иконки для бандла (`src-tauri/icons/`) — скопировать оттуда же или
  сгенерировать через `tauri icon`
- Температуры/обороты кулера — нет прямого WMI-источника на всех платах;
  вероятно потребуется внешняя утилита (LibreHardwareMonitor CLI/DLL) или
  чтение через ACPI, если Echips-платы это поддерживают
- Стресс-тест: мониторинг температур/частот во время нагрузки (зависит от задачи по сенсорам)

## Сборка

Требуется Rust + Tauri CLI, Node не нужен (фронтенд без сборки).

```
cargo tauri dev
cargo tauri build
```
