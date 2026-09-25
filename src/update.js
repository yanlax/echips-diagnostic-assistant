// Проверка и загрузка обновления приложения — тот же UX, что в
// echips-driver-assistant (баннер "Доступна версия X" → кнопка
// "Скачать" → прогресс-бар → открыть папку с файлом), но источник —
// GitHub Releases этого репозитория, а не Яндекс.Диск (см. подробное
// обоснование в src-tauri/src/commands/update.rs). Вся сама проверка и
// сравнение версий — на стороне Rust (invoke('check_for_update')),
// здесь только отрисовка баннера и запуск скачивания.
//
// Слот #update-banner-slot — статичный, вне #screen (как #theme-toggle-slot),
// чтобы render() в app.js не затирал баннер при каждой перерисовке экрана.
(function () {
  function escapeHtml(text) {
    var div = document.createElement("div");
    div.textContent = text;
    return div.innerHTML;
  }

  function mountBanner(info) {
    var host = document.getElementById("update-banner-slot");
    if (!host || host.querySelector(".update-banner")) return;
    var el = document.createElement("div");
    el.className = "update-banner";
    el.innerHTML =
      '<span class="update-banner-text">Доступна новая версия ' + escapeHtml(info.version) + "</span>" +
      '<div class="update-banner-actions">' +
      '<button type="button" class="btn-link" id="update-download-btn">Скачать</button>' +
      '<button type="button" class="btn-link" id="update-dismiss-btn">×</button>' +
      "</div>";
    host.appendChild(el);

    var textEl = el.querySelector(".update-banner-text");
    var downloadBtn = el.querySelector("#update-download-btn");
    var dismissBtn = el.querySelector("#update-dismiss-btn");

    dismissBtn.addEventListener("click", function () { host.innerHTML = ""; });

    downloadBtn.addEventListener("click", function () {
      downloadBtn.style.pointerEvents = "none";
      var unlisten = null;
      window.__TAURI__.event.listen("app-update-progress", function (event) {
        var p = event.payload;
        if (p.total > 0) {
          var mbDone = (p.downloaded / (1024 * 1024)).toFixed(1);
          var mbTotal = (p.total / (1024 * 1024)).toFixed(1);
          textEl.textContent = "Загрузка обновления… " + mbDone + " / " + mbTotal + " МБ";
        }
      }).then(function (fn) { unlisten = fn; });

      // Имя с версией: если программа запущена из «Загрузок» под тем же именем,
      // Windows не даёт перезаписать запущенный exe — скачивание падало.
      var newName = info.file_name.replace(/\.exe$/i, "") + "-" + info.version + ".exe";
      window.__TAURI__.core.invoke("download_update", { url: info.download_url, fileName: newName })
        .then(function (savedPath) {
          if (unlisten) unlisten();
          textEl.textContent = "Скачано — открываю папку…";
          return window.__TAURI__.core.invoke("open_containing_folder", { path: savedPath });
        })
        .then(function () {
          setTimeout(function () { host.innerHTML = ""; }, 1500);
        })
        .catch(function (err) {
          if (unlisten) unlisten();
          textEl.textContent = "Не удалось скачать обновление" + (typeof err === "string" ? ": " + err : "");
          downloadBtn.style.pointerEvents = "";
        });
    });
  }

  function checkForUpdate() {
    if (!window.__TAURI__) return; // браузерный стенд без Tauri — не проверяем
    window.__TAURI__.core.invoke("check_for_update").then(function (info) {
      if (info) mountBanner(info);
    }).catch(function () {
      // Тихо игнорируем — фоновая проверка не должна мешать диагностике.
    });
  }

  // связь появилась позже (ноутбук подключили к сети уже на экране PIN) — проверяем ещё раз; повторный баннер не создаётся
  window.addEventListener("online", checkForUpdate);
  // на экране PIN ноутбук может простоять долго — раз в 30 минут проверяем снова
  setInterval(checkForUpdate, 30 * 60 * 1000);

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", checkForUpdate);
  } else {
    checkForUpdate();
  }
})();
