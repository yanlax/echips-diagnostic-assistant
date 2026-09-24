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

/// Работаем ли в WinPE (диск X: — это ОЗУ, всё, что на нём, пропадает при перезагрузке).
pub fn in_winpe() -> bool {
    #[cfg(target_os = "windows")]
    {
        is_winpe()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

#[cfg(target_os = "windows")]
fn is_winpe() -> bool {
    // Диск WinPE обычно X:, но не всегда — проверяем и по %SystemRoot%, и по системному диску.
    let sysroot = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"X:\Windows"));
    sysroot.join(r"System32\winpeshl.exe").exists()
        || Path::new(r"X:\Windows\System32\winpeshl.exe").exists()
        || std::env::var("SystemDrive").map_or(false, |d| d.eq_ignore_ascii_case("X:"))
}

/// Окно с текстом ошибки — приложение без окна WebView2 иначе молча исчезает.
pub fn message_box(title: &str, text: &str) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let t: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let x: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MessageBoxW(std::ptr::null_mut(), x.as_ptr(), t.as_ptr(), MB_OK | MB_ICONERROR);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, text);
    }
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

        // Флаги нужны в WinPE; для проверки на обычной Windows можно принудительно: ECHIPS_SAFE_WEBVIEW=1
        if (is_winpe() || std::env::var_os("ECHIPS_SAFE_WEBVIEW").is_some())
            && std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none()
        {
            std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--no-sandbox --disable-gpu --disable-gpu-compositing");
        }
    }
}
