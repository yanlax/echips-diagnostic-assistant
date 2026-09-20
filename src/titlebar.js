// Кастомная шапка окна: свернуть/развернуть/закрыть через Tauri API.
// Окно растягиваемое (resizable/maximizable: true в tauri.conf.json), т.к.
// некоторым тестам (дисплей, стресс-тест) полезен полноэкранный/крупный вид.
(function () {
  var getCurrentWindow = window.__TAURI__.window.getCurrentWindow;

  async function mount() {
    var win = getCurrentWindow();
    var minBtn = document.getElementById("win-minimize");
    var maxBtn = document.getElementById("win-maximize");
    var closeBtn = document.getElementById("win-close");
    var dragZone = document.querySelector(".titlebar-drag");

    if (minBtn) minBtn.addEventListener("click", function () { win.minimize(); });
    if (closeBtn) closeBtn.addEventListener("click", function () { win.close(); });
    if (maxBtn) maxBtn.addEventListener("click", function () { win.toggleMaximize(); });
    if (dragZone) {
      dragZone.addEventListener("dblclick", function () { win.toggleMaximize(); });
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", mount);
  } else {
    mount();
  }
})();
