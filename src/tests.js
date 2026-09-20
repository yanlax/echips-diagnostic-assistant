// Реестр тестов диагностики. Каждый пункт — id, заголовок для сайдбара и
// категория (для группировки). Рендер-функция экрана ищется в app.js по id
// в объекте RENDERERS; если её нет — показывается заглушка с ручными
// кнопками "Исправно / Неисправно / Пропустить" (не блокирует workflow,
// пока конкретный тест не реализован).
window.ECHIPS_TESTS = [
  { id: "device", title: "Устройство", group: "Общее", manual: false },
  { id: "keyboard", title: "Клавиатура", group: "Ввод", manual: false },
  { id: "touchpad", title: "Тачпад", group: "Ввод", manual: false },
  { id: "display", title: "Дисплей", group: "Видео", manual: false },
  { id: "camera", title: "Камера", group: "Видео", manual: false },
  { id: "audio", title: "Звук", group: "Аудио", manual: false },
  { id: "usb", title: "USB-порты", group: "Порты", manual: true },
  { id: "wifi_bt", title: "Wi-Fi / Bluetooth", group: "Сеть", manual: true },
  { id: "fingerprint", title: "Отпечаток пальца", group: "Сенсоры", manual: true },
  { id: "battery", title: "Батарея", group: "Питание", manual: false },
  { id: "sensors", title: "Температуры / кулер", group: "Сенсоры", manual: true },
  { id: "stress", title: "Стресс-тест", group: "Нагрузка", manual: true },
  { id: "report", title: "Отчёт", group: "Итог", manual: false, isReport: true }
];
