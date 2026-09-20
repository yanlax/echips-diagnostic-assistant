// Переключение тёмной/светлой темы — та же схема, что в echips-driver-assistant:
// атрибут data-theme на <html>, сохранение выбора в localStorage.
(function () {
  var STORAGE_KEY = "echips-diagnostic-theme";

  function applyStoredTheme() {
    try {
      var saved = localStorage.getItem(STORAGE_KEY);
      if (saved === "light" || saved === "dark") {
        document.documentElement.setAttribute("data-theme", saved);
      }
    } catch (e) {
      // localStorage может быть недоступен — остаёмся на теме по умолчанию (dark)
    }
  }

  window.EchipsTheme = {
    get: function () {
      return document.documentElement.getAttribute("data-theme") || "dark";
    },
    set: function (theme) {
      document.documentElement.setAttribute("data-theme", theme);
      try { localStorage.setItem(STORAGE_KEY, theme); } catch (e) {}
    },
    toggle: function () {
      var next = window.EchipsTheme.get() === "dark" ? "light" : "dark";
      window.EchipsTheme.set(next);
      return next;
    }
  };

  applyStoredTheme();
})();
