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

// Освобождение камеры/микрофона при уходе с экрана (задаётся рендерерами)
var cleanupScreen = null;

function goTo(id) {
  if (cleanupScreen) { cleanupScreen(); cleanupScreen = null; }
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

// ---- Общие для камеры/микрофона ----
function mediaErrorText(err, device) {
  var name = err && err.name;
  if (name === "NotAllowedError" || name === "SecurityError") {
    return "Доступ к " + device + " запрещён. Разрешите доступ в настройках Windows (Конфиденциальность) и повторите.";
  }
  if (name === "NotFoundError" || name === "OverconstrainedError") {
    return "Устройство не найдено: " + device + " не обнаружена в системе.";
  }
  if (name === "NotReadableError" || name === "AbortError") {
    return "Не удалось открыть " + device + " — возможно, устройство занято другим приложением.";
  }
  return "Ошибка доступа к " + device + ": " + ((err && err.message) || err);
}

function stopStream(stream) {
  if (stream) stream.getTracks().forEach(function (t) { t.stop(); });
}

function mediaUnavailable(device) {
  if (navigator.mediaDevices && navigator.mediaDevices.getUserMedia) return false;
  showError("WebView не поддерживает доступ к " + device + " (navigator.mediaDevices недоступен).");
  return true;
}

// ---- Экран "Камера" ----
function renderCameraScreen() {
  if (mediaUnavailable("камере")) return;
  var stream = null;
  cleanupScreen = function () { stopStream(stream); stream = null; };

  screen.innerHTML =
    '<div class="test-screen">' +
    "<h2>Камера</h2>" +
    '<p class="hint">Проверьте, что изображение появилось, чёткое, без артефактов и полос.</p>' +
    '<div class="error-text" id="cam-error"></div>' +
    '<video id="cam-video" class="cam-video" autoplay playsinline muted></video>' +
    '<div class="btn-row">' +
    '<button class="btn-primary" id="btn-pass" disabled>Исправно</button>' +
    '<button class="btn-danger" id="btn-fail">Неисправно</button>' +
    '<button class="btn-ghost" id="btn-retry">Повторить</button>' +
    "</div>" +
    "</div>";

  var video = document.getElementById("cam-video");
  var errEl = document.getElementById("cam-error");
  var passBtn = document.getElementById("btn-pass");

  function start() {
    errEl.textContent = "";
    passBtn.disabled = true;
    stopStream(stream);
    navigator.mediaDevices.getUserMedia({ video: true, audio: false })
      .then(function (s) {
        stream = s;
        video.srcObject = s;
        passBtn.disabled = false;
      })
      .catch(function (err) {
        errEl.textContent = mediaErrorText(err, "камере");
      });
  }
  start();

  document.getElementById("btn-retry").addEventListener("click", start);
  document.getElementById("btn-pass").addEventListener("click", function () {
    cleanupScreen();
    setStatus("camera", "pass", "");
  });
  document.getElementById("btn-fail").addEventListener("click", function () {
    var note = errEl.textContent;
    cleanupScreen();
    setStatus("camera", "fail", note);
  });
}

// ---- Экран "Звук" (микрофон: запись и воспроизведение) ----
function renderAudioScreen() {
  if (mediaUnavailable("микрофону")) return;
  if (typeof MediaRecorder === "undefined") {
    showError("WebView не поддерживает MediaRecorder — запись звука недоступна.");
    return;
  }
  var stream = null, recorder = null, chunks = [], audioUrl = null, timer = null;
  cleanupScreen = function () {
    if (timer) clearTimeout(timer);
    if (recorder && recorder.state !== "inactive") recorder.stop();
    stopStream(stream);
    stream = null;
    if (audioUrl) URL.revokeObjectURL(audioUrl);
  };

  screen.innerHTML =
    '<div class="test-screen">' +
    "<h2>Звук</h2>" +
    '<p class="hint">Нажмите «Записать», скажите что-нибудь (5 секунд), затем прослушайте запись — так проверяются микрофон и динамики.</p>' +
    '<div class="error-text" id="aud-error"></div>' +
    '<div class="keylog" id="aud-status">Готово к записи</div>' +
    '<audio id="aud-player" controls style="display:none;width:100%"></audio>' +
    '<div class="btn-row">' +
    '<button class="btn-ghost" id="btn-rec">Записать (5 с)</button>' +
    '<button class="btn-primary" id="btn-pass" disabled>Исправно</button>' +
    '<button class="btn-danger" id="btn-fail">Неисправно</button>' +
    "</div>" +
    "</div>";

  var errEl = document.getElementById("aud-error");
  var statusEl = document.getElementById("aud-status");
  var player = document.getElementById("aud-player");
  var recBtn = document.getElementById("btn-rec");

  recBtn.addEventListener("click", function () {
    errEl.textContent = "";
    recBtn.disabled = true;
    navigator.mediaDevices.getUserMedia({ audio: true, video: false })
      .then(function (s) {
        stream = s;
        chunks = [];
        recorder = new MediaRecorder(s);
        recorder.ondataavailable = function (e) { if (e.data.size) chunks.push(e.data); };
        recorder.onstop = function () {
          stopStream(stream);
          stream = null;
          if (audioUrl) URL.revokeObjectURL(audioUrl);
          if (!chunks.length) {
            statusEl.textContent = "Запись пуста";
            recBtn.disabled = false;
            return;
          }
          audioUrl = URL.createObjectURL(new Blob(chunks, { type: recorder.mimeType }));
          player.src = audioUrl;
          player.style.display = "block";
          statusEl.textContent = "Запись готова — прослушайте";
          recBtn.disabled = false;
          recBtn.textContent = "Записать заново";
          document.getElementById("btn-pass").disabled = false;
          player.play().catch(function () {});
        };
        recorder.start();
        statusEl.textContent = "Идёт запись...";
        timer = setTimeout(function () {
          if (recorder.state !== "inactive") recorder.stop();
        }, 5000);
      })
      .catch(function (err) {
        errEl.textContent = mediaErrorText(err, "микрофону");
        recBtn.disabled = false;
      });
  });

  document.getElementById("btn-pass").addEventListener("click", function () {
    cleanupScreen();
    setStatus("audio", "pass", "");
  });
  document.getElementById("btn-fail").addEventListener("click", function () {
    var note = errEl.textContent;
    cleanupScreen();
    setStatus("audio", "fail", note);
  });
}

// ---- Экран "USB-порты" ----
function renderUsbScreen() {
  var portCount = 3;
  var checked = {};

  function paint(devices, error) {
    var list = "";
    if (error) {
      list = '<div class="error-text">' + escapeHtml(error) + "</div>";
    } else {
      var real = devices.filter(function (d) { return !d.is_hub; });
      list = '<div class="devlist">' + (real.length ? real.map(function (d) {
        var bad = d.status && d.status !== "OK";
        return devRow(d.name || d.device_id, (d.manufacturer || "") + (bad ? " — статус: " + d.status : ""));
      }).join("") : "<p class=\"hint\">USB-устройств не найдено.</p>") + "</div>";
    }

    var ports = "";
    for (var i = 1; i <= portCount; i++) {
      ports += '<label class="port-check"><input type="checkbox" data-port="' + i + '"' +
        (checked[i] ? " checked" : "") + "> Порт " + i + " работает</label>";
    }

    screen.innerHTML =
      '<div class="test-screen">' +
      "<h2>USB-порты</h2>" +
      '<p class="hint">Вставляйте флешку в каждый порт по очереди и нажимайте «Обновить» — устройство должно появиться в списке. Затем отметьте проверенные порты.</p>' +
      list +
      '<div class="btn-row"><button class="btn-ghost" id="btn-refresh">Обновить</button></div>' +
      '<p class="hint">Количество портов: <input type="number" id="port-count" min="1" max="12" value="' + portCount + '" style="width:4em"></p>' +
      '<div class="port-list">' + ports + "</div>" +
      '<div class="btn-row">' +
      '<button class="btn-primary" id="btn-pass">Все порты исправны</button>' +
      '<button class="btn-danger" id="btn-fail">Есть неисправные</button>' +
      "</div></div>";

    document.getElementById("btn-refresh").addEventListener("click", load);
    document.getElementById("port-count").addEventListener("change", function (e) {
      portCount = Math.max(1, Math.min(12, parseInt(e.target.value, 10) || 1));
      paint(devices, error);
    });
    Array.prototype.forEach.call(screen.querySelectorAll("[data-port]"), function (cb) {
      cb.addEventListener("change", function () { checked[cb.dataset.port] = cb.checked; });
    });
    function summary() {
      var ok = [], bad = [];
      for (var i = 1; i <= portCount; i++) { (checked[i] ? ok : bad).push(i); }
      return { ok: ok, bad: bad };
    }
    document.getElementById("btn-pass").addEventListener("click", function () {
      var s = summary();
      var note = s.bad.length ? "Не отмечены порты: " + s.bad.join(", ") : "Проверено портов: " + portCount;
      setStatus("usb", s.bad.length ? "fail" : "pass", note);
    });
    document.getElementById("btn-fail").addEventListener("click", function () {
      var s = summary();
      setStatus("usb", "fail", s.bad.length ? "Не работают/не проверены порты: " + s.bad.join(", ") : "");
    });
  }

  function load() {
    showLoading("Опрос USB-устройств...");
    invoke("list_usb_devices")
      .then(function (devices) { paint(devices, null); })
      .catch(function (err) { paint([], String(err)); });
  }
  load();
}

// ---- Экран "Wi-Fi / Bluetooth" ----
function renderWifiBtScreen() {
  showLoading("Опрос сетевых адаптеров...");
  invoke("get_network_adapters")
    .then(function (adapters) {
      function adapterStatus(a) {
        if (a.error_code) return "Ошибка устройства (код " + a.error_code + ")";
        if (a.enabled === false) return "Отключён";
        if (a.enabled === true) return a.connection_status === 2 ? "Включён, подключён" : "Включён";
        return "Обнаружен";
      }
      function section(kind, title) {
        var list = adapters.filter(function (a) { return a.kind === kind; });
        return "<h3>" + title + "</h3>" + '<div class="devlist">' + (list.length ? list.map(function (a) {
          return devRow(a.name, adapterStatus(a));
        }).join("") : '<p class="hint">Адаптер не обнаружен.</p>') + "</div>";
      }
      var hasWifi = adapters.some(function (a) { return a.kind === "wifi"; });
      var hasBt = adapters.some(function (a) { return a.kind === "bluetooth"; });

      screen.innerHTML =
        '<div class="test-screen">' +
        "<h2>Wi-Fi / Bluetooth</h2>" +
        section("wifi", "Wi-Fi") + section("bluetooth", "Bluetooth") +
        '<p class="hint">Убедитесь, что Wi-Fi видит сети, а Bluetooth находит устройства, затем отметьте результат.</p>' +
        '<div class="btn-row">' +
        '<button class="btn-primary" id="btn-pass">Исправно</button>' +
        '<button class="btn-danger" id="btn-fail">Неисправно</button>' +
        '<button class="btn-ghost" id="btn-skip">Пропустить</button>' +
        "</div></div>";

      var found = "Wi-Fi: " + (hasWifi ? "есть" : "нет") + ", Bluetooth: " + (hasBt ? "есть" : "нет");
      document.getElementById("btn-pass").addEventListener("click", function () {
        setStatus("wifi_bt", "pass", found);
      });
      document.getElementById("btn-fail").addEventListener("click", function () {
        setStatus("wifi_bt", "fail", found);
      });
      document.getElementById("btn-skip").addEventListener("click", function () {
        setStatus("wifi_bt", "skipped", "");
      });
    })
    .catch(function (err) {
      showError(typeof err === "string" ? err : "Не удалось опросить сетевые адаптеры");
    });
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
  camera: renderCameraScreen,
  audio: renderAudioScreen,
  usb: renderUsbScreen,
  wifi_bt: renderWifiBtScreen,
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
