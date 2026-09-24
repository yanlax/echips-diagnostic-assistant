// Запуск в WinPE / на системе без установленного Edge WebView2.
//
// Окно приложения — это WebView2. В WinPE (и на «голой» Windows без Edge) его
// нет, и Tauri падает с «Could not find the WebView2 Runtime». Решение —
// переносимая (Fixed Version) копия WebView2 рядом с exe: папка вида
// `WebView2` или `Microsoft.WebView2.FixedVersionRuntime.*` (внутри —
// msedgewebview2.exe). Если такая папка найдена, до создания окна выставляем
// WEBVIEW2_BROWSER_EXECUTABLE_FOLDER — загрузчик WebView2 сам читает эту
// переменную. Дополнительно в WinPE: нет профиля пользователя (LOCALAPPDATA) —
// подставляем временную папку, а для браузера отключаем песочницу и GPU.
//
// Что кладётся рядом с exe и какие компоненты нужны в образе WinPE — см.
// README, раздел «Работа в WinPE».

#[cfg(target_os = "windows")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
fn is_winpe() -> bool {
    Path::new(r"X:\Windows\System32\winpeshl.exe").exists()
}

/// Папка, в которой лежит msedgewebview2.exe: сама `dir` или её подпапка.
#[cfg(target_os = "windows")]
fn runtime_folder(dir: &Path) -> Option<PathBuf> {
    if dir.join("msedgewebview2.exe").exists() {
        return Some(dir.to_path_buf());
    }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() && p.join("msedgewebview2.exe").exists() {
            return Some(p);
        }
    }
    None
}

pub fn prepare() {
    #[cfg(target_os = "windows")]
    {
        // В WinPE нет %LOCALAPPDATA%: все логи/кэши приложения читают эту переменную.
        if std::env::var_os("LOCALAPPDATA").is_none() {
            let tmp = std::env::var_os("TEMP").or_else(|| std::env::var_os("TMP")).unwrap_or_else(|| r"X:\Windows\Temp".into());
            std::env::set_var("LOCALAPPDATA", tmp);
        }

        if std::env::var_os("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER").is_none() {
            if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
                if let Ok(rd) = std::fs::read_dir(&exe_dir) {
                    for entry in rd.flatten() {
                        let p = entry.path();
                        let name = entry.file_name().to_string_lossy().to_lowercase();
                        if p.is_dir() && name.contains("webview2") {
                            if let Some(folder) = runtime_folder(&p) {
                                std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", folder);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Папка данных WebView2 должна быть доступна на запись (по умолчанию —
        // рядом с exe или в профиле, а в WinPE их может не быть).
        if std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_none() {
            if let Some(base) = std::env::var_os("LOCALAPPDATA") {
                let dir = PathBuf::from(base).join("Echips").join("HardwareCheck").join("webview2");
                let _ = std::fs::create_dir_all(&dir);
                std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", dir);
            }
        }

        if is_winpe() && std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
            std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--no-sandbox --disable-gpu --disable-gpu-compositing");
        }
    }
}
