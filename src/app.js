// Echips Diagnostic Assistant — основной рендер экранов.
// Конвенции соблюдены из echips-driver-assistant: глобальная переменная
// `screen`, invoke()/listen() для Tauri IPC, escapeHtml()/showLoading()/showError().

var invoke = window.__TAURI__.core.invoke;

var screen = document.getElementById("screen");
var navEl = document.getElementById("test-nav");
var progressFill = document.getElementById("progress-fill");
var progressLabel = document.getElementById("progress-label");

var state = {
  current: "device",
  deviceInfo: null,
  engineer: "",
  startedAt: new Date().toISOString(),
  results: {} // id -> { status: "not_run"|"pass"|"fail"|"skipped", note: string }
};

window.ECHIPS_TESTS.forEach(function (t) {
  state.results[t.id] = { status: "not_run", note: "" };
});

function escapeHtml(str) {
  if (str === null || str === undefined) return "";
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function showLoading(msg) {
  screen.innerHTML =
    '<div class="loading"><div class="spinner"></div><div class="loading-text">' +
    escapeHtml(msg || "Загрузка...") +
    "</div></div>";
}

function showError(msg) {
  screen.innerHTML =
    '<div class="error-box"><div class="error-title">Ошибка</div><div class="error-text">' +
    escapeHtml(msg) +
    "</div></div>";
}

function statusDotClass(status) {
  switch (status) {
    case "pass": return "dot dot-pass";
    case "fail": return "dot dot-fail";
    case "skipped": return "dot dot-skip";
    default: return "dot";
  }
}

function setStatus(id, status, note) {
  state.results[id] = { status: status, note: note || "" };
  renderNav();
  updateProgress();
}

function updateProgress() {
  var testable = window.ECHIPS_TESTS.filter(function (t) { return !t.isReport; });
  var done = testable.filter(function (t) {
    return state.results[t.id].status !== "not_run";
  }).length;
  var pct = testable.length ? Math.round((done / testable.length) * 100) : 0;
  progressFill.style.width = pct + "%";
  progressLabel.textContent = done + " / " + testable.length + " проверок";
}

function renderNav() {
  var groups = {};
  var order = [];
  window.ECHIPS_TESTS.forEach(function (t) {
    if (!groups[t.group]) { groups[t.group] = []; order.push(t.group); }
    groups[t.group].push(t);
  });

  var html = "";
  order.forEach(function (groupName) {
    html += '<div class="nav-group-label">' + escapeHtml(groupName) + "</div>";
    groups[groupName].forEach(function (t) {
      var res = state.results[t.id];
      var active = t.id === state.current ? " active" : "";
      html +=
        '<div class="step' + active + '" data-test="' + t.id + '">' +
        '<span class="' + statusDotClass(res.status) + '"></span>' +
        escapeHtml(t.title) +
        "</div>";
    });
  });
  navEl.innerHTML = html;

  Array.prototype.forEach.call(navEl.querySelectorAll(".step"), function (el) {
    el.addEventListener("click", function () {
      goTo(el.getAttribute("data-test"));
    });
  });
}

function goTo(id) {
  state.current = id;
  renderNav();
  var test = window.ECHIPS_TESTS.filter(function (t) { return t.id === id; })[0];
  if (!test) { showError("Неизвестный экран: " + id); return; }

  var renderer = RENDERERS[id];
  if (renderer) {
    renderer();
  } else {
    renderManualScreen(test);
  }
}

// ---- Заглушка для ещё не реализованных тестов ----
function renderManualScreen(test) {
  var res = state.results[test.id];
  screen.innerHTML =
    '<div class="test-screen">' +
    "<h2>" + escapeHtml(test.title) + "</h2>" +
    '<p class="hint">Автоматическая проверка для этого пункта пока не реализована — заполните результат вручную по итогам осмотра.</p>' +
    '<textarea id="manual-note" class="note-input" placeholder="Комментарий (необязательно)">' + escapeHtml(res.note) + "</textarea>" +
    '<div class="btn-row">' +
    '<button class="btn-primary" id="btn-pass">Исправно</button>' +
    '<button class="btn-danger" id="btn-fail">Неисправно</button>' +
    '<button class="btn-ghost" id="btn-skip">Пропустить</button>' +
    "</div>" +
    "</div>";

  document.getElementById("btn-pass").addEventListener("click", function () {
    setStatus(test.id, "pass", document.getElementById("manual-note").value);
  });
  document.getElementById("btn-fail").addEventListener("click", function () {
    setStatus(test.id, "fail", document.getElementById("manual-note").value);
  });
  document.getElementById("btn-skip").addEventListener("click", function () {
    setStatus(test.id, "skipped", document.getElementById("manual-note").value);
  });
}

// ---- Экран "Устройство" ----
function renderDeviceScreen() {
  showLoading("Определение устройства...");
  invoke("get_system_info")
    .then(function (info) {
      state.deviceInfo = info;
      screen.innerHTML =
        '<div class="test-screen">' +
        "<h2>Устройство</h2>" +
        '<div class="devlist">' +
        devRow("Производитель", info.manufacturer) +
        devRow("Модель", info.model) +
        devRow("Серийный номер", info.serial_number) +
        devRow("BIOS", info.bios_version) +
        devRow("ОС", info.os_version) +
        devRow("Процессор", info.cpu) +
        devRow("ОЗУ", info.ram_total_gb + " ГБ") +
        "</div>" +
        '<label class="field-label">Инженер</label>' +
        '<input id="engineer-input" class="text-input" placeholder="ФИО инженера" value="' + escapeHtml(state.engineer) + '">' +
        '<div class="btn-row">' +
        '<button class="btn-primary" id="btn-confirm-device">Подтвердить и продолжить</button>' +
        "</div>" +
        "</div>";

      document.getElementById("engineer-input").addEventListener("input", function (e) {
        state.engineer = e.target.value;
      });
      document.getElementById("btn-confirm-device").addEventListener("click", function () {
        setStatus("device", "pass", info.model + " / " + info.serial_number);
        goToNext("device");
      });
    })
    .catch(function (err) {
      showError(typeof err === "string" ? err : "Не удалось получить информацию об устройстве");
    });
}

function devRow(label, value) {
  return (
    '<div class="devrow"><span class="devrow-label">' +
    escapeHtml(label) +
    '</span><span class="devrow-value">' +
    escapeHtml(value) +
    "</span></div>"
  );
}

// ---- Экран "Клавиатура" ----
function renderKeyboardScreen() {
  var pressed = {};
  screen.innerHTML =
    '<div class="test-screen">' +
    "<h2>Клавиатура</h2>" +
    '<p class="hint">Нажмите последовательно все клавиши — нажатые подсветятся зелёным. Esc — выйти из режима теста.</p>' +
    '<div class="keylog" id="keylog">Клавиш нажато: 0</div>' +
    '<div class="btn-row">' +
    '<button class="btn-primary" id="btn-pass">Всё работает</button>' +
    '<button class="btn-danger" id="btn-fail">Есть нерабочие клавиши</button>' +
    "</div>" +
    "</div>";

  function onKeydown(e) {
    pressed[e.code] = true;
    document.getElementById("keylog").textContent =
      "Клавиш нажато: " + Object.keys(pressed).length + " (последняя: " + e.code + ")";
  }
  window.addEventListener("keydown", onKeydown);

  document.getElementById("btn-pass").addEventListener("click", function () {
    window.removeEventListener("keydown", onKeydown);
    setStatus("keyboard", "pass", Object.keys(pressed).length + " клавиш проверено");
  });
  document.getElementById("btn-fail").addEventListener("click", function () {
    window.removeEventListener("keydown", onKeydown);
    setStatus("keyboard", "fail", "");
  });
}

// ---- Экран "Тачпад" ----
function renderTouchpadScreen() {
  screen.innerHTML =
    '<div class="test-screen">' +
    "<h2>Тачпад</h2>" +
    '<p class="hint">Проведите курсором по области ниже — след должен рисоваться плавно, без разрывов.</p>' +
    '<canvas id="touchpad-canvas" class="touchpad-canvas"></canvas>' +
    '<div class="btn-row">' +
    '<button class="btn-ghost" id="btn-clear">Очистить</button>' +
    '<button class="btn-primary" id="btn-pass">Исправно</button>' +
    '<button class="btn-danger" id="btn-fail">Неисправно</button>' +
    "</div>" +
    "</div>";

  var canvas = document.getElementById("touchpad-canvas");
  var ctx = canvas.getContext("2d");
  function resize() {
    canvas.width = canvas.clientWidth;
    canvas.height = canvas.clientHeight;
  }
  resize();
  window.addEventListener("resize", resize);

  var drawing = false;
  canvas.addEventListener("pointerdown", function (e) { drawing = true; draw(e); });
  canvas.addEventListener("pointermove", function (e) { if (drawing) draw(e); });
  window.addEventListener("pointerup", function () { drawing = false; });

  function draw(e) {
    var rect = canvas.getBoundingClientRect();
    ctx.fillStyle = "#4caf7d";
    ctx.beginPath();
    ctx.arc(e.clientX - rect.left, e.clientY - rect.top, 3, 0, Math.PI * 2);
    ctx.fill();
  }

  document.getElementById("btn-clear").addEventListener("click", function () {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
  });
  document.getElementById("btn-pass").addEventListener("click", function () {
    setStatus("touchpad", "pass", "");
  });
  document.getElementById("btn-fail").addEventListener("click", function () {
    setStatus("touchpad", "fail", "");
  });
}

// ---- Экран "Дисплей" ----
function renderDisplayScreen() {
  var colors = ["#ffffff", "#000000", "#ff0000", "#00ff00", "#0000ff", "#808080"];
  var idx = 0;

  function paint() {
    screen.innerHTML =
      '<div class="display-test" style="background:' + colors[idx] + '">' +
      '<div class="display-test-controls">' +
      '<button class="btn-ghost" id="btn-prev">◀</button>' +
      '<span>' + (idx + 1) + " / " + colors.length + "</span>" +
      '<button class="btn-ghost" id="btn-next">▶</button>' +
      '<button class="btn-primary" id="btn-pass">Без дефектов</button>' +
      '<button class="btn-danger" id="btn-fail">Есть дефекты (битые пиксели/подсветка)</button>' +
      "</div>" +
      "</div>";

    document.getElementById("btn-prev").addEventListener("click", function () {
      idx = (idx - 1 + colors.length) % colors.length; paint();
    });
    document.getElementById("btn-next").addEventListener("click", function () {
      idx = (idx + 1) % colors.length; paint();
    });
    document.getElementById("btn-pass").addEventListener("click", function () {
      setStatus("display", "pass", "");
    });
    document.getElementById("btn-fail").addEventListener("click", function () {
      setStatus("display", "fail", "");
    });
  }
  paint();
}

// ---- Экран "Батарея" ----
function renderBatteryScreen() {
  showLoading("Опрос контроллера батареи...");
  invoke("get_battery_info")
    .then(function (info) {
      if (!info.present) {
        screen.innerHTML =
          '<div class="test-screen"><h2>Батарея</h2><p class="hint">Батарея не обнаружена системой.</p>' +
          '<div class="btn-row"><button class="btn-danger" id="btn-fail">Отметить как неисправность</button>' +
          '<button class="btn-ghost" id="btn-skip">Пропустить (устройство без батареи)</button></div></div>';
        document.getElementById("btn-fail").addEventListener("click", function () {
          setStatus("battery", "fail", "Не обнаружена");
        });
        document.getElementById("btn-skip").addEventListener("click", function () {
          setStatus("battery", "skipped", "");
        });
        return;
      }

      screen.innerHTML =
        '<div class="test-screen">' +
        "<h2>Батарея</h2>" +
        '<div class="devlist">' +
        devRow("Заряд", info.charge_percent + " %") +
        devRow("Статус", info.charging ? "Заряжается" : "От батареи") +
        "</div>" +
        '<p class="hint">Точный износ (design vs full charge capacity) — TODO: разбор powercfg /batteryreport.</p>' +
        '<div class="btn-row">' +
        '<button class="btn-primary" id="btn-pass">Исправно</button>' +
        '<button class="btn-danger" id="btn-fail">Неисправно</button>' +
        "</div>" +
        "</div>";

      document.getElementById("btn-pass").addEventListener("click", function () {
        setStatus("battery", "pass", info.charge_percent + "%");
      });
      document.getElementById("btn-fail").addEventListener("click", function () {
        setStatus("battery", "fail", "");
      });
    })
    .catch(function (err) {
      showError(typeof err === "string" ? err : "Не удалось опросить батарею");
    });
}

// ---- Экран "Отчёт" ----
function renderReportScreen() {
  var testable = window.ECHIPS_TESTS.filter(function (t) { return !t.isReport; });
  var rows = testable.map(function (t) {
    var res = state.results[t.id];
    return devRow(t.title, statusLabelRu(res.status) + (res.note ? " — " + res.note : ""));
  }).join("");

  var failedCount = testable.filter(function (t) { return state.results[t.id].status === "fail"; }).length;

  screen.innerHTML =
    '<div class="test-screen">' +
    "<h2>Итоговый отчёт</h2>" +
    '<div class="devlist">' + rows + "</div>" +
    '<p class="hint">' +
    (failedCount === 0 ? "Неисправностей не выявлено." : "Выявлено неисправностей: " + failedCount) +
    "</p>" +
    '<div class="btn-row">' +
    '<button class="btn-primary" id="btn-save">Сохранить отчёт</button>' +
    "</div>" +
    '<div id="save-result"></div>' +
    "</div>";

  document.getElementById("btn-save").addEventListener("click", function () {
    var payload = {
      device_model: state.deviceInfo ? (state.deviceInfo.manufacturer + " " + state.deviceInfo.model) : "Неизвестно",
      device_serial: state.deviceInfo ? state.deviceInfo.serial_number : "",
      engineer: state.engineer || "",
      started_at: state.startedAt,
      finished_at: new Date().toISOString(),
      results: testable.map(function (t) {
        var res = state.results[t.id];
        return { id: t.id, title: t.title, status: res.status, note: res.note };
      })
    };

    invoke("save_report", { report: payload })
      .then(function (path) {
        document.getElementById("save-result").innerHTML =
          '<div class="success-box">Отчёт сохранён: ' + escapeHtml(path) + "</div>";
      })
      .catch(function (err) {
        document.getElementById("save-result").innerHTML =
          '<div class="error-box"><div class="error-text">' + escapeHtml(err) + "</div></div>";
      });
  });
}

function statusLabelRu(status) {
  switch (status) {
    case "pass": return "Исправно";
    case "fail": return "Неисправно";
    case "skipped": return "Пропущено";
    default: return "Не проверено";
  }
}

function goToNext(currentId) {
  var ids = window.ECHIPS_TESTS.map(function (t) { return t.id; });
  var i = ids.indexOf(currentId);
  if (i >= 0 && i + 1 < ids.length) {
    goTo(ids[i + 1]);
  }
}

var RENDERERS = {
  device: renderDeviceScreen,
  keyboard: renderKeyboardScreen,
  touchpad: renderTouchpadScreen,
  display: renderDisplayScreen,
  battery: renderBatteryScreen,
  report: renderReportScreen
};

// ---- Кнопка сайта ----
var siteLink = document.getElementById("site-link");
if (siteLink) {
  siteLink.addEventListener("click", function () {
    if (window.__TAURI__ && window.__TAURI__.shell) {
      window.__TAURI__.shell.open("https://echips.ru");
    }
  });
}

// ---- Инициализация ----
renderNav();
updateProgress();
goTo("device");
