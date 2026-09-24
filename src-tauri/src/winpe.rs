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
    // Диск WinPE обычно X:, но не всегда — проверяем и по %SystemRoot%, и по системному диску.
    let sysroot = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"X:\Windows"));
    sysroot.join(r"System32\winpeshl.exe").exists()
        || Path::new(r"X:\Windows\System32\winpeshl.exe").exists()
        || std::env::var("SystemDrive").map_or(false, |d| d.eq_ignore_ascii_case("X:"))
}

/// Лог запуска — пишется рядом с exe и во временную папку. Нужен потому, что при
/// ошибке создания окна (например, в WinPE без WebView2) процесс раньше просто
/// закрывался без единого следа.
pub fn log(msg: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::io::Write;
        let line = format!("{} · {msg}\r\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
        let mut targets: Vec<PathBuf> = vec![];
        if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
            targets.push(dir.join("echips-startup.log"));
        }
        if let Some(t) = std::env::var_os("TEMP").or_else(|| std::env::var_os("TMP")) {
            targets.push(PathBuf::from(t).join("echips-startup.log"));
        }
        for t in targets {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(t) {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = msg;
    }
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


/// Перехват «тяжёлых» исключений процесса (0xC…: нарушение доступа, fast-fail и т. п.) —
/// пишет код, адрес и модуль (DLL), в котором случился сбой. В WinPE процесс умирал через
/// 1–2 с после Ready без паники Rust и без событий закрытия — это признак нативного сбоя
/// внутри библиотеки WebView2, и именно этот лог покажет, в какой.
#[cfg(target_os = "windows")]
mod crash {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[repr(C)]
    struct ExceptionRecord {
        code: u32,
        flags: u32,
        record: *mut ExceptionRecord,
        address: *mut c_void,
        n_params: u32,
        info: [usize; 15],
    }
    #[repr(C)]
    struct ExceptionPointers {
        record: *mut ExceptionRecord,
        context: *mut c_void,
    }

    extern "system" {
        fn AddVectoredExceptionHandler(first: u32, handler: unsafe extern "system" fn(*mut ExceptionPointers) -> i32) -> *mut c_void;
        fn GetModuleHandleExW(flags: u32, addr: *const c_void, module: *mut *mut c_void) -> i32;
        fn GetModuleFileNameW(module: *mut c_void, buf: *mut u16, size: u32) -> u32;
    }

    static SEEN: AtomicU32 = AtomicU32::new(0);

    unsafe extern "system" fn handler(p: *mut ExceptionPointers) -> i32 {
        if p.is_null() || (*p).record.is_null() {
            return 0;
        }
        let rec = &*(*p).record;
        // только серьёзные (0xC…), и не больше 40 записей, чтобы не засорить лог безобидными
        if rec.code >= 0xC000_0000 && SEEN.fetch_add(1, Ordering::Relaxed) < 40 {
            let mut module = String::from("?");
            let mut h: *mut c_void = std::ptr::null_mut();
            // GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS (4) | UNCHANGED_REFCOUNT (2)
            if GetModuleHandleExW(6, rec.address as *const c_void, &mut h) != 0 && !h.is_null() {
                let mut buf = [0u16; 512];
                let n = GetModuleFileNameW(h, buf.as_mut_ptr(), buf.len() as u32) as usize;
                if n > 0 {
                    module = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
                }
            }
            super::log(&format!(
                "исключение {:#010X} по адресу {:p}, модуль {module}, параметры {:?}",
                rec.code,
                rec.address,
                &rec.info[..(rec.n_params as usize).min(3)]
            ));
        }
        0 // EXCEPTION_CONTINUE_SEARCH — обработку не подменяем
    }

    pub fn install() {
        unsafe {
            AddVectoredExceptionHandler(1, handler);
        }
    }
}

pub fn prepare() {
    #[cfg(target_os = "windows")]
    {
        crash::install();
        log(&format!(
            "старт: pid={}, exe={:?}, WinPE={}, SystemRoot={:?}, LOCALAPPDATA={:?}, TEMP={:?}",
            std::process::id(),
            std::env::current_exe().ok(),
            is_winpe(),
            std::env::var_os("SystemRoot"),
            std::env::var_os("LOCALAPPDATA"),
            std::env::var_os("TEMP"),
        ));
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
                                log(&format!("WebView2 (переносимый): {}", folder.display()));
                                std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", folder);
                                break;
                            } else {
                                log(&format!("папка {} похожа на WebView2, но msedgewebview2.exe в ней не найден", p.display()));
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

        if std::env::var_os("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER").is_none() {
            log("рядом с exe нет папки с переносимым WebView2 (в имени должно быть «webview2», внутри msedgewebview2.exe) — будет искаться установленный");
        }
        // Флаги нужны в WinPE; для проверки на обычной Windows можно принудительно: ECHIPS_SAFE_WEBVIEW=1
        if (is_winpe() || std::env::var_os("ECHIPS_SAFE_WEBVIEW").is_some())
            && std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none()
        {
            // Журнал самого Chromium (--enable-logging) — по нему видно, почему не стартует
            // браузерный процесс WebView2 (в WinPE процесс приложения перезапускался каждую секунду
            // после «окно создано» без каких-либо паник).
            let chromium_log = std::env::var_os("TEMP")
                .or_else(|| std::env::var_os("TMP"))
                .map(|t| PathBuf::from(t).join("echips-webview2.log"))
                .unwrap_or_else(|| PathBuf::from(r"X:\Windows\Temp\echips-webview2.log"));
            let args = format!(
                "--no-sandbox --disable-gpu --disable-gpu-compositing --enable-logging --v=1 --log-file={}",
                chromium_log.display()
            );
            std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", &args);
            log(&format!("включены флаги браузера для WinPE: {args}"));
        }
        log(&format!(
            "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER={:?}, WEBVIEW2_USER_DATA_FOLDER={:?}",
            std::env::var_os("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER"),
            std::env::var_os("WEBVIEW2_USER_DATA_FOLDER"),
        ));
    }
}

/// Страница загрузилась (ставит on_page_load в lib.rs) — сторожевой поток тогда молчит.
pub static PAGE_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Сторожевой поток: если за 20 с страница так и не загрузилась (в WinPE процесс приложения
/// зависал без окна и без ошибок), пишет в лог, что происходит — процессы msedgewebview2/приложения
/// и содержимое папки данных WebView2 и файла журнала Chromium. Так причина видна без ручной отладки.
pub fn start_watchdog() {
    #[cfg(target_os = "windows")]
    {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(20));
            if PAGE_LOADED.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            log("сторож: за 20 с страница так и не загрузилась");
            use std::os::windows::process::CommandExt;
            if let Ok(out) = std::process::Command::new("tasklist").args(["/fo", "csv", "/nh"]).creation_flags(0x0800_0000).output() {
                let text = String::from_utf8_lossy(&out.stdout);
                let mine: Vec<&str> = text
                    .lines()
                    .filter(|l| {
                        let low = l.to_lowercase();
                        low.contains("msedgewebview2") || low.contains("echips")
                    })
                    .collect();
                log(&format!("сторож: процессы ({}): {}", mine.len(), mine.join(" | ")));
            }
            if let Some(dir) = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER") {
                let names: Vec<String> = std::fs::read_dir(&dir)
                    .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
                    .unwrap_or_default();
                log(&format!("сторож: папка данных WebView2 {:?}: {:?}", dir, names));
            }
            if let Some(t) = std::env::var_os("TEMP") {
                let p = std::path::PathBuf::from(t).join("echips-webview2.log");
                log(&format!("сторож: журнал Chromium {} — {}", p.display(), if p.exists() { "есть" } else { "не создан (браузерный процесс не стартовал)" }));
            }
        });
    }
}
